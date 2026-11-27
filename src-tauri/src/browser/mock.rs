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
    eval_fail: Option<Arc<dyn Fn(&str) -> bool + Send + Sync>>,
    wheel_failures: Arc<Mutex<u32>>,
    screenshot: Arc<Mutex<Vec<u8>>>,
    pub bounds: Arc<Mutex<WindowBounds>>,
    pub cookies: Arc<Mutex<Vec<Value>>>,
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
            eval_fail: None,
            wheel_failures: Arc::default(),
            screenshot: Arc::new(Mutex::new(vec![0x89, 0x50])),
            bounds: Arc::new(Mutex::new(WindowBounds { left: 0, top: 0, width: 560, height: 900 })),
            cookies: Arc::default(),
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

    /// Makes `eval_json` fail with an evaluation error for every script the predicate accepts.
    pub fn with_eval_failure(mut self, f: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.eval_fail = Some(Arc::new(f));
        self
    }

    /// The next `n` wheel scrolls fail.
    pub fn fail_next_wheels(&self, n: u32) {
        *self.wheel_failures.lock().unwrap() = n;
    }

    /// Simulates Chrome disappearing: the driver stops reporting itself as running.
    pub fn crash(&self) {
        *self.running.lock().unwrap() = false;
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
        if self.eval_fail.as_ref().is_some_and(|f| f(expression)) {
            return Err(BrowserError::Evaluation("mock evaluation failure".into()));
        }
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
        let mut left = self.wheel_failures.lock().unwrap();
        if *left > 0 {
            *left -= 1;
            return Err(BrowserError::Evaluation("mock wheel failure".into()));
        }
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
    async fn export_cookies(&self) -> Result<Vec<Value>, BrowserError> {
        self.record("export_cookies");
        Ok(self.cookies.lock().unwrap().clone())
    }
    async fn import_cookies(&self, cookies: &[Value]) -> Result<(), BrowserError> {
        self.record(format!("import_cookies {}", cookies.len()));
        *self.cookies.lock().unwrap() = cookies.to_vec();
        Ok(())
    }
    async fn get_bounds(&self) -> Result<WindowBounds, BrowserError> {
        Ok(self.bounds.lock().unwrap().clone())
    }
    async fn set_file_input_files(&self, selector: &str, paths: &[std::path::PathBuf]) -> Result<(), BrowserError> {
        self.record(format!("files {selector} {}", paths.len()));
        Ok(())
    }
    async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
        self.record("close");
        *self.running.lock().unwrap() = false;
        Ok(())
    }
}
