use super::{BrowserDriver, BrowserError, ChromeLaunchConfig, WindowBounds};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Scriptable driver for tests of anything that depends only on the [`BrowserDriver`] trait.
#[derive(Clone)]
pub struct MockDriver {
    pub calls: Arc<Mutex<Vec<String>>>,
    pub eval_result: Arc<Mutex<Value>>,
    running: Arc<Mutex<bool>>,
    pub fail_launch: bool,
}

impl MockDriver {
    pub fn new() -> Self {
        Self {
            calls: Arc::default(),
            eval_result: Arc::new(Mutex::new(Value::Null)),
            running: Arc::default(),
            fail_launch: false,
        }
    }

    pub fn with_eval_result(self, v: Value) -> Self {
        *self.eval_result.lock().unwrap() = v;
        self
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, s: impl Into<String>) {
        self.calls.lock().unwrap().push(s.into());
    }
}

impl Default for MockDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BrowserDriver for MockDriver {
    async fn launch(&mut self, cfg: ChromeLaunchConfig) -> Result<(), BrowserError> {
        self.record(format!("launch {}", cfg.start_url));
        if self.fail_launch {
            return Err(BrowserError::ChromeExitedEarly("mock".into()));
        }
        *self.running.lock().unwrap() = true;
        Ok(())
    }
    fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }
    async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        self.record(format!("navigate {url}"));
        Ok(())
    }
    async fn current_url(&self) -> Result<String, BrowserError> {
        Ok("https://x.com/home".into())
    }
    async fn eval_json(&self, expression: &str) -> Result<Value, BrowserError> {
        self.record(format!("eval {expression}"));
        Ok(self.eval_result.lock().unwrap().clone())
    }
    async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError> {
        self.record("screenshot");
        Ok(vec![0x89, 0x50])
    }
    async fn ax_tree(&self, _max_depth: Option<u32>) -> Result<Value, BrowserError> {
        self.record("ax_tree");
        Ok(Value::Null)
    }
    async fn click_point(&self, x: f64, y: f64) -> Result<(), BrowserError> {
        self.record(format!("click {x},{y}"));
        Ok(())
    }
    async fn wheel_scroll(&self, _x: f64, _y: f64, delta_y: f64) -> Result<(), BrowserError> {
        self.record(format!("wheel {delta_y}"));
        Ok(())
    }
    async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
        self.record(format!("insert {text}"));
        Ok(())
    }
    async fn set_bounds(&self, b: WindowBounds) -> Result<(), BrowserError> {
        self.record(format!("bounds {}x{}", b.width, b.height));
        Ok(())
    }
    async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
        self.record("close");
        *self.running.lock().unwrap() = false;
        Ok(())
    }
}
