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
    eval_fn: Option<EvalFn>,
    screenshot: Arc<Mutex<Vec<u8>>>,
    pub bounds: Arc<Mutex<WindowBounds>>,
}

pub type EvalFn = Arc<dyn Fn(&str) -> Value + Send + Sync>;

impl MockDriver {
    pub fn new() -> Self {
        Self {
            calls: Arc::default(),
            eval_result: Arc::new(Mutex::new(Value::Null)),
            running: Arc::default(),
            fail_launch: false,
            eval_fn: None,
            screenshot: Arc::new(Mutex::new(vec![0x89, 0x50])),
            bounds: Arc::new(Mutex::new(WindowBounds { left: 0, top: 0, width: 560, height: 900 })),
        }
    }

    pub fn with_eval_result(self, v: Value) -> Self {
        *self.eval_result.lock().unwrap() = v;
        self
    }

    /// Computes each `eval_json` result from the script text, for flows that issue many different scripts.
    pub fn with_eval_fn(mut self, f: impl Fn(&str) -> Value + Send + Sync + 'static) -> Self {
        self.eval_fn = Some(Arc::new(f));
        self
    }

    pub fn with_screenshot(self, bytes: Vec<u8>) -> Self {
        *self.screenshot.lock().unwrap() = bytes;
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
        if let Some(f) = &self.eval_fn {
            return Ok(f(expression));
        }
        Ok(self.eval_result.lock().unwrap().clone())
    }
    async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError> {
        self.record("screenshot");
        Ok(self.screenshot.lock().unwrap().clone())
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
        self.record(format!("bounds {},{} {}x{}", b.left, b.top, b.width, b.height));
        *self.bounds.lock().unwrap() = b;
        Ok(())
    }
    async fn bring_to_front(&self) -> Result<(), BrowserError> {
        self.record("front");
        Ok(())
    }
    async fn get_bounds(&self) -> Result<WindowBounds, BrowserError> {
        Ok(self.bounds.lock().unwrap().clone())
    }
    async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
        self.record("close");
        *self.running.lock().unwrap() = false;
        Ok(())
    }
}
