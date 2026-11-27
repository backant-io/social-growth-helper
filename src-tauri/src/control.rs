//! Pause-all kill switch (SHL-20) and the event sink used by background work.

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;

/// Per-session stop signal supporting both non-blocking polls and immediate async cancellation.
#[derive(Default)]
pub struct StopSignal {
    stopped: AtomicBool,
    notify: Notify,
}

impl StopSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Resolves immediately if already stopped, or as soon as `stop()` is called.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_stopped() {
                return;
            }
            notified.await;
        }
    }
}

/// Global stop. Scout, the publish queue and every executor step poll or await it; engaging it
/// wakes waiters immediately so work cancels without waiting for a model call or page load.
#[derive(Default)]
pub struct KillSwitch {
    engaged: AtomicBool,
    notify: Notify,
}

impl KillSwitch {
    pub fn new(engaged: bool) -> Self {
        Self { engaged: AtomicBool::new(engaged), notify: Notify::new() }
    }

    pub fn engage(&self) {
        self.engaged.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn release(&self) {
        self.engaged.store(false, Ordering::SeqCst);
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::SeqCst)
    }

    /// Resolves immediately if already engaged, or as soon as `engage()` is called.
    pub async fn engaged(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_engaged() {
                return;
            }
            notified.await;
        }
    }

    /// Sleeps for `total`, waking immediately if the kill switch engages and polling `extra_stop` every 50 ms.
    /// Returns `false` if the switch (or `extra_stop`) fired.
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
            let step = (deadline - now).min(Duration::from_millis(50));
            tokio::select! {
                biased;
                _ = self.engaged() => return false,
                _ = tokio::time::sleep(step) => {}
            }
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

    #[tokio::test]
    async fn stop_signal_wakes_waiters_immediately_and_when_already_stopped() {
        let s = Arc::new(StopSignal::new());
        let s2 = s.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            s2.stop();
        });
        let t = Instant::now();
        s.cancelled().await;
        assert!(s.is_stopped());
        assert!(t.elapsed() < Duration::from_secs(1));

        // Already-stopped signal resolves immediately.
        let t2 = Instant::now();
        s.cancelled().await;
        assert!(t2.elapsed() < Duration::from_millis(50));
    }
}
