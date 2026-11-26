//! SHL-13: wipe everything this app stored on this machine and return to first-run onboarding.
//!
//! Order matters: keychain entries go first (and abort the wipe if they cannot be removed, since losing the
//! references would orphan them), then the database in one transaction, then every file in the data directory,
//! including backups and the browser profile that holds the X session.

use crate::error::{AppError, AppResult};
use crate::secrets::SecretsService;
use crate::state::AppPaths;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

pub const CONFIRMATION: &str = "DELETE";

#[derive(Debug, Clone, Serialize, Default)]
pub struct WipeReport {
    pub rows_deleted: i64,
    pub keychain_removed: usize,
    pub files_removed: usize,
    pub warnings: Vec<String>,
}

fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn is_db_file(path: &Path, db_path: &Path) -> bool {
    let (Some(name), Some(base)) = (path.file_name().and_then(|n| n.to_str()), db_path.file_name().and_then(|n| n.to_str())) else { return false };
    name == base || name == format!("{base}-wal") || name == format!("{base}-shm") || name == format!("{base}-journal")
}

fn safe_data_dir(dir: &Path) -> bool {
    dir.components().count() >= 3 && dirs::home_dir().map(|h| h != dir).unwrap_or(true)
}

/// `profile_id` stays: the running app is bound to it. Its handle is cleared so no account detail survives.
pub fn wipe(conn: &Connection, paths: &AppPaths, secrets: &SecretsService, confirmation: &str, ignore_keychain_errors: bool) -> AppResult<WipeReport> {
    if confirmation.trim() != CONFIRMATION {
        return Err(AppError::Validation(format!("type {CONFIRMATION} to confirm")));
    }
    if !safe_data_dir(&paths.data_dir) {
        return Err(AppError::Validation("refusing to wipe an unexpected data directory".into()));
    }
    let mut report = WipeReport::default();

    let refs: Vec<String> = {
        let has: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='provider_accounts'", [], |r| r.get(0))?;
        if has == 0 {
            vec![]
        } else {
            let mut stmt = conn.prepare("SELECT keychain_ref FROM provider_accounts WHERE keychain_ref IS NOT NULL")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        }
    };
    let mut failures = vec![];
    for r in &refs {
        match secrets.remove(r) {
            Ok(()) => report.keychain_removed += 1,
            Err(e) => failures.push(e.to_string()),
        }
    }
    if !failures.is_empty() {
        if !ignore_keychain_errors {
            return Err(AppError::Validation(format!(
                "could not remove {} saved API key(s) from the system keychain ({}). Your data was not deleted. Fix the keychain and retry, or wipe anyway and delete the entries yourself.",
                failures.len(),
                failures[0]
            )));
        }
        report.warnings.push(format!("{} keychain entr{} could not be removed; delete {} in your keychain app (service io.backant.social-growth-helper).", failures.len(), if failures.len() == 1 { "y" } else { "ies" }, if failures.len() == 1 { "it" } else { "them" }));
    }

    conn.execute_batch("PRAGMA foreign_keys=OFF")?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let db_result = (|| -> AppResult<i64> {
        let names = super::transfer::user_table_names(conn, "main")?;
        let mut deleted = 0i64;
        for t in names.iter().filter(|t| !matches!(t.as_str(), "profiles" | "task_model_routes")) {
            deleted += conn.execute(&format!("DELETE FROM {}", q(t)), [])? as i64;
        }
        if names.iter().any(|t| t == "task_model_routes") {
            conn.execute(
                "UPDATE task_model_routes SET provider_account_id=NULL, model_id=NULL, reasoning_effort=NULL, temperature=NULL,
                        fallback_account_id=NULL, fallback_model_id=NULL, max_calls_per_hour=NULL",
                [],
            )?;
        }
        conn.execute("UPDATE profiles SET handle='', display_name=NULL", [])?;
        let has_seq: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name='sqlite_sequence'", [], |r| r.get(0))?;
        if has_seq > 0 {
            conn.execute("DELETE FROM sqlite_sequence", [])?;
        }
        Ok(deleted)
    })();
    match db_result {
        Ok(n) => {
            report.rows_deleted = n;
            conn.execute_batch("COMMIT")?;
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            let _ = conn.execute_batch("PRAGMA foreign_keys=ON");
            return Err(e);
        }
    }
    conn.execute_batch("PRAGMA foreign_keys=ON")?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    if let Err(e) = conn.execute_batch("VACUUM") {
        report.warnings.push(format!("could not compact the database file: {e}"));
    }

    match std::fs::read_dir(&paths.data_dir) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let p = entry.path();
                if is_db_file(&p, &paths.db_path) {
                    continue;
                }
                let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                match r {
                    Ok(()) => report.files_removed += 1,
                    Err(e) => report.warnings.push(format!("could not delete {}: {e}", p.display())),
                }
            }
        }
        Err(e) => report.warnings.push(format!("could not read the data directory: {e}")),
    }
    if paths.browser_profile_dir.exists() && !paths.browser_profile_dir.starts_with(&paths.data_dir) {
        match std::fs::remove_dir_all(&paths.browser_profile_dir) {
            Ok(()) => report.files_removed += 1,
            Err(e) => report.warnings.push(format!("could not delete the browser profile: {e}")),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::patch::tests::seed_published_draft;
    use crate::learning::patch::{active_style, apply, Patch, PatchOp, PatchTarget};
    use crate::onboarding;
    use crate::secrets::{MemoryBackend, Platform};
    use crate::storage::{self, repo};
    use tempfile::tempdir;

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: AppPaths,
        conn: Connection,
        pid: String,
        secrets: SecretsService,
    }

    fn fixture(backend: MemoryBackend) -> Fixture {
        let dir = tempdir().unwrap();
        let data = dir.path().join("a").join("social-growth-helper");
        std::fs::create_dir_all(&data).unwrap();
        let paths = AppPaths::under(data);
        let (conn, _) = storage::open(&paths.db_path, &paths.backup_dir).unwrap();
        let pid = repo::ensure_default_profile(&conn, &paths.browser_profile_dir.display().to_string()).unwrap();
        let secrets = SecretsService::with_backend(Box::new(backend), Platform::Macos, true);
        secrets.store("provider:k1", "sk-secret").unwrap_or(());
        Fixture { _dir: dir, paths, conn, pid, secrets }
    }

    fn populate(f: &Fixture) {
        let c = &f.conn;
        repo::save_preferences(c, &f.pid, repo::Preferences { niche_topics: vec!["rust".into()], ..Default::default() }).unwrap();
        onboarding::acknowledge(c, onboarding::notice().version, true).unwrap();
        onboarding::mark_tour_completed(c).unwrap();
        seed_published_draft(c, &f.pid, "d1", "Shipping beats planning");
        apply(c, &f.pid, &Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "great point".into() }] }, None).unwrap();
        c.execute("INSERT INTO provider_accounts (id, provider_type, label, keychain_ref, billing_lane, status) VALUES ('a1','anthropic','Work','provider:k1','api','verified')", []).unwrap();
        c.execute("UPDATE task_model_routes SET provider_account_id='a1', model_id='claude' WHERE task_type='reply_drafting'", []).unwrap();
        c.execute("UPDATE profiles SET handle='pavel'", []).unwrap();
        std::fs::create_dir_all(&f.paths.browser_profile_dir).unwrap();
        std::fs::write(f.paths.browser_profile_dir.join("Cookies"), b"x-session-cookie").unwrap();
        std::fs::create_dir_all(&f.paths.backup_dir).unwrap();
        std::fs::write(f.paths.backup_dir.join("app-v1.db"), b"old copy").unwrap();
    }

    #[test]
    fn wipe_requires_the_typed_confirmation() {
        let f = fixture(MemoryBackend::new());
        populate(&f);
        for bad in ["", "delete", "yes", "DELETE ME"] {
            assert!(wipe(&f.conn, &f.paths, &f.secrets, bad, false).is_err());
        }
        assert_eq!(f.secrets.load("provider:k1").unwrap().as_deref(), Some("sk-secret"));
        assert!(f.paths.browser_profile_dir.exists());
    }

    #[test]
    fn wipe_removes_data_keychain_entries_profile_and_backups_and_returns_to_onboarding() {
        let f = fixture(MemoryBackend::new());
        populate(&f);
        assert!(onboarding::status(&f.conn, &f.pid).unwrap().tour_completed);

        let r = wipe(&f.conn, &f.paths, &f.secrets, " DELETE ", false).unwrap();
        assert!(r.rows_deleted > 0 && r.keychain_removed == 1 && r.files_removed >= 2, "{r:?}");
        assert!(f.secrets.load("provider:k1").unwrap().is_none());
        assert!(!f.paths.browser_profile_dir.exists() && !f.paths.backup_dir.exists());
        assert!(f.paths.db_path.exists(), "the database file stays, empty");

        let st = onboarding::status(&f.conn, &f.pid).unwrap();
        assert!(!st.tour_completed);
        assert!(repo::get_preferences(&f.conn, &f.pid).unwrap().is_none());
        assert!(repo::latest_acknowledgement(&f.conn, "rules_of_engagement").unwrap().is_none());
        for t in ["reply_drafts", "observed_posts", "provider_accounts", "style_guides", "style_examples", "learning_changelog", "learning_rubrics", "app_settings", "app_acknowledgements"] {
            let n: i64 = f.conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "{t}");
        }
        let (acc, model): (Option<String>, Option<String>) = f.conn.query_row("SELECT provider_account_id, model_id FROM task_model_routes WHERE task_type='reply_drafting'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((acc, model), (None, None));
        let routes: i64 = f.conn.query_row("SELECT count(*) FROM task_model_routes", [], |r| r.get(0)).unwrap();
        assert_eq!(routes, 7, "route slots are kept, unset");
        let handle: String = f.conn.query_row("SELECT handle FROM profiles WHERE id=?1", [&f.pid], |r| r.get(0)).unwrap();
        assert_eq!(handle, "");
        assert_eq!(active_style(&f.conn, &f.pid).unwrap().version, None, "style guides are gone");
        assert_eq!(repo::ensure_default_profile(&f.conn, "/x").unwrap(), f.pid, "the running app's profile id still works");
        assert!(repo::save_preferences(&f.conn, &f.pid, repo::Preferences::default()).is_ok(), "app is usable again");
        let fk_on: i64 = f.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk_on, 1);
    }

    #[test]
    fn keychain_failure_aborts_unless_explicitly_ignored() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("a").join("social-growth-helper");
        std::fs::create_dir_all(&data).unwrap();
        let paths = AppPaths::under(data);
        let (conn, _) = storage::open(&paths.db_path, &paths.backup_dir).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/p").unwrap();
        seed_published_draft(&conn, &pid, "d1", "hello there friend");
        conn.execute("INSERT INTO provider_accounts (id, provider_type, label, keychain_ref, billing_lane, status) VALUES ('a1','anthropic','Work','provider:k1','api','verified')", []).unwrap();
        let down = SecretsService::with_backend(Box::new(MemoryBackend::unavailable("no secret service")), Platform::Linux, false);

        let err = wipe(&conn, &paths, &down, CONFIRMATION, false).unwrap_err();
        assert!(err.to_string().contains("not deleted"), "{err}");
        let n: i64 = conn.query_row("SELECT count(*) FROM reply_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);

        let r = wipe(&conn, &paths, &down, CONFIRMATION, true).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("keychain")));
        let n: i64 = conn.query_row("SELECT count(*) FROM reply_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn wipe_without_provider_keys_does_not_need_the_keychain() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("a").join("social-growth-helper");
        std::fs::create_dir_all(&data).unwrap();
        let paths = AppPaths::under(data);
        let (conn, _) = storage::open(&paths.db_path, &paths.backup_dir).unwrap();
        repo::ensure_default_profile(&conn, "/p").unwrap();
        let down = SecretsService::with_backend(Box::new(MemoryBackend::unavailable("none")), Platform::Linux, false);
        assert!(wipe(&conn, &paths, &down, CONFIRMATION, false).is_ok());
    }
}
