//! Minimal CDP client over a loopback WebSocket (BRW-04).
//!
//! Hygiene is enforced in one place: [`CleanCdpSession::send`] refuses methods in [`FORBIDDEN_METHODS`]
//! (most importantly `Runtime.enable`, a classic anti-bot tell), and every command actually sent is
//! recorded in a protocol log that tests and diagnostics can inspect.

use super::BrowserError;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// CDP methods that must never be sent. `Runtime.enable` makes `consoleAPICalled` serialization
/// observable from page scripts; `Runtime.evaluate` works without it.
pub const FORBIDDEN_METHODS: &[&str] = &["Runtime.enable"];

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn check_method(method: &str) -> Result<(), BrowserError> {
    if FORBIDDEN_METHODS.contains(&method) {
        Err(BrowserError::ForbiddenCdpMethod(method.to_string()))
    } else {
        Ok(())
    }
}

type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value, BrowserError>>>>>;

pub struct CleanCdpSession {
    next_id: AtomicU64,
    tx: mpsc::UnboundedSender<Message>,
    pending: Pending,
    log: Arc<StdMutex<Vec<String>>>,
    closed: Arc<AtomicBool>,
}

impl CleanCdpSession {
    pub async fn connect(ws_url: &str) -> Result<Self, BrowserError> {
        if !(ws_url.starts_with("ws://127.0.0.1:") || ws_url.starts_with("ws://localhost:")) {
            return Err(BrowserError::Other(format!("refusing non-loopback debugging endpoint: {ws_url}")));
        }
        let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(ws_url))
            .await
            .map_err(|_| BrowserError::Timeout("connecting to Chrome's debugging WebSocket".into()))?
            .map_err(|e| BrowserError::Other(format!("websocket connect failed: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));

        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if sink.send(m).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let reader_pending = pending.clone();
        let reader_closed = closed.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                let Some(id) = v.get("id").and_then(Value::as_u64) else { continue };
                let result = match v.get("error") {
                    Some(err) => Err(BrowserError::CdpProtocol {
                        code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
                        message: err.get("message").and_then(Value::as_str).unwrap_or("unknown").to_string(),
                    }),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(waiter) = reader_pending.lock().unwrap().remove(&id) {
                    let _ = waiter.send(result);
                }
            }
            reader_closed.store(true, Ordering::SeqCst);
            for (_, waiter) in reader_pending.lock().unwrap().drain() {
                let _ = waiter.send(Err(BrowserError::Disconnected));
            }
        });

        Ok(Self { next_id: AtomicU64::new(1), tx, pending, log: Arc::default(), closed })
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Every method name sent so far, in order (BRW-04: "verified by protocol log").
    pub fn protocol_log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub async fn send(&self, method: &str, params: Value, session_id: Option<&str>) -> Result<Value, BrowserError> {
        check_method(method)?;
        if self.is_closed() {
            return Err(BrowserError::Disconnected);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut payload = json!({ "id": id, "method": method, "params": params });
        if let Some(sid) = session_id {
            payload["sessionId"] = Value::String(sid.to_string());
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, resp_tx);
        self.log.lock().unwrap().push(method.to_string());
        if self.tx.send(Message::text(payload.to_string())).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(BrowserError::Disconnected);
        }

        match tokio::time::timeout(REQUEST_TIMEOUT, resp_rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(BrowserError::Disconnected),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(BrowserError::Timeout(format!("CDP {method}")))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! Tiny in-process CDP endpoint used to verify the wire protocol without a real Chrome.
    use super::*;
    use tokio::net::TcpListener;

    pub struct FakeCdp {
        pub url: String,
        pub received: Arc<StdMutex<Vec<Value>>>,
    }

    pub async fn spawn(respond: impl Fn(&Value) -> Value + Send + Sync + 'static) -> FakeCdp {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://127.0.0.1:{}/devtools/browser/test", listener.local_addr().unwrap().port());
        let received: Arc<StdMutex<Vec<Value>>> = Arc::default();
        let rec = received.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(t))) = ws.next().await {
                let req: Value = serde_json::from_str(t.as_str()).unwrap();
                rec.lock().unwrap().push(req.clone());
                let mut resp = respond(&req);
                resp["id"] = req["id"].clone();
                if ws.send(Message::text(resp.to_string())).await.is_err() {
                    break;
                }
                if req["method"] == "Browser.close" {
                    break;
                }
            }
        });
        FakeCdp { url, received }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_enable_is_forbidden() {
        assert!(matches!(check_method("Runtime.enable"), Err(BrowserError::ForbiddenCdpMethod(_))));
        assert!(check_method("Runtime.evaluate").is_ok());
        assert!(check_method("Page.captureScreenshot").is_ok());
    }

    #[tokio::test]
    async fn guard_blocks_runtime_enable_before_it_reaches_the_wire() {
        let fake = test_server::spawn(|_| json!({ "result": {} })).await;
        let s = CleanCdpSession::connect(&fake.url).await.unwrap();
        let err = s.send("Runtime.enable", json!({}), None).await.unwrap_err();
        assert!(matches!(err, BrowserError::ForbiddenCdpMethod(_)));
        s.send("Browser.getVersion", json!({}), None).await.unwrap();
        assert_eq!(s.protocol_log(), vec!["Browser.getVersion"]);
        let wire: Vec<_> = fake.received.lock().unwrap().iter().map(|r| r["method"].as_str().unwrap().to_string()).collect();
        assert_eq!(wire, vec!["Browser.getVersion"], "Runtime.enable never hit the socket");
    }

    #[tokio::test]
    async fn correlates_results_errors_and_session_ids() {
        let fake = test_server::spawn(|req| match req["method"].as_str().unwrap() {
            "Target.attachToTarget" => json!({ "result": { "sessionId": "S1" } }),
            "Page.navigate" => json!({ "error": { "code": -32000, "message": "nope" } }),
            _ => json!({ "result": { "echo": req["sessionId"] } }),
        })
        .await;
        let s = CleanCdpSession::connect(&fake.url).await.unwrap();
        let r = s.send("Target.attachToTarget", json!({ "targetId": "T", "flatten": true }), None).await.unwrap();
        assert_eq!(r["sessionId"], "S1");
        let r = s.send("Page.captureScreenshot", json!({}), Some("S1")).await.unwrap();
        assert_eq!(r["echo"], "S1");
        let e = s.send("Page.navigate", json!({ "url": "x" }), Some("S1")).await.unwrap_err();
        assert!(matches!(e, BrowserError::CdpProtocol { code: -32000, .. }));
    }

    #[tokio::test]
    async fn disconnect_fails_pending_and_future_calls() {
        let fake = test_server::spawn(|_| json!({ "result": {} })).await;
        let s = CleanCdpSession::connect(&fake.url).await.unwrap();
        let _ = s.send("Browser.close", json!({}), None).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(s.is_closed());
        assert!(matches!(s.send("Browser.getVersion", json!({}), None).await, Err(BrowserError::Disconnected)));
    }

    #[tokio::test]
    async fn refuses_non_loopback_endpoints() {
        assert!(CleanCdpSession::connect("ws://example.com:9222/devtools/browser/x").await.is_err());
    }
}
