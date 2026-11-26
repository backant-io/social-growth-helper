//! SQLite persistence for provider accounts, task model routes, and usage logs (LLM-08, LLM-12, LLM-17).

use super::{BillingLane, ProviderType, TaskType};
use crate::error::{AppError, AppResult};
use crate::util::{format_unix, new_id, now_epoch_secs, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderAccountConfig {
    pub custom_models: Vec<String>,
    pub endpoint_override: Option<String>,
    pub cli_binary_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub id: String,
    pub provider_type: ProviderType,
    pub label: String,
    pub keychain_ref: Option<String>,
    pub billing_lane: BillingLane,
    pub status: String,
    pub last_verified_at: Option<String>,
    pub last_error: Option<String>,
    pub config: ProviderAccountConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskModelRoute {
    pub task_type: TaskType,
    pub provider_account_id: Option<String>,
    pub model_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
    pub fallback_account_id: Option<String>,
    pub fallback_model_id: Option<String>,
    pub max_calls_per_hour: Option<u32>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskModelRoutePatch {
    pub task_type: TaskType,
    pub provider_account_id: Option<String>,
    pub model_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
    pub fallback_account_id: Option<String>,
    pub fallback_model_id: Option<String>,
    pub max_calls_per_hour: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    pub task_type: TaskType,
    pub provider_account_id: Option<String>,
    pub provider_type: ProviderType,
    pub model_id: String,
    pub prompt_version: Option<u32>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub used_fallback: bool,
    pub success: bool,
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummaryRow {
    pub task_type: TaskType,
    pub provider_type: ProviderType,
    pub model_id: String,
    pub total_calls: u64,
    pub successful_calls: u64,
    pub fallback_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDashboard {
    pub window_days: u32,
    pub total_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub rows: Vec<UsageSummaryRow>,
    pub hourly_counts: Vec<TaskHourlyCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHourlyCount {
    pub task_type: TaskType,
    pub calls_last_hour: u32,
    pub max_calls_per_hour: Option<u32>,
}

fn parse_provider_account(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderAccount> {
    let pt_str: String = r.get(1)?;
    let bl_str: String = r.get(4)?;
    let cfg_str: String = r.get(8)?;
    let provider_type = ProviderType::parse(&pt_str).unwrap_or(ProviderType::ClaudeCli);
    let billing_lane = if bl_str == "api" { BillingLane::Api } else { BillingLane::Subscription };
    let config = serde_json::from_str(&cfg_str).unwrap_or_default();
    Ok(ProviderAccount {
        id: r.get(0)?,
        provider_type,
        label: r.get(2)?,
        keychain_ref: r.get(3)?,
        billing_lane,
        status: r.get(5)?,
        last_verified_at: r.get(6)?,
        last_error: r.get(7)?,
        config,
    })
}

pub fn list_provider_accounts(conn: &Connection) -> AppResult<Vec<ProviderAccount>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider_type, label, keychain_ref, billing_lane, status, last_verified_at, last_error, config_json
         FROM provider_accounts ORDER BY provider_type, label",
    )?;
    let rows = stmt
        .query_map([], parse_provider_account)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_provider_account(conn: &Connection, id: &str) -> AppResult<Option<ProviderAccount>> {
    Ok(conn
        .query_row(
            "SELECT id, provider_type, label, keychain_ref, billing_lane, status, last_verified_at, last_error, config_json
             FROM provider_accounts WHERE id=?1",
            [id],
            parse_provider_account,
        )
        .optional()?)
}

pub fn get_provider_by_type(conn: &Connection, pt: ProviderType) -> AppResult<Option<ProviderAccount>> {
    Ok(conn
        .query_row(
            "SELECT id, provider_type, label, keychain_ref, billing_lane, status, last_verified_at, last_error, config_json
             FROM provider_accounts WHERE provider_type=?1 LIMIT 1",
            [pt.as_str()],
            parse_provider_account,
        )
        .optional()?)
}

pub fn upsert_provider_account(conn: &Connection, acct: &ProviderAccount) -> AppResult<()> {
    let cfg_json = serde_json::to_string(&acct.config).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO provider_accounts (id, provider_type, label, keychain_ref, billing_lane, status, last_verified_at, last_error, config_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(id) DO UPDATE SET
           label=excluded.label,
           keychain_ref=excluded.keychain_ref,
           billing_lane=excluded.billing_lane,
           status=excluded.status,
           last_verified_at=excluded.last_verified_at,
           last_error=excluded.last_error,
           config_json=excluded.config_json",
        params![
            acct.id,
            acct.provider_type.as_str(),
            acct.label,
            acct.keychain_ref,
            acct.billing_lane.as_str(),
            acct.status,
            acct.last_verified_at,
            acct.last_error,
            cfg_json,
        ],
    )?;
    Ok(())
}

pub fn update_provider_status(
    conn: &Connection,
    id: &str,
    status: &str,
    last_verified_at: Option<&str>,
    last_error: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE provider_accounts SET status=?2, last_verified_at=COALESCE(?3, last_verified_at), last_error=?4 WHERE id=?1",
        params![id, status, last_verified_at, last_error],
    )?;
    Ok(())
}

pub fn add_custom_model(conn: &Connection, id: &str, model_id: &str) -> AppResult<ProviderAccount> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(AppError::Validation("model id must be 1..120 characters".into()));
    }
    let mut acct = get_provider_account(conn, id)?
        .ok_or_else(|| AppError::Validation("provider account not found".into()))?;
    if !acct.config.custom_models.iter().any(|m| m == trimmed) {
        acct.config.custom_models.push(trimmed.to_string());
        upsert_provider_account(conn, &acct)?;
    }
    Ok(acct)
}

pub fn delete_provider_account(conn: &Connection, id: &str) -> AppResult<()> {
    // Clear any routes pointing to this account before deleting.
    conn.execute(
        "UPDATE task_model_routes SET provider_account_id=NULL, model_id=NULL, updated_at=?2 WHERE provider_account_id=?1",
        params![id, now_rfc3339()],
    )?;
    conn.execute(
        "UPDATE task_model_routes SET fallback_account_id=NULL, fallback_model_id=NULL, updated_at=?2 WHERE fallback_account_id=?1",
        params![id, now_rfc3339()],
    )?;
    conn.execute("DELETE FROM provider_accounts WHERE id=?1", [id])?;
    Ok(())
}

fn parse_route(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskModelRoute> {
    let tt_str: String = r.get(0)?;
    let task_type = TaskType::parse(&tt_str).unwrap_or(TaskType::ReplyDrafting);
    let temp: Option<f64> = r.get(4)?;
    Ok(TaskModelRoute {
        task_type,
        provider_account_id: r.get(1)?,
        model_id: r.get(2)?,
        reasoning_effort: r.get(3)?,
        temperature: temp.map(|t| t as f32),
        fallback_account_id: r.get(5)?,
        fallback_model_id: r.get(6)?,
        max_calls_per_hour: r.get::<_, Option<u32>>(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn list_task_routes(conn: &Connection) -> AppResult<Vec<TaskModelRoute>> {
    let mut stmt = conn.prepare(
        "SELECT task_type, provider_account_id, model_id, reasoning_effort, temperature,
                fallback_account_id, fallback_model_id, max_calls_per_hour, updated_at
         FROM task_model_routes",
    )?;
    let mut rows = stmt.query_map([], parse_route)?.collect::<Result<Vec<_>, _>>()?;
    rows.sort_by_key(|r| TaskType::ALL.iter().position(|t| *t == r.task_type).unwrap_or(99));
    Ok(rows)
}

pub fn get_task_route(conn: &Connection, task_type: TaskType) -> AppResult<TaskModelRoute> {
    conn.query_row(
        "SELECT task_type, provider_account_id, model_id, reasoning_effort, temperature,
                fallback_account_id, fallback_model_id, max_calls_per_hour, updated_at
         FROM task_model_routes WHERE task_type=?1",
        [task_type.as_str()],
        parse_route,
    )
    .optional()?
    .ok_or_else(|| AppError::Other(format!("missing task route row for {}", task_type.as_str())))
}

pub fn save_task_route(conn: &Connection, patch: TaskModelRoutePatch) -> AppResult<TaskModelRoute> {
    // Validate primary pairing: either both set or both unset.
    let clean = |o: Option<String>| o.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let primary_acc = clean(patch.provider_account_id);
    let primary_mod = clean(patch.model_id);
    if primary_acc.is_some() != primary_mod.is_some() {
        return Err(AppError::Validation(
            "choose both a provider and a model, or leave both unset".into(),
        ));
    }
    let fb_acc = clean(patch.fallback_account_id);
    let fb_mod = clean(patch.fallback_model_id);
    if fb_acc.is_some() != fb_mod.is_some() {
        return Err(AppError::Validation(
            "fallback requires both a fallback provider and a fallback model".into(),
        ));
    }
    if let Some(t) = patch.temperature {
        if !(0.0..=2.0).contains(&t) {
            return Err(AppError::Validation("temperature must be between 0.0 and 2.0".into()));
        }
    }
    if let Some(cap) = patch.max_calls_per_hour {
        if cap == 0 || cap > 10_000 {
            return Err(AppError::Validation("max calls per hour must be between 1 and 10000".into()));
        }
    }
    let effort = clean(patch.reasoning_effort);
    let now = now_rfc3339();
    conn.execute(
        "UPDATE task_model_routes SET
           provider_account_id=?2,
           model_id=?3,
           reasoning_effort=?4,
           temperature=?5,
           fallback_account_id=?6,
           fallback_model_id=?7,
           max_calls_per_hour=?8,
           updated_at=?9
         WHERE task_type=?1",
        params![
            patch.task_type.as_str(),
            primary_acc,
            primary_mod,
            effort,
            patch.temperature.map(|v| v as f64),
            fb_acc,
            fb_mod,
            patch.max_calls_per_hour,
            now,
        ],
    )?;
    get_task_route(conn, patch.task_type)
}

pub fn record_usage(conn: &Connection, entry: &UsageEntry) -> AppResult<()> {
    conn.execute(
        "INSERT INTO llm_usage_logs
         (id, task_type, provider_account_id, provider_type, model_id, prompt_version,
          input_tokens, output_tokens, latency_ms, used_fallback, success, error_kind, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            new_id(),
            entry.task_type.as_str(),
            entry.provider_account_id,
            entry.provider_type.as_str(),
            entry.model_id,
            entry.prompt_version,
            entry.input_tokens as i64,
            entry.output_tokens as i64,
            entry.latency_ms as i64,
            if entry.used_fallback { 1 } else { 0 },
            if entry.success { 1 } else { 0 },
            entry.error_kind,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn calls_in_last_hour(conn: &Connection, task_type: TaskType) -> AppResult<u32> {
    let cutoff = format_unix(now_epoch_secs() - 3600);
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM llm_usage_logs WHERE task_type=?1 AND created_at>=?2 AND success=1",
        params![task_type.as_str(), cutoff],
        |r| r.get(0),
    )?;
    Ok(count)
}

pub fn usage_dashboard(conn: &Connection, days: u32) -> AppResult<UsageDashboard> {
    let days = days.clamp(1, 90);
    let cutoff = format_unix(now_epoch_secs() - i64::from(days) * 86_400);
    let mut stmt = conn.prepare(
        "SELECT task_type, provider_type, model_id,
                COUNT(*) as total_calls,
                SUM(CASE WHEN success=1 THEN 1 ELSE 0 END) as ok_calls,
                SUM(CASE WHEN used_fallback=1 THEN 1 ELSE 0 END) as fb_calls,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(CAST(AVG(latency_ms) AS INTEGER), 0)
         FROM llm_usage_logs
         WHERE created_at >= ?1
         GROUP BY task_type, provider_type, model_id
         ORDER BY total_calls DESC",
    )?;
    let rows = stmt
        .query_map([cutoff], |r| {
            let tt: String = r.get(0)?;
            let pt: String = r.get(1)?;
            Ok(UsageSummaryRow {
                task_type: TaskType::parse(&tt).unwrap_or(TaskType::ReplyDrafting),
                provider_type: ProviderType::parse(&pt).unwrap_or(ProviderType::ClaudeCli),
                model_id: r.get(2)?,
                total_calls: r.get::<_, i64>(3)? as u64,
                successful_calls: r.get::<_, i64>(4)? as u64,
                fallback_calls: r.get::<_, i64>(5)? as u64,
                input_tokens: r.get::<_, i64>(6)? as u64,
                output_tokens: r.get::<_, i64>(7)? as u64,
                avg_latency_ms: r.get::<_, i64>(8)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let total_calls = rows.iter().map(|r| r.total_calls).sum();
    let total_input_tokens = rows.iter().map(|r| r.input_tokens).sum();
    let total_output_tokens = rows.iter().map(|r| r.output_tokens).sum();

    let routes = list_task_routes(conn)?;
    let mut hourly_counts = Vec::with_capacity(routes.len());
    for route in routes {
        let calls_last_hour = calls_in_last_hour(conn, route.task_type)?;
        hourly_counts.push(TaskHourlyCount {
            task_type: route.task_type,
            calls_last_hour,
            max_calls_per_hour: route.max_calls_per_hour,
        });
    }

    Ok(UsageDashboard {
        window_days: days,
        total_calls,
        total_input_tokens,
        total_output_tokens,
        rows,
        hourly_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open;
    use tempfile::tempdir;

    #[test]
    fn provider_accounts_and_routes_round_trip() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();

        let acct = ProviderAccount {
            id: "acc-claude".into(),
            provider_type: ProviderType::ClaudeCli,
            label: "Claude Code CLI (Personal)".into(),
            keychain_ref: None,
            billing_lane: BillingLane::Subscription,
            status: "connected".into(),
            last_verified_at: Some("2026-11-26T10:00:00Z".into()),
            last_error: None,
            config: ProviderAccountConfig::default(),
        };
        upsert_provider_account(&conn, &acct).unwrap();

        // browser_navigation ships unset.
        let nav = get_task_route(&conn, TaskType::BrowserNavigation).unwrap();
        assert!(nav.provider_account_id.is_none());
        assert!(nav.model_id.is_none());

        // Set reply_drafting route.
        let updated = save_task_route(
            &conn,
            TaskModelRoutePatch {
                task_type: TaskType::ReplyDrafting,
                provider_account_id: Some("acc-claude".into()),
                model_id: Some("sonnet".into()),
                reasoning_effort: Some("medium".into()),
                temperature: Some(0.6),
                fallback_account_id: None,
                fallback_model_id: None,
                max_calls_per_hour: Some(60),
            },
        )
        .unwrap();
        assert_eq!(updated.model_id.as_deref(), Some("sonnet"));
        assert_eq!(updated.max_calls_per_hour, Some(60));

        // Partial route pairing rejected.
        assert!(save_task_route(
            &conn,
            TaskModelRoutePatch {
                task_type: TaskType::PostScoring,
                provider_account_id: Some("acc-claude".into()),
                model_id: None,
                reasoning_effort: None,
                temperature: None,
                fallback_account_id: None,
                fallback_model_id: None,
                max_calls_per_hour: None,
            }
        )
        .is_err());

        // Record usage and check hourly + 7d summary.
        record_usage(
            &conn,
            &UsageEntry {
                task_type: TaskType::ReplyDrafting,
                provider_account_id: Some("acc-claude".into()),
                provider_type: ProviderType::ClaudeCli,
                model_id: "sonnet".into(),
                prompt_version: Some(1),
                input_tokens: 320,
                output_tokens: 95,
                latency_ms: 850,
                used_fallback: false,
                success: true,
                error_kind: None,
            },
        )
        .unwrap();
        assert_eq!(calls_in_last_hour(&conn, TaskType::ReplyDrafting).unwrap(), 1);
        let dash = usage_dashboard(&conn, 7).unwrap();
        assert_eq!(dash.total_calls, 1);
        assert_eq!(dash.total_input_tokens, 320);
    }
}
