//! SHL-12: export your data to a zip and import it on another machine.
//!
//! The archive holds a sanitized copy of the SQLite database, a human-readable `style.json`, and a manifest.
//! API keys never leave the OS keychain, the browser profile (X cookies and session) is never read, and the
//! rules acknowledgement is not carried over, so a new machine asks for it again.

use super::patch::{active_rubric, active_style};
use super::versions::examples;
use crate::error::{AppError, AppResult};
use crate::secrets::SecretsService;
use crate::storage::{self, MIGRATIONS};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "social-growth-helper-export";
pub const FORMAT_VERSION: u32 = 1;
const MAX_DB_BYTES: u64 = 1 << 30;
const MAX_SMALL_BYTES: u64 = 4 << 20;

/// Tables that describe this machine rather than your data.
const MACHINE_TABLES: [&str; 2] = ["app_settings", "app_acknowledgements"];

pub const EXCLUDED: [&str; 4] = [
    "API keys and tokens (they stay in the OS keychain; accounts come back as 'needs key')",
    "Browser profile, cookies and X login session",
    "Rules-of-engagement acknowledgement and tour state",
    "Machine settings such as the browser path",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub app_version: String,
    pub schema_version: u32,
    pub created_at: String,
    pub tables: BTreeMap<String, i64>,
    pub excluded: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    pub path: String,
    pub bytes: u64,
    pub tables: BTreeMap<String, i64>,
    pub excluded: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub tables: BTreeMap<String, i64>,
    pub rows: i64,
    pub accounts_needing_keys: i64,
    pub exported_at: String,
    pub source_app_version: String,
    pub safety_backup: Option<String>,
    pub warnings: Vec<String>,
}

fn zerr(e: zip::result::ZipError) -> AppError {
    AppError::Other(format!("zip error: {e}"))
}

struct TempFiles(Vec<PathBuf>);

impl Drop for TempFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
            for ext in ["-wal", "-shm", "-journal"] {
                let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
            }
        }
    }
}

fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Tables that hold user data: virtual tables (FTS5) and their shadow tables are excluded, because
/// copying or deleting shadow rows directly corrupts the index. The virtual table itself is kept.
pub(crate) fn user_table_names(conn: &Connection, schema: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(&format!("SELECT name, sql FROM {schema}.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"))?;
    let rows: Vec<(String, Option<String>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let virtuals: Vec<&str> = rows
        .iter()
        .filter(|(_, sql)| sql.as_deref().is_some_and(|s| s.trim_start().to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE")))
        .map(|(n, _)| n.as_str())
        .collect();
    Ok(rows
        .iter()
        .map(|(n, _)| n.clone())
        .filter(|n| !virtuals.iter().any(|v| n.len() > v.len() && n.starts_with(v) && n.as_bytes()[v.len()] == b'_'))
        .collect())
}

fn columns(conn: &Connection, schema: &str, table: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA {schema}.table_info({})", q(table)))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn count(conn: &Connection, table: &str) -> AppResult<i64> {
    Ok(conn.query_row(&format!("SELECT count(*) FROM {}", q(table)), [], |r| r.get(0))?)
}

fn has_table(conn: &Connection, table: &str) -> AppResult<bool> {
    Ok(user_table_names(conn, "main")?.iter().any(|t| t == table))
}

/// Strips everything that must not travel: keychain refs, machine paths, machine tables.
fn sanitize(snap: &Connection) -> AppResult<()> {
    if has_table(snap, "provider_accounts")? {
        snap.execute("UPDATE provider_accounts SET keychain_ref=NULL, status='needs_key', last_verified_at=NULL", [])?;
    }
    if has_table(snap, "profiles")? {
        snap.execute("UPDATE profiles SET browser_profile_dir=''", [])?;
    }
    for t in MACHINE_TABLES {
        if has_table(snap, t)? {
            snap.execute(&format!("DELETE FROM {}", q(t)), [])?;
        }
    }
    Ok(())
}

pub fn default_export_path() -> PathBuf {
    let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    let stamp = now_rfc3339().replace(['-', ':'], "").replace('T', "-").replace('Z', "");
    dir.join(format!("social-growth-helper-export-{stamp}.zip"))
}

fn style_json(conn: &Connection, profile_id: &str) -> AppResult<serde_json::Value> {
    let rubric = active_rubric(conn, profile_id)?;
    let style = active_style(conn, profile_id)?;
    Ok(serde_json::json!({
        "rubric": { "version": rubric.version, "text": rubric.text, "weights": rubric.weights },
        "style": { "version": style.version, "free_text_guide": style.free_text, "learned_rules": style.rules, "banned_patterns": style.banned },
        "examples": examples(conn, profile_id)?,
    }))
}

pub fn export_to(conn: &Connection, profile_id: &str, work_dir: &Path, dest: &Path) -> AppResult<ExportSummary> {
    if dest.exists() {
        return Err(AppError::Validation(format!("{} already exists; choose another file name", dest.display())));
    }
    std::fs::create_dir_all(work_dir)?;
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let snap_path = work_dir.join(format!(".export-{}.db", new_id()));
    let mut tmp = TempFiles(vec![snap_path.clone()]);
    conn.execute("VACUUM INTO ?1", [snap_path.to_string_lossy().as_ref()])?;

    let mut tables = BTreeMap::new();
    {
        let snap = Connection::open(&snap_path)?;
        sanitize(&snap)?;
        for t in user_table_names(&snap, "main")? {
            tables.insert(t.clone(), count(&snap, &t)?);
        }
        snap.execute_batch("VACUUM")?;
    }
    let manifest = Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema_version: storage::user_version(conn)?,
        created_at: now_rfc3339(),
        tables: tables.clone(),
        excluded: EXCLUDED.iter().map(|s| s.to_string()).collect(),
    };
    let style = serde_json::to_vec_pretty(&style_json(conn, profile_id)?).expect("style json");
    let readme = format!(
        "Social Growth Helper export\n\nimport this file from Settings > Data on the other machine.\n\nNot included:\n{}\n",
        EXCLUDED.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")
    );

    let part = dest.with_extension("zip.part");
    tmp.0.push(part.clone());
    {
        let mut zip = zip::ZipWriter::new(File::create(&part)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts).map_err(zerr)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest).expect("manifest"))?;
        zip.start_file("README.txt", opts).map_err(zerr)?;
        zip.write_all(readme.as_bytes())?;
        zip.start_file("style.json", opts).map_err(zerr)?;
        zip.write_all(&style)?;
        zip.start_file("data.db", opts.large_file(true)).map_err(zerr)?;
        let mut f = File::open(&snap_path)?;
        std::io::copy(&mut f, &mut zip)?;
        zip.finish().map_err(zerr)?;
    }
    std::fs::rename(&part, dest)?;
    tmp.0.retain(|p| p != &part);
    Ok(ExportSummary { path: dest.display().to_string(), bytes: std::fs::metadata(dest)?.len(), tables, excluded: manifest.excluded })
}

fn read_entry(zip: &mut zip::ZipArchive<File>, name: &str, limit: u64) -> AppResult<Vec<u8>> {
    let f = zip.by_name(name).map_err(|_| AppError::Validation(format!("this is not a Social Growth Helper export (missing {name})")))?;
    let mut buf = vec![];
    f.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(AppError::Validation(format!("{name} in the archive is too large")));
    }
    Ok(buf)
}

pub fn read_manifest(src: &Path) -> AppResult<Manifest> {
    let mut zip = zip::ZipArchive::new(File::open(src)?).map_err(|_| AppError::Validation("this file is not a zip archive".into()))?;
    let m: Manifest = serde_json::from_slice(&read_entry(&mut zip, "manifest.json", MAX_SMALL_BYTES)?)
        .map_err(|_| AppError::Validation("the export manifest is unreadable".into()))?;
    if m.format != FORMAT {
        return Err(AppError::Validation("this is not a Social Growth Helper export".into()));
    }
    if m.format_version > FORMAT_VERSION || m.schema_version > storage::latest_version() {
        return Err(AppError::Validation("this export was made by a newer version of the app; update the app and try again".into()));
    }
    Ok(m)
}

