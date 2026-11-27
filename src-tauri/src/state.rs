use crate::browser::chrome_driver::ChromeCdpDriver;
use crate::browser::health::BrowserHealth;
use crate::browser::service::BrowserService;
use crate::formats::llm::StructuredLlm;
use crate::formats::router_llm::RouterLlm;
use crate::lifecycle::JobRegistry;
use crate::llm::router::LlmRouter;
use crate::secrets::SecretsService;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct AppPaths {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub backup_dir: PathBuf,
    pub browser_profile_dir: PathBuf,
    pub publish_browser_profile_dir: PathBuf,
    pub log_dir: PathBuf,
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
        let profiles = data_dir.join("browser-profiles");
        Self {
            db_path: data_dir.join("app.db"),
            backup_dir: data_dir.join("backups"),
            log_dir: data_dir.join("logs"),
            browser_profile_dir: profiles.join("x-default"),
            publish_browser_profile_dir: profiles.join("x-publish"),
            data_dir,
        }
    }
}

pub struct AppState {
    pub paths: AppPaths,
    pub db: Arc<Mutex<Connection>>,
    pub profile_id: String,
    pub secrets: Arc<SecretsService>,
    /// Visible X browser used for login and Scout feed browsing/scrolling.
    pub browser: Arc<BrowserService>,
    /// Separate invisible Chrome instance (`--headless=new`) used for publishing replies/actions,
    /// kept synced with cookies/profile state from `browser`.
    pub publish_browser: Arc<BrowserService>,
    pub browser_health: BrowserHealth,
    pub llm_router: Arc<LlmRouter>,
    pub http: reqwest::Client,
    pub jobs: JobRegistry,
    /// Phase 5 model seam, backed by the Phase 2 router (`LlmRouter::complete_json_for_task`).
    pub llm: Arc<dyn StructuredLlm>,
    pub quit_approved: AtomicBool,
    /// Model ids fetched live from each provider API (account id to ids). Merged into the catalogue.
    pub remote_models: Mutex<std::collections::HashMap<String, Vec<String>>>,
}

impl AppState {
    pub fn new(paths: AppPaths, db: Connection, profile_id: String) -> Self {
        let db = Arc::new(Mutex::new(db));
        let secrets = Arc::new(SecretsService::system());
        let browser = Arc::new(BrowserService::new(Box::new(ChromeCdpDriver::new())));
        let publish_browser = Arc::new(BrowserService::new(Box::new(ChromeCdpDriver::new())));
        let llm_router = Arc::new(LlmRouter::new());
        {
            let conn = db.lock().unwrap();
            let _ = llm_router.sync_from_db(&conn, secrets.clone(), browser.clone());
        }
        let llm: Arc<dyn StructuredLlm> = Arc::new(RouterLlm::new(llm_router.clone(), db.clone()));
        Self {
            paths,
            db,
            profile_id,
            secrets,
            browser,
            publish_browser,
            browser_health: BrowserHealth::new(),
            llm_router,
            http: reqwest::Client::new(),
            jobs: JobRegistry::new(),
            llm,
            quit_approved: AtomicBool::new(false),
            remote_models: Mutex::new(std::collections::HashMap::new()),
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
        assert_eq!(p.log_dir, PathBuf::from("/d/social-growth-helper/logs"));
        assert_eq!(p.browser_profile_dir, PathBuf::from("/d/social-growth-helper/browser-profiles/x-default"));
        assert_eq!(p.publish_browser_profile_dir, PathBuf::from("/d/social-growth-helper/browser-profiles/x-publish"));
    }
}
