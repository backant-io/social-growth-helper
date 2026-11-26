use super::BrowserError;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Minimal loopback-only HTTP GET used for Chrome's `/json/version`; avoids pulling in an HTTP client.
pub async fn get_json(port: u16, path: &str, timeout: Duration) -> Result<serde_json::Value, BrowserError> {
    let fut = async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n\r\n");
        stream.write_all(req.as_bytes()).await?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 || response_complete(&buf_with(&mut buf, &chunk[..n])) {
                break;
            }
        }
        Ok::<_, std::io::Error>(buf)
    };
    let raw = tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| BrowserError::Timeout(format!("GET 127.0.0.1:{port}{path}")))??;
    let text = String::from_utf8_lossy(&raw);
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    let (start, end) = (body.find(['{', '[']), body.rfind(['}', ']']));
    match (start, end) {
        (Some(s), Some(e)) if e >= s => {
            serde_json::from_str(&body[s..=e]).map_err(|e| BrowserError::Other(format!("bad JSON from Chrome: {e}")))
        }
        _ => Err(BrowserError::Other("empty response from Chrome debugging endpoint".into())),
    }
}

fn buf_with<'a>(buf: &'a mut Vec<u8>, more: &[u8]) -> &'a [u8] {
    buf.extend_from_slice(more);
    buf
}

/// Chrome keeps the socket open after answering, so completion is judged by Content-Length rather than EOF.
fn response_complete(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    head.lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().ok()))
        .flatten()
        .is_some_and(|len| body.len() >= len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn parses_json_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b).await.unwrap();
            let body = r#"{"Browser":"Chrome/157","webSocketDebuggerUrl":"ws://127.0.0.1:1/x"}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            s.write_all(resp.as_bytes()).await.unwrap();
        });
        let v = get_json(port, "/json/version", Duration::from_secs(2)).await.unwrap();
        assert_eq!(v["Browser"], "Chrome/157");
    }

    #[tokio::test]
    async fn returns_without_waiting_for_eof_when_content_length_is_satisfied() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b).await.unwrap();
            let body = r#"{"ok":true}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            s.write_all(resp.as_bytes()).await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let started = std::time::Instant::now();
        let v = get_json(port, "/x", Duration::from_secs(3)).await.unwrap();
        assert_eq!(v["ok"], true);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn closed_port_errors() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(get_json(port, "/json/version", Duration::from_millis(500)).await.is_err());
    }
}
