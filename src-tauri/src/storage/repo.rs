//! Typed access to the handful of tables Phase 1 writes: profile, preferences, settings, acknowledgements.

use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Returns the single active X profile id, creating the placeholder profile on first run (SHL-10).
/// There is deliberately no API to add a second profile in v1.
pub fn ensure_default_profile(conn: &Connection, browser_profile_dir: &str) -> AppResult<String> {
    if let Some(id) = conn
        .query_row("SELECT id FROM profiles WHERE platform='x' AND is_active=1", [], |r| r.get::<_, String>(0))
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO profiles (id, platform, handle, browser_profile_dir, is_active, created_at) VALUES (?1,'x','',?2,1,?3)",
        params![id, browser_profile_dir, now_rfc3339()],
    )?;
    Ok(id)
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1,?2,?3)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        params![key, value, now_rfc3339()],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM app_settings WHERE key=?1", [key])?;
    Ok(())
}

/// Onboarding answers that seed the scout rubric and filters (SHL-26).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    pub niche_topics: Vec<String>,
    pub prefer_accounts: Vec<String>,
    pub avoid_accounts: Vec<String>,
    pub languages: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            niche_topics: vec![],
            prefer_accounts: vec![],
            avoid_accounts: vec![],
            languages: vec!["en".into()],
        }
    }
}

const MAX_ITEMS: usize = 50;

fn dedupe_ci(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|s| seen.insert(s.to_lowercase()))
        .collect()
}

fn normalize_handle(raw: &str) -> AppResult<String> {
    let h = raw.trim().trim_start_matches('@').trim();
    let valid = !h.is_empty() && h.len() <= 15 && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(AppError::Validation(format!(
            "\"{raw}\" is not a valid X handle (1-15 letters, digits or underscores)"
        )));
    }
    Ok(h.to_string())
}

fn normalize_language(raw: &str) -> AppResult<String> {
    let l = raw.trim().to_lowercase();
    let valid = (2..=8).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
    if !valid {
        return Err(AppError::Validation(format!("\"{raw}\" is not a valid language code (for example en, de, pt-br)")));
    }
    Ok(l)
}

impl Preferences {
    pub fn normalized(self) -> AppResult<Self> {
        let topics = dedupe_ci(
            self.niche_topics
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
        );
        if topics.iter().any(|t| t.chars().count() > 80) {
            return Err(AppError::Validation("a niche topic is longer than 80 characters".into()));
        }
        let prefer = dedupe_ci(
            self.prefer_accounts
                .iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| normalize_handle(s))
                .collect::<AppResult<Vec<_>>>()?,
        );
        let avoid = dedupe_ci(
            self.avoid_accounts
                .iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| normalize_handle(s))
                .collect::<AppResult<Vec<_>>>()?,
        );
        if let Some(both) = prefer.iter().find(|p| avoid.iter().any(|a| a.eq_ignore_ascii_case(p))) {
            return Err(AppError::Validation(format!("@{both} is in both the prefer and avoid lists")));
        }
        let mut languages = dedupe_ci(
            self.languages
                .iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| normalize_language(s))
                .collect::<AppResult<Vec<_>>>()?,
        );
        if languages.is_empty() {
            languages.push("en".into());
        }
        if topics.len() > MAX_ITEMS || prefer.len() > MAX_ITEMS || avoid.len() > MAX_ITEMS {
            return Err(AppError::Validation(format!("each list is limited to {MAX_ITEMS} entries")));
        }
        Ok(Self {
            niche_topics: topics,
            prefer_accounts: prefer,
            avoid_accounts: avoid,
            languages,
        })
    }
}

