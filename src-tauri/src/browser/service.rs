//! Application-facing wrapper over a [`BrowserDriver`]: serialises access, makes launch idempotent,
//! and guarantees the X window is closed when the app quits (BRW-16).

use super::{AnalyticsPageCapture, AnalyticsPageRequest, BrowserDriver, BrowserError, ChromeLaunchConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, MutexGuard};

/// Exclusive hold on the driver (BRW-10). Publish keeps one for a whole job, Scout takes one per tick,
/// so the two never drive the page at the same time and queued work runs in arrival order.
pub struct BrowserGuard<'a>(MutexGuard<'a, Box<dyn BrowserDriver>>);

impl std::ops::Deref for BrowserGuard<'_> {
    type Target = dyn BrowserDriver;
    fn deref(&self) -> &Self::Target {
        &**self.0
    }
}

pub const X_HOME_URL: &str = "https://x.com/home";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleTweet {
    pub id: Option<String>,
    pub handle: Option<String>,
    pub author: String,
    pub text: String,
    pub promoted: bool,
    pub time: Option<String>,
}

/// Read-only extraction of tweet cards currently rendered in the feed. Selectors come from the
/// Phase 0 catalogue (SPK-06); the final versioned registry lands with BRW-12.
pub const READ_VISIBLE_TWEETS_JS: &str = r#"(() => {
  const out = [];
  for (const a of document.querySelectorAll('article[data-testid="tweet"]')) {
    const link = a.querySelector('a[href*="/status/"] time')?.closest('a');
    const m = (link?.getAttribute('href') || '').match(/^\/([^/]+)\/status\/(\d+)/);
    out.push({
      id: m ? m[2] : null,
      handle: m ? m[1] : null,
      author: (a.querySelector('[data-testid="User-Name"]')?.innerText || '').split('\n')[0],
      text: a.querySelector('[data-testid="tweetText"]')?.innerText || '',
      promoted: !!a.querySelector('[data-testid="placementTracking"]'),
      time: a.querySelector('time')?.getAttribute('datetime') || null
    });
  }
  return out;
})()"#;

pub struct BrowserService {
    driver: Mutex<Box<dyn BrowserDriver>>,
}

impl BrowserService {
    pub fn new(driver: Box<dyn BrowserDriver>) -> Self {
        Self { driver: Mutex::new(driver) }
    }

    pub async fn lock(&self) -> BrowserGuard<'_> {
        BrowserGuard(self.driver.lock().await)
    }

    pub async fn bring_to_front(&self) -> Result<(), BrowserError> {
        self.driver.lock().await.bring_to_front().await
    }

    pub async fn is_running(&self) -> bool {
        self.driver.lock().await.is_running()
    }

    /// Opens the X window; returns `false` (and does nothing) if it is already open.
    pub async fn launch(&self, cfg: ChromeLaunchConfig) -> Result<bool, BrowserError> {
        let mut d = self.driver.lock().await;
        if d.is_running() {
            return Ok(false);
        }
        d.launch(cfg).await?;
        Ok(true)
    }

    pub async fn close(&self) -> Result<(), BrowserError> {
        self.driver.lock().await.close_gracefully().await
    }

    /// Called on quit. Never errors: a failed close must not keep the app from exiting.
    pub async fn shutdown(&self) {
        if let Err(e) = self.close().await {
            log::warn!("closing X browser on quit: {e}");
        }
    }

    pub async fn current_url(&self) -> Result<String, BrowserError> {
        self.driver.lock().await.current_url().await
    }

    pub async fn read_visible_tweets(&self) -> Result<Vec<VisibleTweet>, BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        let v = d.eval_json(READ_VISIBLE_TWEETS_JS).await?;
        serde_json::from_value(v).map_err(|e| BrowserError::Evaluation(format!("unexpected tweet payload: {e}")))
    }

    pub async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        d.navigate(url).await
    }

    pub async fn eval_json(&self, expression: &str) -> Result<serde_json::Value, BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        d.eval_json(expression).await
    }

    pub async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        d.insert_text(text).await
    }

    pub async fn click_point(&self, x: f64, y: f64) -> Result<(), BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        d.click_point(x, y).await
    }

    /// Captures an X analytics page in a dedicated tab with temporary passive network sniffing (BRW-13, ANL-04).
    pub async fn capture_analytics_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError> {
        let d = self.driver.lock().await;
        if !d.is_running() {
            return Err(BrowserError::Disconnected);
        }
        d.capture_analytics_page(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::mock::MockDriver;
    use super::*;
    use serde_json::json;

    fn cfg() -> ChromeLaunchConfig {
        ChromeLaunchConfig {
            binary_path: "/x".into(),
            profile_dir: "/p".into(),
            start_url: X_HOME_URL.into(),
            width: 1,
            height: 1,
        }
    }

    #[tokio::test]
    async fn launch_is_idempotent_and_shutdown_closes() {
        let mock = MockDriver::new();
        let svc = BrowserService::new(Box::new(mock.clone()));
        assert!(svc.launch(cfg()).await.unwrap());
        assert!(!svc.launch(cfg()).await.unwrap(), "second launch is a no-op");
        assert_eq!(mock.calls().iter().filter(|c| c.starts_with("launch")).count(), 1);
        svc.shutdown().await;
        assert!(!svc.is_running().await);
        assert_eq!(mock.calls().last().unwrap(), "close");
    }

    #[tokio::test]
    async fn reads_visible_tweets_through_the_trait_only() {
        let mock = MockDriver::new().with_eval_result(json!([
            { "id": "1", "handle": "naval", "author": "Naval", "text": "hi", "promoted": false, "time": "2026-11-26T10:00:00Z" },
            { "id": null, "handle": null, "author": "Ad", "text": "buy", "promoted": true, "time": null }
        ]));
        let svc = BrowserService::new(Box::new(mock.clone()));
        assert!(matches!(svc.read_visible_tweets().await, Err(BrowserError::Disconnected)));
        svc.launch(cfg()).await.unwrap();
        let tweets = svc.read_visible_tweets().await.unwrap();
        assert_eq!(tweets.len(), 2);
        assert_eq!(tweets[0].handle.as_deref(), Some("naval"));
        assert!(tweets[1].promoted);
        assert!(mock.calls().iter().any(|c| c.contains("article[data-testid=\"tweet\"]")));
    }

    #[tokio::test]
    async fn failed_launch_leaves_service_closed() {
        let mut mock = MockDriver::new();
        mock.fail_launch = true;
        let svc = BrowserService::new(Box::new(mock));
        assert!(svc.launch(cfg()).await.is_err());
        assert!(!svc.is_running().await);
        svc.shutdown().await;
    }

    #[test]
    fn script_is_read_only() {
        for banned in ["click(", ".submit", "dispatchEvent", "fetch(", "XMLHttpRequest", "innerHTML ="] {
            assert!(!READ_VISIBLE_TWEETS_JS.contains(banned), "extraction script must not contain {banned}");
        }
    }
}
