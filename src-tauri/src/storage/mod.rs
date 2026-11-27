//! SQLite storage: WAL + foreign keys, forward-only versioned migrations, and a pre-migration backup (SHL-11).

pub mod repo;

use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "rev2_initial",
        sql: include_str!("migrations/0001_rev2_initial.sql"),
    },
    Migration {
        version: 2,
        name: "app_settings",
        sql: include_str!("migrations/0002_app_settings.sql"),
    },
    Migration {
        version: 3,
        name: "phase2_llm_style",
        sql: include_str!("migrations/0003_phase2_llm_style.sql"),
    },
    Migration {
        version: 4,
        name: "phase3",
        sql: include_str!("migrations/0004_phase3.sql"),
    },
    Migration {
        version: 5,
        name: "phase4_analytics_views",
        sql: include_str!("migrations/0005_phase4_analytics_views.sql"),
    },
    Migration {
        version: 6,
        name: "learning",
        sql: include_str!("migrations/0006_learning.sql"),
    },
    Migration {
        version: 7,
        name: "viral_formats_creator",
        sql: include_str!("migrations/0007_viral_formats_creator.sql"),
    },
    Migration {
        version: 8,
        name: "outlier_scoring",
        sql: include_str!("migrations/0008_outlier_scoring.sql"),
    },
];

const BACKUPS_TO_KEEP: usize = 5;

pub fn latest_version() -> u32 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub from_version: u32,
    pub to_version: u32,
    pub applied: Vec<u32>,
    pub backup_path: Option<PathBuf>,
}

pub fn apply_pragmas(conn: &Connection) -> AppResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

pub fn user_version(conn: &Connection) -> AppResult<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?)
}

#[cfg(test)]
pub fn open_in_memory() -> AppResult<Connection> {
    let mut conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn, MIGRATIONS, None)?;
    Ok(conn)
}

/// Opens (creating if needed) the app database, backs it up before any upgrade, and migrates it.
pub fn open(db_path: &Path, backup_dir: &Path) -> AppResult<(Connection, MigrationReport)> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(db_path)?;
    apply_pragmas(&conn)?;
    let report = migrate(&mut conn, MIGRATIONS, Some(backup_dir))?;
    Ok((conn, report))
}

/// Applies every migration newer than `PRAGMA user_version`, each in its own transaction.
/// A backup is written via `VACUUM INTO` before the first change when upgrading an existing database.
pub fn migrate(
    conn: &mut Connection,
    migrations: &[Migration],
    backup_dir: Option<&Path>,
) -> AppResult<MigrationReport> {
    let current = user_version(conn)?;
    let target = migrations.last().map(|m| m.version).unwrap_or(0);
    if current > target {
        return Err(AppError::Migration(format!(
            "database schema v{current} is newer than this app supports (v{target}); refusing to open it"
        )));
    }

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    let mut backup_path = None;
    if current > 0 && !pending.is_empty() {
        if let Some(dir) = backup_dir {
            backup_path = Some(backup_database(conn, dir, current)?);
            prune_backups(dir, BACKUPS_TO_KEEP);
        }
    }

    let mut applied = Vec::new();
    for m in pending {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)
            .map_err(|e| AppError::Migration(format!("migration {} ({}) failed: {e}", m.version, m.name)))?;
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
        applied.push(m.version);
    }

    Ok(MigrationReport {
        from_version: current,
        to_version: target.max(current),
        applied,
        backup_path,
    })
}

pub fn backup_database(conn: &Connection, dir: &Path, version: u32) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = crate::util::now_rfc3339().replace([':', '-'], "");
    let path = dir.join(format!("app-v{version}-{stamp}.db"));
    let mut n = 1;
    let mut final_path = path.clone();
    while final_path.exists() {
        final_path = dir.join(format!("app-v{version}-{stamp}-{n}.db"));
        n += 1;
    }
    conn.execute("VACUUM INTO ?1", [final_path.to_string_lossy().as_ref()])?;
    Ok(final_path)
}

