//! Real-browser workspace: a genuine Chrome in `--app` mode driven over a minimal, hygienic CDP client.
//!
//! Everything outside this module talks only to the [`BrowserDriver`] trait, so a future embedded
//! driver can replace [`chrome_driver::ChromeCdpDriver`] without touching Scout, Publish or Analytics.

pub mod cdp;
pub mod chrome_driver;
pub mod discovery;
pub mod http;
pub mod launcher;
pub mod mock;
pub mod service;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ChromeLaunchConfig {
    pub binary_path: PathBuf,
    pub profile_dir: PathBuf,
    pub start_url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowBounds {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("forbidden CDP method attempted: {0}")]
    ForbiddenCdpMethod(String),
    #[error("no supported browser found. Install Google Chrome (Chromium or Brave also work) or set a browser path in Settings. {0}")]
    BinaryNotFound(String),
    #[error("cdp error {code}: {message}")]
    CdpProtocol { code: i64, message: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("browser not connected")]
    Disconnected,
    #[error("browser is already running")]
    AlreadyRunning,
    #[error("Chrome exited immediately ({0}). Another Chrome may already be using the app's profile; close it and try again")]
    ChromeExitedEarly(String),
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("no page target found in Chrome")]
    NoPageTarget,
    #[error("script error: {0}")]
    Evaluation(String),
    #[error("{0}")]
    Other(String),
}

/// Core abstraction (BRW-03) so every module calls only this trait and tests can use the mock driver.
#[async_trait::async_trait]
pub trait BrowserDriver: Send + Sync {
    async fn launch(&mut self, cfg: ChromeLaunchConfig) -> Result<(), BrowserError>;
    fn is_running(&self) -> bool;
    async fn navigate(&self, url: &str) -> Result<(), BrowserError>;
    async fn current_url(&self) -> Result<String, BrowserError>;
    async fn eval_json(&self, expression: &str) -> Result<serde_json::Value, BrowserError>;
    async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError>;
    async fn ax_tree(&self, max_depth: Option<u32>) -> Result<serde_json::Value, BrowserError>;
    async fn click_point(&self, x: f64, y: f64) -> Result<(), BrowserError>;
    async fn wheel_scroll(&self, x: f64, y: f64, delta_y: f64) -> Result<(), BrowserError>;
    async fn insert_text(&self, text: &str) -> Result<(), BrowserError>;
    async fn set_bounds(&self, bounds: WindowBounds) -> Result<(), BrowserError>;
    /// Graceful `Browser.close` so Chrome flushes cookies and storage before exiting (BRW-16).
    /// Raises the X window (tray "Focus X window", challenge takeover). No-op for drivers without windows.
    async fn bring_to_front(&self) -> Result<(), BrowserError> {
        Ok(())
    }
    async fn close_gracefully(&mut self) -> Result<(), BrowserError>;
}
