//! `ChromeCdpDriver`: genuine Chrome in `--app` mode, controlled through [`CleanCdpSession`].
//!
//! Observation and input use only clean primitives. `Runtime.enable` is never sent; scripts run through
//! `Runtime.evaluate` inside a dedicated isolated world created with `Page.createIsolatedWorld`.

use super::cdp::CleanCdpSession;
use super::launcher::{self, StaleRecord};
use super::{BrowserDriver, BrowserError, ChromeLaunchConfig, WindowBounds};
use base64::Engine;
use serde_json::{json, Value};
use std::process::Child;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

const ENDPOINT_WAIT: Duration = Duration::from_secs(25);
const TARGET_WAIT: Duration = Duration::from_secs(15);
const CLOSE_WAIT: Duration = Duration::from_secs(10);
const WORLD_NAME: &str = "sgh-isolated";

struct Live {
    session: Arc<CleanCdpSession>,
    target_id: String,
    page_session: String,
    profile_dir: std::path::PathBuf,
}

#[derive(Default)]
pub struct ChromeCdpDriver {
    live: Option<Live>,
    child: StdMutex<Option<Child>>,
    world: StdMutex<Option<i64>>,
}

impl ChromeCdpDriver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Methods sent over the wire so far (for diagnostics and the live protocol-hygiene test).
    pub fn protocol_log(&self) -> Vec<String> {
        self.live.as_ref().map(|l| l.session.protocol_log()).unwrap_or_default()
    }

    fn live(&self) -> Result<&Live, BrowserError> {
        match &self.live {
            Some(l) if !l.session.is_closed() => Ok(l),
            _ => Err(BrowserError::Disconnected),
        }
    }

    async fn page_call(&self, method: &str, params: Value) -> Result<Value, BrowserError> {
        let l = self.live()?;
        l.session.send(method, params, Some(&l.page_session)).await
    }

    async fn create_world(&self) -> Result<i64, BrowserError> {
        let tree = self.page_call("Page.getFrameTree", json!({})).await?;
        let frame_id = tree["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or_else(|| BrowserError::Other("frame tree had no main frame".into()))?
            .to_string();
        let r = self
            .page_call("Page.createIsolatedWorld", json!({ "frameId": frame_id, "worldName": WORLD_NAME }))
            .await?;
        let ctx = r["executionContextId"]
            .as_i64()
            .ok_or_else(|| BrowserError::Other("no executionContextId from createIsolatedWorld".into()))?;
        *self.world.lock().unwrap() = Some(ctx);
        Ok(ctx)
    }

    async fn evaluate_in_world(&self, expression: &str, ctx: i64) -> Result<Value, BrowserError> {
        let r = self
            .page_call(
                "Runtime.evaluate",
                json!({ "expression": expression, "contextId": ctx, "returnByValue": true, "awaitPromise": true }),
            )
            .await?;
        if let Some(ex) = r.get("exceptionDetails") {
            let msg = ex["exception"]["description"]
                .as_str()
                .or_else(|| ex["text"].as_str())
                .unwrap_or("script threw")
                .to_string();
            return Err(BrowserError::Evaluation(msg));
        }
        Ok(r["result"]["value"].clone())
    }

    async fn wait_until_loaded(&self) -> Result<(), BrowserError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        loop {
            if let Ok(Value::String(s)) = self.eval_json("document.readyState").await {
                if s == "complete" {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(BrowserError::Timeout("page load".into()));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    fn take_child(&self) -> Option<Child> {
        self.child.lock().unwrap().take()
    }

    async fn wait_for_endpoint(&self, port: u16) -> Result<Value, BrowserError> {
        let deadline = tokio::time::Instant::now() + ENDPOINT_WAIT;
        loop {
            if let Some(child) = self.child.lock().unwrap().as_mut() {
                if let Some(status) = child.try_wait()? {
                    return Err(BrowserError::ChromeExitedEarly(status.to_string()));
                }
            }
            if let Ok(v) = super::http::get_json(port, "/json/version", Duration::from_secs(2)).await {
                if v["webSocketDebuggerUrl"].is_string() {
                    return Ok(v);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(BrowserError::Timeout("Chrome's debugging endpoint".into()));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn find_page_target(session: &CleanCdpSession, start_url: &str) -> Result<String, BrowserError> {
        let host = start_url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or("");
        let deadline = tokio::time::Instant::now() + TARGET_WAIT;
        loop {
            let r = session.send("Target.getTargets", json!({}), None).await?;
            let pages: Vec<&Value> = r["targetInfos"]
                .as_array()
                .map(|a| a.iter().filter(|t| t["type"] == "page").collect())
                .unwrap_or_default();
            let pick = pages
                .iter()
                .find(|t| !host.is_empty() && t["url"].as_str().map(|u| u.contains(host)).unwrap_or(false))
                .or_else(|| pages.first());
            if let Some(t) = pick {
                if let Some(id) = t["targetId"].as_str() {
                    return Ok(id.to_string());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(BrowserError::NoPageTarget);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

impl Drop for ChromeCdpDriver {
    fn drop(&mut self) {
        // Last resort only (panic or abrupt teardown). Normal exit goes through close_gracefully.
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[async_trait::async_trait]
impl BrowserDriver for ChromeCdpDriver {
    async fn launch(&mut self, cfg: ChromeLaunchConfig) -> Result<(), BrowserError> {
        if self.live.is_some() && self.is_running() {
            return Err(BrowserError::AlreadyRunning);
        }
        self.live = None;
        launcher::reclaim_stale_chrome(&cfg.profile_dir).await;

        let port = launcher::allocate_loopback_port()?;
        let child = launcher::spawn_clean_chrome(&cfg, port)?;
        launcher::write_record(&StaleRecord {
            pid: child.id(),
            port,
            profile_dir: cfg.profile_dir.display().to_string(),
        })?;
        *self.child.lock().unwrap() = Some(child);

        let result: Result<Live, BrowserError> = async {
            let version = self.wait_for_endpoint(port).await?;
            let ws = version["webSocketDebuggerUrl"].as_str().expect("checked in wait_for_endpoint");
            let session = Arc::new(CleanCdpSession::connect(ws).await?);
            let target_id = Self::find_page_target(&session, &cfg.start_url).await?;
            let attached = session
                .send("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }), None)
                .await?;
            let page_session = attached["sessionId"]
                .as_str()
                .ok_or_else(|| BrowserError::Other("attachToTarget returned no sessionId".into()))?
                .to_string();
            Ok(Live { session, target_id, page_session, profile_dir: cfg.profile_dir.clone() })
        }
        .await;

        match result {
            Ok(live) => {
                self.live = Some(live);
                Ok(())
            }
            Err(e) => {
                if let Some(mut c) = self.take_child() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                launcher::clear_record(&cfg.profile_dir);
                Err(e)
            }
        }
    }

    fn is_running(&self) -> bool {
        self.live.as_ref().map(|l| !l.session.is_closed()).unwrap_or(false)
    }

    async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        let r = self.page_call("Page.navigate", json!({ "url": url })).await?;
        if let Some(err) = r["errorText"].as_str() {
            return Err(BrowserError::Other(format!("navigation failed: {err}")));
        }
        *self.world.lock().unwrap() = None;
        self.wait_until_loaded().await
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        let l = self.live()?;
        let r = l.session.send("Target.getTargetInfo", json!({ "targetId": l.target_id }), None).await?;
        Ok(r["targetInfo"]["url"].as_str().unwrap_or_default().to_string())
    }

    async fn eval_json(&self, expression: &str) -> Result<Value, BrowserError> {
        let cached = *self.world.lock().unwrap();
        if let Some(ctx) = cached {
            match self.evaluate_in_world(expression, ctx).await {
                Err(BrowserError::CdpProtocol { .. }) => {}
                other => return other,
            }
        }
        let ctx = self.create_world().await?;
        self.evaluate_in_world(expression, ctx).await
    }

    async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError> {
        let r = self.page_call("Page.captureScreenshot", json!({ "format": "png" })).await?;
        let data = r["data"].as_str().ok_or_else(|| BrowserError::Other("screenshot had no data".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| BrowserError::Other(format!("bad screenshot encoding: {e}")))
    }

    async fn ax_tree(&self, max_depth: Option<u32>) -> Result<Value, BrowserError> {
        let params = match max_depth {
            Some(d) => json!({ "depth": d }),
            None => json!({}),
        };
        self.page_call("Accessibility.getFullAXTree", params).await
    }

    async fn click_point(&self, x: f64, y: f64) -> Result<(), BrowserError> {
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y })).await?;
        self.page_call(
            "Input.dispatchMouseEvent",
            json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
        )
        .await?;
        self.page_call(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
        )
        .await?;
        Ok(())
    }

    async fn wheel_scroll(&self, x: f64, y: f64, delta_y: f64) -> Result<(), BrowserError> {
        self.page_call(
            "Input.dispatchMouseEvent",
            json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": delta_y }),
        )
        .await?;
        Ok(())
    }

    async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
        self.page_call("Input.insertText", json!({ "text": text })).await?;
        Ok(())
    }

    async fn set_bounds(&self, bounds: WindowBounds) -> Result<(), BrowserError> {
        let l = self.live()?;
        let w = l.session.send("Browser.getWindowForTarget", json!({ "targetId": l.target_id }), None).await?;
        let window_id = w["windowId"].as_i64().ok_or_else(|| BrowserError::Other("no windowId".into()))?;
        l.session
            .send(
                "Browser.setWindowBounds",
                json!({
                    "windowId": window_id,
                    "bounds": {
                        "left": bounds.left, "top": bounds.top,
                        "width": bounds.width, "height": bounds.height,
                        "windowState": "normal"
                    }
                }),
                None,
            )
            .await?;
        Ok(())
    }

    async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
        let Some(live) = self.live.take() else {
            if let Some(mut c) = self.take_child() {
                let _ = c.kill();
                let _ = c.wait();
            }
            return Ok(());
        };
        *self.world.lock().unwrap() = None;

        // Browser.close tears the socket down mid-reply, so a Disconnected error here means success.
        let _ = live.session.send("Browser.close", json!({}), None).await;

        let mut child = self.take_child();
        let mut exited_cleanly = child.is_none();
        if let Some(c) = child.as_mut() {
            let deadline = tokio::time::Instant::now() + CLOSE_WAIT;
            while tokio::time::Instant::now() < deadline {
                if c.try_wait()?.is_some() {
                    exited_cleanly = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if !exited_cleanly {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        launcher::clear_record(&live.profile_dir);
        if exited_cleanly {
            Ok(())
        } else {
            Err(BrowserError::Timeout("Chrome did not exit after Browser.close; it was terminated".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::cdp::test_server;
    use super::*;

    /// Builds a driver already attached to a fake CDP endpoint, as `launch` would leave it.
    async fn attached_driver(
        respond: impl Fn(&Value) -> Value + Send + Sync + 'static,
    ) -> (ChromeCdpDriver, Arc<StdMutex<Vec<Value>>>) {
        let fake = test_server::spawn(respond).await;
        let session = Arc::new(CleanCdpSession::connect(&fake.url).await.unwrap());
        let mut driver = ChromeCdpDriver::new();
        driver.live = Some(Live {
            session,
            target_id: "T1".into(),
            page_session: "S1".into(),
            profile_dir: std::env::temp_dir().join("sgh-test-profile"),
        });
        (driver, fake.received)
    }

    fn methods(rec: &Arc<StdMutex<Vec<Value>>>) -> Vec<String> {
        rec.lock().unwrap().iter().map(|r| r["method"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test]
    async fn eval_uses_isolated_world_without_runtime_enable() {
        let (d, rec) = attached_driver(|req| match req["method"].as_str().unwrap() {
            "Page.getFrameTree" => json!({ "result": { "frameTree": { "frame": { "id": "F1" } } } }),
            "Page.createIsolatedWorld" => json!({ "result": { "executionContextId": 7 } }),
            "Runtime.evaluate" => json!({ "result": { "result": { "type": "number", "value": 42 } } }),
            _ => json!({ "result": {} }),
        })
        .await;
        assert_eq!(d.eval_json("6*7").await.unwrap(), json!(42));
        assert_eq!(d.eval_json("6*7").await.unwrap(), json!(42));
        let m = methods(&rec);
        assert_eq!(m, vec!["Page.getFrameTree", "Page.createIsolatedWorld", "Runtime.evaluate", "Runtime.evaluate"]);
        assert!(!m.iter().any(|x| x == "Runtime.enable"));
        let eval = rec.lock().unwrap().iter().find(|r| r["method"] == "Runtime.evaluate").cloned().unwrap();
        assert_eq!(eval["sessionId"], "S1");
        assert_eq!(eval["params"]["contextId"], 7);
        assert_eq!(eval["params"]["returnByValue"], true);
    }

    #[tokio::test]
    async fn eval_recreates_world_after_navigation_invalidates_it() {
        let calls = Arc::new(StdMutex::new(0u32));
        let c2 = calls.clone();
        let (d, rec) = attached_driver(move |req| match req["method"].as_str().unwrap() {
            "Page.getFrameTree" => json!({ "result": { "frameTree": { "frame": { "id": "F1" } } } }),
            "Page.createIsolatedWorld" => json!({ "result": { "executionContextId": 9 } }),
            "Runtime.evaluate" => {
                let mut n = c2.lock().unwrap();
                *n += 1;
                if *n == 2 {
                    json!({ "error": { "code": -32000, "message": "Cannot find context with specified id" } })
                } else {
                    json!({ "result": { "result": { "value": "ok" } } })
                }
            }
            _ => json!({ "result": {} }),
        })
        .await;
        d.eval_json("1").await.unwrap();
        assert_eq!(d.eval_json("1").await.unwrap(), json!("ok"));
        assert_eq!(methods(&rec).iter().filter(|m| *m == "Page.createIsolatedWorld").count(), 2);
    }

    #[tokio::test]
    async fn script_exceptions_surface_as_errors() {
        let (d, _) = attached_driver(|req| match req["method"].as_str().unwrap() {
            "Page.getFrameTree" => json!({ "result": { "frameTree": { "frame": { "id": "F" } } } }),
            "Page.createIsolatedWorld" => json!({ "result": { "executionContextId": 1 } }),
            _ => json!({ "result": { "exceptionDetails": { "text": "Uncaught", "exception": { "description": "ReferenceError: nope" } } } }),
        })
        .await;
        let e = d.eval_json("nope").await.unwrap_err();
        assert!(e.to_string().contains("ReferenceError"));
    }

    #[tokio::test]
    async fn input_events_are_sent_on_the_page_session() {
        let (d, rec) = attached_driver(|_| json!({ "result": {} })).await;
        d.click_point(10.0, 20.0).await.unwrap();
        d.wheel_scroll(5.0, 5.0, 300.0).await.unwrap();
        d.insert_text("hello").await.unwrap();
        let sent = rec.lock().unwrap().clone();
        let types: Vec<_> = sent.iter().filter_map(|r| r["params"]["type"].as_str()).collect();
        assert_eq!(types, vec!["mouseMoved", "mousePressed", "mouseReleased", "mouseWheel"]);
        assert!(sent.iter().all(|r| r["sessionId"] == "S1"));
        assert_eq!(sent.last().unwrap()["method"], "Input.insertText");
    }

    #[tokio::test]
    async fn screenshot_decodes_base64() {
        let (d, _) = attached_driver(|_| json!({ "result": { "data": "iVBORw0K" } })).await;
        assert_eq!(d.screenshot_png().await.unwrap(), vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
    }

    #[tokio::test]
    async fn set_bounds_resolves_window_id_first() {
        let (d, rec) = attached_driver(|req| match req["method"].as_str().unwrap() {
            "Browser.getWindowForTarget" => json!({ "result": { "windowId": 3 } }),
            _ => json!({ "result": {} }),
        })
        .await;
        d.set_bounds(WindowBounds { left: 600, top: 40, width: 500, height: 800 }).await.unwrap();
        let sent = rec.lock().unwrap().clone();
        assert_eq!(sent[1]["method"], "Browser.setWindowBounds");
        assert_eq!(sent[1]["params"]["windowId"], 3);
        assert_eq!(sent[1]["params"]["bounds"]["width"], 500);
        assert!(sent[1].get("sessionId").is_none(), "browser-level call has no page session");
    }

    #[tokio::test]
    async fn close_gracefully_sends_browser_close_and_clears_state() {
        let (mut d, rec) = attached_driver(|_| json!({ "result": {} })).await;
        d.close_gracefully().await.unwrap();
        assert_eq!(methods(&rec), vec!["Browser.close"]);
        assert!(!d.is_running());
        assert!(matches!(d.eval_json("1").await, Err(BrowserError::Disconnected)));
        d.close_gracefully().await.unwrap();
    }

    #[tokio::test]
    async fn unlaunched_driver_reports_disconnected() {
        let d = ChromeCdpDriver::new();
        assert!(!d.is_running());
        assert!(matches!(d.navigate("https://x.com").await, Err(BrowserError::Disconnected)));
    }
}