fn prune_backups(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("app-v"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(old.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn table_count(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn fresh_database_gets_wal_fks_and_full_schema() {
        let dir = tempdir().unwrap();
        let (conn, report) = open(&dir.path().join("app.db"), &dir.path().join("backups")).unwrap();
        assert_eq!(report.from_version, 0);
        assert_eq!(report.to_version, latest_version());
        assert!(report.backup_path.is_none(), "no backup for a brand-new database");

        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);

        for t in [
            "profiles", "provider_accounts", "task_model_routes", "style_guides", "style_examples",
            "style_examples_fts", "llm_usage_logs", "prompt_templates",
            "observed_posts", "scout_evaluations", "viral_formats", "reply_drafts", "ogc_drafts",
            "publish_audit_log", "analytics_snapshots", "learning_rubrics", "learning_insights",
            "profile_preferences", "app_acknowledgements", "scout_sessions", "dashboard_layouts",
            "dashboard_widgets", "app_settings", "learning_runs", "learning_changelog",
            "format_candidates", "creator_watchlist",
        ] {
            let n: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "missing table {t}");
        }
    }

    #[test]
    fn all_seven_model_routes_ship_unset() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("app.db"), &dir.path().join("b")).unwrap();
        let unset: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_model_routes WHERE provider_account_id IS NULL AND model_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unset, 7);
        let nav: i64 = conn
            .query_row("SELECT count(*) FROM task_model_routes WHERE task_type='browser_navigation'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(nav, 1);
    }

    #[test]
    fn reopening_is_idempotent() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("app.db");
        let bk = dir.path().join("backups");
        open(&db, &bk).unwrap();
        let (_, report) = open(&db, &bk).unwrap();
        assert!(report.applied.is_empty());
        assert!(report.backup_path.is_none());
    }

    #[test]
    fn upgrade_from_older_version_backs_up_first_and_keeps_data() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("app.db");
        let bk = dir.path().join("backups");

        {
            let mut conn = Connection::open(&db).unwrap();
            apply_pragmas(&conn).unwrap();
            migrate(&mut conn, &MIGRATIONS[..1], Some(&bk)).unwrap();
            conn.execute(
                "INSERT INTO profiles (id, platform, handle, browser_profile_dir, created_at) VALUES ('p1','x','pavel','/tmp/p','2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            assert_eq!(user_version(&conn).unwrap(), 1);
        }

        let (conn, report) = open(&db, &bk).unwrap();
        assert_eq!(report.from_version, 1);
        let expected: Vec<u32> = MIGRATIONS.iter().map(|m| m.version).filter(|v| *v > 1).collect();
        assert_eq!(report.applied, expected);
        let backup = report.backup_path.expect("backup must exist before migrating an existing db");
        assert!(backup.exists());

        let handle: String = conn.query_row("SELECT handle FROM profiles WHERE id='p1'", [], |r| r.get(0)).unwrap();
        assert_eq!(handle, "pavel");

        let b = Connection::open(&backup).unwrap();
        assert_eq!(user_version(&b).unwrap(), 1, "backup holds the pre-migration schema");
        let has_settings: i64 = b
            .query_row("SELECT count(*) FROM sqlite_master WHERE name='app_settings'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(has_settings, 0);
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_version() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("app.db");
        let (mut conn, _) = open(&db, &dir.path().join("b")).unwrap();
        let before = table_count(&conn);
        let bad_version = latest_version() + 1;
        let mut bad: Vec<Migration> = (1..=latest_version())
            .map(|version| Migration { version, name: "noop", sql: "SELECT 1;" })
            .collect();
        bad.push(Migration {
            version: bad_version,
            name: "bad",
            sql: "CREATE TABLE ok_table (x INT); CREATE TABLE ok_table (x INT);",
        });
        let err = migrate(&mut conn, &bad, None).unwrap_err();
        assert!(err.to_string().contains(&format!("migration {bad_version}")));
        assert_eq!(user_version(&conn).unwrap(), latest_version());
        assert_eq!(table_count(&conn), before, "partial DDL must roll back");
    }

    #[test]
    fn refuses_database_newer_than_app() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("app.db");
        {
            let c = Connection::open(&db).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        let err = open(&db, &dir.path().join("b")).err().unwrap();
        assert!(err.to_string().contains("newer"));
    }

    #[test]
    fn only_one_active_profile_per_platform() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("app.db"), &dir.path().join("b")).unwrap();
        let ins = "INSERT INTO profiles (id, platform, handle, browser_profile_dir, is_active, created_at) VALUES (?1,'x','h','/d',?2,'t')";
        conn.execute(ins, rusqlite::params!["a", 1]).unwrap();
        assert!(conn.execute(ins, rusqlite::params!["b", 1]).is_err(), "second active X profile must be rejected");
        conn.execute(ins, rusqlite::params!["c", 0]).unwrap();
    }

    #[test]
    fn backups_are_pruned() {
        let dir = tempdir().unwrap();
        for i in 0..9 {
            std::fs::write(dir.path().join(format!("app-v1-2026010{i}.db")), b"x").unwrap();
        }
        prune_backups(dir.path(), 5);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 5);
    }
}
