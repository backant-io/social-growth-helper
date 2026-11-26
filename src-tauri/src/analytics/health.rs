//! Ingest health checks and selector/schema drift detection (ANL-12).

use crate::error::AppResult;
use crate::storage::repo;
use crate::util::now_rfc3339;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub const LAST_SYNC_HEALTH_KEY: &str = "analytics_last_sync_health";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthSeverity {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthIssue {
    pub code: String,
    pub severity: HealthSeverity,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncHealthReport {
    pub checked_at: String,
    pub status: HealthSeverity,
    pub extraction_mode: String,
    pub tier_mode: String,
    pub account_synced: bool,
    pub posts_synced: usize,
    pub external_posts_matched: usize,
    pub backfill_posts_added: usize,
    pub issues: Vec<HealthIssue>,
}

impl SyncHealthReport {
    pub fn recompute_status(&mut self) {
        self.status = if self.issues.iter().any(|i| i.severity == HealthSeverity::Error) {
            HealthSeverity::Error
        } else if self.issues.iter().any(|i| i.severity == HealthSeverity::Warning) {
            HealthSeverity::Warning
        } else {
            HealthSeverity::Ok
        };
    }
}

/// Validates a newly captured post metric snapshot against sanity bounds and any prior snapshot (ANL-12).
pub fn check_post_metrics_drift(
    platform_post_id: &str,
    impressions: Option<i64>,
    engagement_rate: Option<f64>,
    prior_impressions: Option<i64>,
    issues: &mut Vec<HealthIssue>,
) {
    if let Some(imp) = impressions {
        if imp < 0 {
            issues.push(HealthIssue {
                code: "negative_impressions".into(),
                severity: HealthSeverity::Error,
                message: format!("Post {platform_post_id} reported negative impressions ({imp})."),
                hint: "X analytics DOM or GraphQL field mapping may have drifted; inspect raw snapshot JSON.".into(),
            });
        }
        if let Some(prev) = prior_impressions {
            if prev >= 100 && imp < (prev * 3 / 4) {
                issues.push(HealthIssue {
                    code: "impressions_drop".into(),
                    severity: HealthSeverity::Warning,
                    message: format!(
                        "Post {platform_post_id} impressions dropped from {prev} to {imp} (>25% drop)."
                    ),
                    hint: "Verify whether X changed impression rounding or the page had not finished loading.".into(),
                });
            }
        }
    }
    if let Some(rate) = engagement_rate {
        if !(0.0..=100.0).contains(&rate) {
            issues.push(HealthIssue {
                code: "engagement_out_of_range".into(),
                severity: HealthSeverity::Warning,
                message: format!("Post {platform_post_id} had out-of-range engagement rate ({rate:.2}%)."),
                hint: "Check percentage parsing for X locale formatting.".into(),
            });
        }
    }
}

pub fn save_last_health(conn: &Connection, report: &SyncHealthReport) -> AppResult<()> {
    let raw = serde_json::to_string(report).unwrap_or_default();
    repo::set_setting(conn, LAST_SYNC_HEALTH_KEY, &raw)
}

pub fn load_last_health(conn: &Connection) -> AppResult<Option<SyncHealthReport>> {
    match repo::get_setting(conn, LAST_SYNC_HEALTH_KEY)? {
        Some(raw) => Ok(serde_json::from_str(&raw).ok()),
        None => Ok(None),
    }
}

pub fn empty_ok_report(extraction_mode: &str, tier_mode: &str) -> SyncHealthReport {
    SyncHealthReport {
        checked_at: now_rfc3339(),
        status: HealthSeverity::Ok,
        extraction_mode: extraction_mode.to_string(),
        tier_mode: tier_mode.to_string(),
        account_synced: false,
        posts_synced: 0,
        external_posts_matched: 0,
        backfill_posts_added: 0,
        issues: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_negative_impressions_steep_drop_and_invalid_engagement() {
        let mut issues = Vec::new();
        check_post_metrics_drift("p1", Some(-5), Some(150.0), None, &mut issues);
        assert!(issues.iter().any(|i| i.code == "negative_impressions" && i.severity == HealthSeverity::Error));
        assert!(issues.iter().any(|i| i.code == "engagement_out_of_range"));

        let mut drop_issues = Vec::new();
        check_post_metrics_drift("p2", Some(500), Some(3.2), Some(1000), &mut drop_issues);
        assert!(drop_issues.iter().any(|i| i.code == "impressions_drop"));
    }
}