pub fn get_preferences(conn: &Connection, profile_id: &str) -> AppResult<Option<Preferences>> {
    let row = conn
        .query_row(
            "SELECT niche_topics_json, prefer_accounts_json, avoid_accounts_json, languages_json
             FROM profile_preferences WHERE profile_id=?1",
            [profile_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        )
        .optional()?;
    let Some((t, p, a, l)) = row else { return Ok(None) };
    let parse = |s: &str| -> AppResult<Vec<String>> {
        serde_json::from_str(s).map_err(|e| AppError::Other(format!("corrupt preferences JSON: {e}")))
    };
    Ok(Some(Preferences {
        niche_topics: parse(&t)?,
        prefer_accounts: parse(&p)?,
        avoid_accounts: parse(&a)?,
        languages: parse(&l)?,
    }))
}

pub fn save_preferences(conn: &Connection, profile_id: &str, prefs: Preferences) -> AppResult<Preferences> {
    let prefs = prefs.normalized()?;
    let j = |v: &Vec<String>| serde_json::to_string(v).expect("vec of strings serializes");
    conn.execute(
        "INSERT INTO profile_preferences (profile_id, niche_topics_json, prefer_accounts_json, avoid_accounts_json, languages_json, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(profile_id) DO UPDATE SET
           niche_topics_json=excluded.niche_topics_json,
           prefer_accounts_json=excluded.prefer_accounts_json,
           avoid_accounts_json=excluded.avoid_accounts_json,
           languages_json=excluded.languages_json,
           updated_at=excluded.updated_at",
        params![
            profile_id,
            j(&prefs.niche_topics),
            j(&prefs.prefer_accounts),
            j(&prefs.avoid_accounts),
            j(&prefs.languages),
            now_rfc3339()
        ],
    )?;
    Ok(prefs)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Acknowledgement {
    pub kind: String,
    pub notice_version: u32,
    pub acknowledged_at: String,
}

pub fn latest_acknowledgement(conn: &Connection, kind: &str) -> AppResult<Option<Acknowledgement>> {
    Ok(conn
        .query_row(
            "SELECT kind, notice_version, acknowledged_at FROM app_acknowledgements
             WHERE kind=?1 ORDER BY notice_version DESC, acknowledged_at DESC LIMIT 1",
            [kind],
            |r| {
                Ok(Acknowledgement {
                    kind: r.get(0)?,
                    notice_version: r.get(1)?,
                    acknowledged_at: r.get(2)?,
                })
            },
        )
        .optional()?)
}

pub fn record_acknowledgement(conn: &Connection, kind: &str, notice_version: u32) -> AppResult<Acknowledgement> {
    let ack = Acknowledgement {
        kind: kind.to_string(),
        notice_version,
        acknowledged_at: now_rfc3339(),
    };
    conn.execute(
        "INSERT INTO app_acknowledgements (id, kind, notice_version, acknowledged_at) VALUES (?1,?2,?3,?4)",
        params![new_id(), ack.kind, ack.notice_version, ack.acknowledged_at],
    )?;
    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open;
    use tempfile::{tempdir, TempDir};

    fn db() -> (TempDir, Connection) {
        let dir = tempdir().unwrap();
        let (c, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        (dir, c)
    }

    #[test]
    fn default_profile_is_created_once() {
        let (_d, c) = db();
        let a = ensure_default_profile(&c, "/profiles/x-default").unwrap();
        let b = ensure_default_profile(&c, "/other").unwrap();
        assert_eq!(a, b);
        let n: i64 = c.query_row("SELECT count(*) FROM profiles", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn preferences_round_trip_and_normalize() {
        let (_d, c) = db();
        let pid = ensure_default_profile(&c, "/p").unwrap();
        assert!(get_preferences(&c, &pid).unwrap().is_none());
        let saved = save_preferences(
            &c,
            &pid,
            Preferences {
                niche_topics: vec!["  rust ".into(), "Rust".into(), "indie hacking".into(), "".into()],
                prefer_accounts: vec!["@naval".into(), "paulg".into(), "NAVAL".into()],
                avoid_accounts: vec!["@spammer_1".into()],
                languages: vec!["EN".into(), "de".into(), "en".into()],
            },
        )
        .unwrap();
        assert_eq!(saved.niche_topics, vec!["rust", "indie hacking"]);
        assert_eq!(saved.prefer_accounts, vec!["naval", "paulg"]);
        assert_eq!(saved.languages, vec!["en", "de"]);
        assert_eq!(get_preferences(&c, &pid).unwrap().unwrap(), saved);

        let edited = save_preferences(&c, &pid, Preferences { languages: vec![], ..saved }).unwrap();
        assert_eq!(edited.languages, vec!["en"], "empty language list falls back to English");
    }

    #[test]
    fn preferences_reject_bad_input() {
        let bad_handle = Preferences { prefer_accounts: vec!["not a handle!".into()], ..Default::default() };
        assert!(bad_handle.normalized().is_err());
        let both = Preferences {
            prefer_accounts: vec!["@same".into()],
            avoid_accounts: vec!["Same".into()],
            ..Default::default()
        };
        assert!(both.normalized().unwrap_err().to_string().contains("both"));
        let bad_lang = Preferences { languages: vec!["e".into()], ..Default::default() };
        assert!(bad_lang.normalized().is_err());
    }

    #[test]
    fn settings_upsert_and_delete() {
        let (_d, c) = db();
        assert_eq!(get_setting(&c, "k").unwrap(), None);
        set_setting(&c, "k", "1").unwrap();
        set_setting(&c, "k", "2").unwrap();
        assert_eq!(get_setting(&c, "k").unwrap().as_deref(), Some("2"));
        delete_setting(&c, "k").unwrap();
        assert_eq!(get_setting(&c, "k").unwrap(), None);
    }

    #[test]
    fn acknowledgements_return_latest_version() {
        let (_d, c) = db();
        assert!(latest_acknowledgement(&c, "rules_of_engagement").unwrap().is_none());
        record_acknowledgement(&c, "rules_of_engagement", 1).unwrap();
        record_acknowledgement(&c, "rules_of_engagement", 2).unwrap();
        assert_eq!(latest_acknowledgement(&c, "rules_of_engagement").unwrap().unwrap().notice_version, 2);
    }
}