/// Replaces this machine's data with the archive's. A backup of the current database is written first, and the
/// swap is one transaction, so a bad archive leaves everything as it was.
pub fn import_from(conn: &Connection, profile_id: &str, work_dir: &Path, backup_dir: &Path, src: &Path, secrets: &SecretsService) -> AppResult<ImportSummary> {
    let manifest = read_manifest(src)?;
    std::fs::create_dir_all(work_dir)?;
    let snap_path = work_dir.join(format!(".import-{}.db", new_id()));
    let _tmp = TempFiles(vec![snap_path.clone()]);
    {
        let mut zip = zip::ZipArchive::new(File::open(src)?).map_err(|_| AppError::Validation("this file is not a zip archive".into()))?;
        let entry = zip.by_name("data.db").map_err(|_| AppError::Validation("this is not a Social Growth Helper export (missing data.db)".into()))?;
        let mut out = File::create(&snap_path)?;
        let written = std::io::copy(&mut entry.take(MAX_DB_BYTES + 1), &mut out)?;
        if written > MAX_DB_BYTES {
            return Err(AppError::Validation("data.db in the archive is too large".into()));
        }
    }

    let (snap_handle, snap_display, accounts_needing_keys) = {
        let mut snap = Connection::open(&snap_path).map_err(|_| AppError::Validation("the archive's database is unreadable".into()))?;
        snap.execute_batch("PRAGMA foreign_keys=OFF")?;
        let check: String = snap
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| AppError::Validation("the archive's database is unreadable".into()))?;
        if check != "ok" {
            return Err(AppError::Validation("the archive's database failed its integrity check".into()));
        }
        if storage::user_version(&snap)? != manifest.schema_version {
            return Err(AppError::Validation("the archive's database does not match its manifest".into()));
        }
        storage::migrate(&mut snap, MIGRATIONS, None)?;
        sanitize(&snap)?;
        for t in user_table_names(&snap, "main")? {
            if columns(&snap, "main", &t)?.iter().any(|c| c == "profile_id") {
                snap.execute(&format!("UPDATE {} SET profile_id=?1", q(&t)), [profile_id])?;
            }
        }
        let (h, d): (String, Option<String>) = snap
            .query_row("SELECT handle, display_name FROM profiles ORDER BY is_active DESC, created_at LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or_default();
        let needs: i64 = if has_table(&snap, "provider_accounts")? { count(&snap, "provider_accounts")? } else { 0 };
        (h, d, needs)
    };

    let old_refs: Vec<String> = if has_table(conn, "provider_accounts")? {
        let mut stmt = conn.prepare("SELECT keychain_ref FROM provider_accounts WHERE keychain_ref IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    } else {
        vec![]
    };

    let safety_backup = storage::backup_database(conn, backup_dir, storage::user_version(conn)?)?;

    conn.execute("ATTACH DATABASE ?1 AS imp", [snap_path.to_string_lossy().as_ref()])?;
    conn.execute_batch("PRAGMA foreign_keys=OFF")?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> AppResult<BTreeMap<String, i64>> {
        let skip = |t: &str| t == "profiles" || MACHINE_TABLES.contains(&t);
        for t in user_table_names(conn, "main")?.into_iter().filter(|t| !skip(t)) {
            conn.execute(&format!("DELETE FROM main.{}", q(&t)), [])?;
        }
        let mut copied = BTreeMap::new();
        let main_tables = user_table_names(conn, "main")?;
        for t in user_table_names(conn, "imp")?.into_iter().filter(|t| !skip(t) && main_tables.contains(t)) {
            let main_cols = columns(conn, "main", &t)?;
            let cols: Vec<String> = columns(conn, "imp", &t)?.into_iter().filter(|c| main_cols.contains(c)).map(|c| q(&c)).collect();
            if cols.is_empty() {
                continue;
            }
            let list = cols.join(", ");
            let n = conn.execute(&format!("INSERT INTO main.{t_q} ({list}) SELECT {list} FROM imp.{t_q}", t_q = q(&t)), [])?;
            copied.insert(t, n as i64);
        }
        conn.execute("UPDATE profiles SET handle=?1, display_name=?2 WHERE id=?3", params![snap_handle, snap_display, profile_id])?;
        let violations = {
            let mut stmt = conn.prepare("PRAGMA main.foreign_key_check")?;
            let n = stmt.query_map([], |_| Ok(()))?.count();
            n
        };
        if violations > 0 {
            return Err(AppError::Validation("the archive has inconsistent data (broken references); nothing was imported".into()));
        }
        Ok(copied)
    })();
    let outcome = match result {
        Ok(copied) => conn.execute_batch("COMMIT").map(|_| copied).map_err(AppError::from),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    };
    let _ = conn.execute_batch("DETACH DATABASE imp");
    let _ = conn.execute_batch("PRAGMA foreign_keys=ON");
    let tables = outcome?;

    let mut warnings = vec![];
    for r in old_refs {
        if let Err(e) = secrets.remove(&r) {
            warnings.push(format!("could not remove a previous API key from the keychain: {e}"));
            break;
        }
    }
    if accounts_needing_keys > 0 {
        warnings.push(format!("{accounts_needing_keys} provider account(s) need their API key entered again on this machine"));
    }
    Ok(ImportSummary {
        rows: tables.values().sum(),
        tables,
        accounts_needing_keys,
        exported_at: manifest.created_at,
        source_app_version: manifest.app_version,
        safety_backup: Some(safety_backup.display().to_string()),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::patch::tests::{db, seed_published_draft};
    use crate::learning::patch::{active_style, apply, Patch, PatchOp, PatchTarget};
    use crate::secrets::{MemoryBackend, Platform};
    use crate::storage::repo;
    use tempfile::tempdir;

    fn secrets() -> SecretsService {
        SecretsService::with_backend(Box::new(MemoryBackend::new()), Platform::Macos, true)
    }

    fn populate(c: &Connection, pid: &str) {
        repo::save_preferences(c, pid, repo::Preferences { niche_topics: vec!["rust".into()], ..Default::default() }).unwrap();
        seed_published_draft(c, pid, "d1", "Shipping beats planning every day");
        apply(c, pid, &Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "great point".into() }] }, None).unwrap();
        let promote = PatchOp::PromoteExample {
            draft_id: "d1".into(),
            example_text: "Shipping beats planning every day".into(),
            source_context_text: None,
            why_it_works: None,
            performance_score: Some(2.0),
        };
        apply(c, pid, &Patch { target: PatchTarget::Examples, ops: vec![promote] }, None).unwrap();
        c.execute("INSERT INTO provider_accounts (id, provider_type, label, keychain_ref, billing_lane, status) VALUES ('a1','anthropic','Work','provider:SECRET-REF-123','api','verified')", []).unwrap();
        repo::set_setting(c, "browser_path_override", "/opt/chrome-secret-path").unwrap();
        repo::record_acknowledgement(c, "rules_of_engagement", 1).unwrap();
        c.execute("UPDATE profiles SET handle='pavel', display_name='Pavel'", []).unwrap();
    }

    #[test]
    fn export_contains_no_secrets_cookies_or_machine_state() {
        let (d, c, pid) = db();
        populate(&c, &pid);
        let dest = d.path().join("out").join("export.zip");
        let s = export_to(&c, &pid, &d.path().join("work"), &dest).unwrap();
        assert!(dest.exists() && s.bytes > 0);
        assert!(std::fs::read_dir(d.path().join("work")).unwrap().next().is_none(), "temp files are cleaned up");
        assert!(!dest.with_extension("zip.part").exists());

        let mut zip = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
        assert_eq!(names, vec!["manifest.json", "README.txt", "style.json", "data.db"]);
        let db_bytes = read_entry(&mut zip, "data.db", MAX_DB_BYTES).unwrap();
        for needle in ["SECRET-REF-123", "chrome-secret-path", "rules_of_engagement"] {
            assert!(!db_bytes.windows(needle.len()).any(|w| w == needle.as_bytes()), "{needle} must not be in the export");
        }
        let style: serde_json::Value = serde_json::from_slice(&read_entry(&mut zip, "style.json", MAX_SMALL_BYTES).unwrap()).unwrap();
        assert_eq!(style["style"]["banned_patterns"][0], "great point");
        let m = read_manifest(&dest).unwrap();
        assert_eq!(m.tables["app_settings"], 0);
        assert_eq!(m.tables["app_acknowledgements"], 0);
        assert_eq!(m.tables["reply_drafts"], 1);
        assert_eq!(m.schema_version, storage::latest_version());
        assert!(export_to(&c, &pid, &d.path().join("work"), &dest).is_err(), "never overwrites");
    }

    #[test]
    fn import_on_another_machine_restores_data_but_not_machine_state() {
        let (d, c, pid) = db();
        populate(&c, &pid);
        let dest = d.path().join("export.zip");
        export_to(&c, &pid, &d.path().join("work"), &dest).unwrap();

        let d2 = tempdir().unwrap();
        let (c2, _) = storage::open(&d2.path().join("b.db"), &d2.path().join("bk")).unwrap();
        let pid2 = repo::ensure_default_profile(&c2, "/other/profile/dir").unwrap();
        assert_ne!(pid, pid2);
        repo::set_setting(&c2, "browser_path_override", "/mine").unwrap();
        c2.execute("INSERT INTO provider_accounts (id, provider_type, label, keychain_ref, billing_lane, status) VALUES ('old','openai','Old','provider:old','api','verified')", []).unwrap();
        let sec = secrets();
        sec.store("provider:old", "k").unwrap();

        let s = import_from(&c2, &pid2, &d2.path().join("work"), &d2.path().join("bk"), &dest, &sec).unwrap();
        assert_eq!(s.tables["reply_drafts"], 1);
        let hits: i64 = c2.query_row("SELECT count(*) FROM style_examples_fts WHERE style_examples_fts MATCH 'planning'", [], |r| r.get(0)).unwrap();
        assert_eq!(hits, 1, "promoted examples stay searchable after import");
        assert!(!s.tables.keys().any(|t| t.ends_with("_data") || t.ends_with("_idx") || t.ends_with("_docsize") || t.ends_with("_config")));
        assert_eq!(s.accounts_needing_keys, 1);
        assert!(s.safety_backup.is_some());

        let style = active_style(&c2, &pid2).unwrap();
        assert_eq!(style.banned, vec!["great point"]);
        assert_eq!(repo::get_preferences(&c2, &pid2).unwrap().unwrap().niche_topics, vec!["rust"]);
        let owner: String = c2.query_row("SELECT profile_id FROM reply_drafts WHERE id='d1'", [], |r| r.get(0)).unwrap();
        assert_eq!(owner, pid2, "data is re-homed to this machine's profile");
        let (handle, dir): (String, String) = c2.query_row("SELECT handle, browser_profile_dir FROM profiles WHERE id=?1", [&pid2], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((handle.as_str(), dir.as_str()), ("pavel", "/other/profile/dir"));
        assert_eq!(repo::get_setting(&c2, "browser_path_override").unwrap().as_deref(), Some("/mine"));
        assert!(repo::latest_acknowledgement(&c2, "rules_of_engagement").unwrap().is_none(), "acknowledgement must be given again");
        let (n, ref_, status): (i64, Option<String>, String) = c2.query_row("SELECT count(*), keychain_ref, status FROM provider_accounts", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((n, ref_, status.as_str()), (1, None, "needs_key"));
        assert!(sec.load("provider:old").unwrap().is_none(), "replaced accounts' keys are removed from the keychain");
        let fk: i64 = c2.query_row("SELECT count(*) FROM pragma_foreign_key_list('reply_drafts')", [], |r| r.get(0)).unwrap();
        assert!(fk > 0);
        let fk_on: i64 = c2.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk_on, 1);
    }

    #[test]
    fn bad_archives_are_rejected_without_touching_data() {
        let (d, c, pid) = db();
        populate(&c, &pid);
        let junk = d.path().join("junk.zip");
        std::fs::write(&junk, b"not a zip").unwrap();
        let sec = secrets();
        let w = d.path().join("work");
        let bk = d.path().join("bk");
        assert!(import_from(&c, &pid, &w, &bk, &junk, &sec).is_err());

        let wrong = d.path().join("wrong.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&wrong).unwrap());
            z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(br#"{"format":"something-else","format_version":1,"app_version":"1","schema_version":1,"created_at":"x","tables":{},"excluded":[]}"#).unwrap();
            z.finish().unwrap();
        }
        assert!(import_from(&c, &pid, &w, &bk, &wrong, &sec).unwrap_err().to_string().contains("not a Social Growth Helper export"));

        let newer = d.path().join("newer.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&newer).unwrap());
            z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(format!(r#"{{"format":"{FORMAT}","format_version":1,"app_version":"9","schema_version":9999,"created_at":"x","tables":{{}},"excluded":[]}}"#).as_bytes()).unwrap();
            z.finish().unwrap();
        }
        assert!(import_from(&c, &pid, &w, &bk, &newer, &sec).unwrap_err().to_string().contains("newer version"));

        let n: i64 = c.query_row("SELECT count(*) FROM reply_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert!(active_style(&c, &pid).unwrap().banned.contains(&"great point".to_string()));
    }

    #[test]
    fn corrupt_references_roll_the_whole_import_back() {
        let (d, c, pid) = db();
        populate(&c, &pid);
        let dest = d.path().join("export.zip");
        export_to(&c, &pid, &d.path().join("work"), &dest).unwrap();

        // Rebuild the archive with a dangling foreign key in data.db.
        let snap = d.path().join("evil.db");
        {
            let mut z = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
            let bytes = read_entry(&mut z, "data.db", MAX_DB_BYTES).unwrap();
            std::fs::write(&snap, bytes).unwrap();
            let e = Connection::open(&snap).unwrap();
            e.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
            e.execute("INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, text, feed_source, first_seen_at, metrics_at_capture_json) VALUES ('x','p','x','a','t','for_you','2026-01-01T00:00:00Z','{}')", []).unwrap();
            e.execute_batch("UPDATE reply_drafts SET observed_post_id='missing'").unwrap();
        }
        let evil = d.path().join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&evil).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            let mut orig = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
            z.start_file("manifest.json", o).unwrap();
            z.write_all(&read_entry(&mut orig, "manifest.json", MAX_SMALL_BYTES).unwrap()).unwrap();
            z.start_file("data.db", o).unwrap();
            z.write_all(&std::fs::read(&snap).unwrap()).unwrap();
            z.finish().unwrap();
        }
        repo::set_setting(&c, "marker", "1").unwrap();
        let err = import_from(&c, &pid, &d.path().join("work"), &d.path().join("bk"), &evil, &secrets()).unwrap_err();
        assert!(err.to_string().contains("inconsistent"), "{err}");
        let n: i64 = c.query_row("SELECT count(*) FROM reply_drafts WHERE observed_post_id='op-d1'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "original data is intact");
        let fk_on: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk_on, 1);
        assert!(c.execute("ATTACH DATABASE ':memory:' AS imp", []).is_ok(), "imp was detached");
    }
}
