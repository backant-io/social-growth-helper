use crate::browser::chrome_driver::ChromeCdpDriver;
use crate::browser::service::BrowserService;
use crate::lifecycle::JobRegistry;
use crate::secrets::SecretsService;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

pub struct AppPaths {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub backup_dir: PathBuf,
    pub browser_profile_dir: PathBuf,
}

impl AppPaths {
    /// `~/Library/Application Support/social-growth-helper` on macOS, `~/.local/share/social-growth-helper` on Linux.
    pub fn resolve() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("social-growth-helper");
        Self::under(data_dir)
    }

    pub fn under(data_dir: PathBuf) -> Self {
        Self {
            db_path: data_dir.join("app.db"),
            backup_dir: data_dir.join("backups"),
            browser_profile_dir: data_dir.join("browser-profiles").join("x-default"),
            data_dir,
        }
    }
}

pub struct AppState {
    pub paths: AppPaths,
    pub db: Mutex<Connection>,
    pub profile_id: String,
    pub secrets: SecretsService,
    pub browser: BrowserService,
    pub jobs: JobRegistry,
    pub quit_approved: AtomicBool,
}

impl AppState {
    pub fn new(paths: AppPaths, db: Connection, profile_id: String) -> Self {
        Self {
            paths,
            db: Mutex::new(db),
            profile_id,
            secrets: SecretsService::system(),
            browser: BrowserService::new(Box::new(ChromeCdpDriver::new())),
            jobs: JobRegistry::new(),
            quit_approved: AtomicBool::new(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_layout_matches_the_architecture_plan() {
        let p = AppPaths::under(PathBuf::from("/d/social-growth-helper"));
        assert_eq!(p.db_path, PathBuf::from("/d/social-growth-helper/app.db"));
        assert_eq!(p.browser_profile_dir, PathBuf::from("/d/social-growth-helper/browser-profiles/x-default"));
    }
}
