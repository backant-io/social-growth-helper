//! Reply voice: the user's own system prompt for reply drafting, plus meta-level "successful comment
//! formats" distilled from replies that performed well. A format is a structure with slots, never the
//! exact words of any reply, so drafts borrow the shape and not the sentences.

pub mod commands;
pub mod fetch;
pub mod repo;

use crate::drafts::generate::ReplyShape;
use crate::error::{AppError, AppResult};
use crate::storage::repo::{delete_setting, get_setting, set_setting};
use rusqlite::Connection;

pub use repo::ReplyFormat;

const PROMPT_KEY: &str = "reply_system_prompt";
pub const MAX_PROMPT_CHARS: usize = 4_000;

/// The saved custom prompt, or `None` when the built-in opening is in use.
pub fn get_custom_prompt(conn: &Connection) -> AppResult<Option<String>> {
    Ok(get_setting(conn, PROMPT_KEY)?.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()))
}

/// Saves the prompt; an empty one goes back to the built-in. Returns what is now stored.
pub fn set_custom_prompt(conn: &Connection, prompt: &str) -> AppResult<Option<String>> {
    let p = prompt.trim();
    if p.chars().count() > MAX_PROMPT_CHARS {
        return Err(AppError::Validation(format!("The reply prompt is limited to {MAX_PROMPT_CHARS} characters.")));
    }
    if p.is_empty() {
        delete_setting(conn, PROMPT_KEY)?;
        return Ok(None);
    }
    set_setting(conn, PROMPT_KEY, p)?;
    Ok(Some(p.to_string()))
}

/// Enabled formats as they appear in a drafting prompt, best performing first.
pub fn shapes_for_prompt(conn: &Connection, profile_id: &str, limit: usize) -> AppResult<Vec<ReplyShape>> {
    Ok(repo::list_enabled(conn, profile_id, limit)?
        .into_iter()
        .map(|f| ReplyShape { name: f.name, pattern: f.pattern, when_to_use: f.when_to_use })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::generate::{build_prompt, build_slots, GenRequest, Instruction, StyleContext};

    fn db() -> Connection {
        crate::storage::open_in_memory().unwrap()
    }

    #[test]
    fn prompt_saves_trims_and_resets() {
        let conn = db();
        assert_eq!(get_custom_prompt(&conn).unwrap(), None);
        assert_eq!(set_custom_prompt(&conn, "  Write like a tired staff engineer.  ").unwrap().as_deref(), Some("Write like a tired staff engineer."));
        assert_eq!(get_custom_prompt(&conn).unwrap().as_deref(), Some("Write like a tired staff engineer."));
        assert_eq!(set_custom_prompt(&conn, "   ").unwrap(), None);
        assert_eq!(get_custom_prompt(&conn).unwrap(), None);
        assert!(set_custom_prompt(&conn, &"x".repeat(MAX_PROMPT_CHARS + 1)).is_err());
    }

    fn req<'a>(style: &'a StyleContext, quote: bool) -> GenRequest<'a> {
        GenRequest { quote, handle: "naval", post_text: "post", max_chars: 280, style, instruction: Instruction::Fresh, current: None, angle: None, model_hint: None, context: None, why_working: None }
    }

    #[test]
    fn custom_prompt_and_formats_reach_the_system_prompt_and_survive_overrides() {
        let style = StyleContext {
            reply_prompt: Some("Reply as Pavel: blunt, lowercase.".into()),
            reply_formats: vec![ReplyShape { name: "Counter then proof".into(), pattern: "Disagree with [claim], then give [one concrete example]".into(), when_to_use: "strong opinions".into() }],
            ..Default::default()
        };
        let (system, user) = build_prompt(&req(&style, false));
        assert!(system.starts_with("Reply as Pavel: blunt, lowercase."));
        assert!(system.contains("Counter then proof") && system.contains("[one concrete example]"));
        assert!(system.contains("never reuse wording") && system.contains("\"variants\""), "guardrails stay");
        assert!(!system.contains("You draft a reply"), "custom prompt replaces the built-in opening");
        assert!(!user.contains("Pavel") && !user.contains("Counter then proof"), "voice never travels with untrusted text");

        let slots = build_slots(&req(&style, false));
        assert!(slots.guard.contains("Reply as Pavel") && slots.guard.contains("Counter then proof"));
    }

    #[test]
    fn quote_posts_ignore_reply_voice_and_empty_context_is_unchanged() {
        let style = StyleContext { reply_prompt: Some("Reply as Pavel".into()), reply_formats: vec![ReplyShape { name: "F".into(), pattern: "P".into(), when_to_use: String::new() }], ..Default::default() };
        let (system, _) = build_prompt(&req(&style, true));
        assert!(system.starts_with("You draft a quote post") && !system.contains("Reply as Pavel"));
        let (plain, _) = build_prompt(&req(&StyleContext::default(), false));
        assert!(plain.starts_with("You draft a reply to a post on X in the user's own voice.") && !plain.contains("Reply formats that have worked"));
    }
}
