//! First-run onboarding state: tour completion, rules-of-engagement acknowledgement gate (SEC-10),
//! and the preference answers that seed the scout rubric (SHL-26).

use crate::error::{AppError, AppResult};
use crate::storage::repo;
use rusqlite::Connection;
use serde::Serialize;

pub const RULES_KIND: &str = "rules_of_engagement";
/// Bump when `rules_of_engagement.md` changes meaning; users are re-prompted only on a bump.
pub const NOTICE_VERSION: u32 = 1;
pub const NOTICE_TEXT: &str = include_str!("rules_of_engagement.md");
const TOUR_COMPLETED_KEY: &str = "tour_completed_notice_version";

#[derive(Debug, Clone, Serialize)]
pub struct RulesNotice {
    pub version: u32,
    pub text: &'static str,
}

pub fn notice() -> RulesNotice {
    RulesNotice { version: NOTICE_VERSION, text: NOTICE_TEXT }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulesStatus {
    pub current_version: u32,
    pub acknowledged_version: Option<u32>,
    pub acknowledged_at: Option<String>,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnboardingStatus {
    pub rules: RulesStatus,
    pub tour_completed: bool,
    pub preferences_saved: bool,
}

pub fn rules_status_for(conn: &Connection, current_version: u32) -> AppResult<RulesStatus> {
    let ack = repo::latest_acknowledgement(conn, RULES_KIND)?;
    Ok(RulesStatus {
        current_version,
        acknowledged: ack.as_ref().map(|a| a.notice_version >= current_version).unwrap_or(false),
        acknowledged_version: ack.as_ref().map(|a| a.notice_version),
        acknowledged_at: ack.map(|a| a.acknowledged_at),
    })
}

pub fn rules_status(conn: &Connection) -> AppResult<RulesStatus> {
    rules_status_for(conn, NOTICE_VERSION)
}

pub fn acknowledge_for(conn: &Connection, current_version: u32, version: u32, confirmed: bool) -> AppResult<RulesStatus> {
    if !confirmed {
        return Err(AppError::Validation("tick the checkbox to confirm you have read the rules".into()));
    }
    if version != current_version {
        return Err(AppError::Validation(
            "the rules notice changed since it was shown; read the current version and confirm again".into(),
        ));
    }
    repo::record_acknowledgement(conn, RULES_KIND, version)?;
    rules_status_for(conn, current_version)
}

pub fn acknowledge(conn: &Connection, version: u32, confirmed: bool) -> AppResult<RulesStatus> {
    acknowledge_for(conn, NOTICE_VERSION, version, confirmed)
}

/// Gate every write action (Scout, Publish, action bar) behind the acknowledgement.
pub fn require_acknowledged(conn: &Connection) -> AppResult<()> {
    if rules_status(conn)?.acknowledged {
        Ok(())
    } else {
        Err(AppError::RulesNotAcknowledged)
    }
}

pub fn mark_tour_completed(conn: &Connection) -> AppResult<()> {
    repo::set_setting(conn, TOUR_COMPLETED_KEY, &NOTICE_VERSION.to_string())
}

pub fn status(conn: &Connection, profile_id: &str) -> AppResult<OnboardingStatus> {
    Ok(OnboardingStatus {
        rules: rules_status(conn)?,
        tour_completed: repo::get_setting(conn, TOUR_COMPLETED_KEY)?.is_some(),
        preferences_saved: repo::get_preferences(conn, profile_id)?.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open;
    use tempfile::tempdir;

    fn conn() -> (tempfile::TempDir, Connection) {
        let d = tempdir().unwrap();
        let (c, _) = open(&d.path().join("a.db"), &d.path().join("b")).unwrap();
        (d, c)
    }

    #[test]
    fn notice_text_carries_the_required_message() {
        let t = NOTICE_TEXT.to_lowercase();
        assert!(t.contains("spam"));
        assert!(t.contains("ai slop"));
        assert!(t.contains("guideline"));
        assert!(t.contains("tool"));
    }

    #[test]
    fn gate_blocks_until_acknowledged() {
        let (_d, c) = conn();
        assert!(matches!(require_acknowledged(&c), Err(AppError::RulesNotAcknowledged)));
        assert!(acknowledge(&c, NOTICE_VERSION, false).is_err(), "checkbox is mandatory");
        assert!(!rules_status(&c).unwrap().acknowledged);
        let s = acknowledge(&c, NOTICE_VERSION, true).unwrap();
        assert!(s.acknowledged);
        assert!(s.acknowledged_at.is_some());
        require_acknowledged(&c).unwrap();
    }

    #[test]
    fn stale_version_cannot_be_acknowledged() {
        let (_d, c) = conn();
        assert!(acknowledge_for(&c, 2, 1, true).is_err());
    }

    #[test]
    fn reprompts_only_when_notice_version_changes() {
        let (_d, c) = conn();
        acknowledge_for(&c, 1, 1, true).unwrap();
        assert!(rules_status_for(&c, 1).unwrap().acknowledged);
        let bumped = rules_status_for(&c, 2).unwrap();
        assert!(!bumped.acknowledged, "a new notice version must be re-acknowledged");
        assert_eq!(bumped.acknowledged_version, Some(1));
        acknowledge_for(&c, 2, 2, true).unwrap();
        assert!(rules_status_for(&c, 2).unwrap().acknowledged);
    }

    #[test]
    fn status_reflects_tour_and_preferences() {
        let (_d, c) = conn();
        let pid = repo::ensure_default_profile(&c, "/p").unwrap();
        let s = status(&c, &pid).unwrap();
        assert!(!s.tour_completed && !s.preferences_saved);
        mark_tour_completed(&c).unwrap();
        repo::save_preferences(&c, &pid, repo::Preferences::default()).unwrap();
        let s = status(&c, &pid).unwrap();
        assert!(s.tour_completed && s.preferences_saved);
    }
}
