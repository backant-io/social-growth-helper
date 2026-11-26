//! Pause-all kill switch (SHL-20) and the event sink used by background work.

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Global stop. Scout, the publish queue and every executor step poll it; engaging it never waits for
/// a model call or a page load, so work ends at the next step boundary (well inside two seconds).
#[derive(Default)]
pub struct KillSwitch {
    engaged: AtomicBool,
}

impl KillSwitch {
    pub fn new(engaged: bool) -> Self {
        Self { engaged: AtomicBool::new(engaged) }
    }

    pub fn engage(&self) {
        self.engaged.store(true, Ordering::SeqCst);
    }

    pub fn release(&self) {
        self.engaged.store(false, Ordering::SeqCst);
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::SeqCst)
    }

    /// Sleeps for `total`, waking every 50 ms. Returns `false` if the switch (or `extra_stop`) fired.
    pub async fn sleep(&self, total: Duration, extra_stop: &AtomicBool) -> bool {
        let deadline = tokio::time::Instant::now() + total;
        loop {
            if self.is_engaged() || extra_stop.load(Ordering::SeqCst) {
                return false;
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return true;
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(50))).await;
        }
    }
}

/// Where background jobs report. The Tauri app forwards to the webview and OS notifications; tests collect.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
}

pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: &str, _payload: Value) {}
}

#[derive(Default)]
pub struct CollectSink(pub std::sync::Mutex<Vec<(String, Value)>>);

impl EventSink for CollectSink {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_string(), payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[tokio::test]
    async fn sleep_ends_promptly_when_engaged() {
        let k = Arc::new(KillSwitch::default());
        let stop = AtomicBool::new(false);
        let k2 = k.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            k2.engage();
        });
        let t = Instant::now();
        assert!(!k.sleep(Duration::from_secs(30), &stop).await);
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn sleep_completes_when_nothing_fires() {
        let k = KillSwitch::default();
        assert!(k.sleep(Duration::from_millis(60), &AtomicBool::new(false)).await);
    }
}
