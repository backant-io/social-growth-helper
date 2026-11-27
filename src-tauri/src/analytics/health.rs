//! Quiet status of the background analytics worker.
//!
//! Failures are retried later and never shown. The only thing the UI can learn from here is the time of the last
//! update and, when X itself needs the user (logged out, challenge, locked, suspended), why syncing is waiting.

use crate::error::AppResult;
use crate::storage::repo;
use crate::util::now_rfc3339;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub const SYNC_STATUS_KEY: &str = "analytics_sync_status";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncStatus {
    pub last_update_at: Option<String>,
    /// `logged_out`, `challenge`, `locked` or `suspended` while X needs the user; otherwise `None`.
    pub blocked: Option<String>,
}

pub fn load_status(conn: &Connection) -> SyncStatus {
    repo::get_setting(conn, SYNC_STATUS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_status(conn: &Connection, status: &SyncStatus) -> AppResult<()> {
    repo::set_setting(conn, SYNC_STATUS_KEY, &serde_json::to_string(status).unwrap_or_default())
}

/// Something was stored from X: stamp the time and clear any block.
pub fn mark_updated(conn: &Connection) -> AppResult<()> {
    save_status(conn, &SyncStatus { last_update_at: Some(now_rfc3339()), blocked: None })
}

pub fn set_blocked(conn: &Connection, reason: &str) -> AppResult<()> {
    let mut status = load_status(conn);
    status.blocked = Some(reason.to_string());
    save_status(conn, &status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open_in_memory;

    #[test]
    fn status_defaults_to_nothing_and_clears_a_block_on_the_next_update() {
        let conn = open_in_memory().unwrap();
        assert_eq!(load_status(&conn), SyncStatus::default());

        set_blocked(&conn, "logged_out").unwrap();
        let s = load_status(&conn);
        assert_eq!(s.blocked.as_deref(), Some("logged_out"));
        assert!(s.last_update_at.is_none());

        mark_updated(&conn).unwrap();
        let s = load_status(&conn);
        assert!(s.blocked.is_none());
        assert!(s.last_update_at.is_some());
    }
}
