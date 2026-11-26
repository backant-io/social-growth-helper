//! The six engagement actions and the rules for combining them in one approved bundle (DRF-06).

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Like,
    Reply,
    Repost,
    Quote,
    Bookmark,
    Follow,
}

/// Fixed execution order: cheap state toggles first, the one text action last (architecture plan, section 8).
pub const EXEC_ORDER: [ActionKind; 6] =
    [ActionKind::Like, ActionKind::Bookmark, ActionKind::Follow, ActionKind::Repost, ActionKind::Reply, ActionKind::Quote];

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Like => "like",
            ActionKind::Reply => "reply",
            ActionKind::Repost => "repost",
            ActionKind::Quote => "quote",
            ActionKind::Bookmark => "bookmark",
            ActionKind::Follow => "follow",
        }
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        EXEC_ORDER
            .into_iter()
            .find(|a| a.as_str() == s)
            .ok_or_else(|| AppError::Validation(format!("unknown action \"{s}\"")))
    }

    /// Reply and Quote carry user-written text and therefore always run through Publish (DRF-27).
    pub fn is_text(self) -> bool {
        matches!(self, ActionKind::Reply | ActionKind::Quote)
    }
}

/// Validates a requested bundle: known actions, no duplicates, at least one, at most one text action.
/// Returns the actions in execution order.
pub fn normalize(actions: &[String]) -> AppResult<Vec<ActionKind>> {
    let mut parsed = Vec::new();
    for a in actions {
        let k = ActionKind::parse(a)?;
        if !parsed.contains(&k) {
            parsed.push(k);
        }
    }
    if parsed.is_empty() {
        return Err(AppError::Validation("choose at least one action".into()));
    }
    if parsed.iter().filter(|a| a.is_text()).count() > 1 {
        return Err(AppError::Validation("a bundle can hold one text action: Reply or Quote, not both".into()));
    }
    Ok(EXEC_ORDER.into_iter().filter(|a| parsed.contains(a)).collect())
}

pub fn draft_kind(actions: &[ActionKind]) -> &'static str {
    if actions.contains(&ActionKind::Reply) {
        "reply"
    } else if actions.contains(&ActionKind::Quote) {
        "quote"
    } else {
        "actions_only"
    }
}

pub fn text_action(actions: &[ActionKind]) -> Option<ActionKind> {
    actions.iter().copied().find(|a| a.is_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn default_bundle_is_like_and_reply() {
        let b = normalize(&s(&["reply", "like"])).unwrap();
        assert_eq!(b, vec![ActionKind::Like, ActionKind::Reply]);
        assert_eq!(draft_kind(&b), "reply");
    }

    #[test]
    fn orders_actions_with_text_last() {
        let b = normalize(&s(&["quote", "follow", "repost", "bookmark", "like"])).unwrap();
        assert_eq!(b, vec![ActionKind::Like, ActionKind::Bookmark, ActionKind::Follow, ActionKind::Repost, ActionKind::Quote]);
        assert_eq!(draft_kind(&b), "quote");
    }

    #[test]
    fn rejects_two_text_actions_empty_and_unknown() {
        assert!(normalize(&s(&["reply", "quote"])).is_err());
        assert!(normalize(&[]).is_err());
        assert!(normalize(&s(&["like", "dm"])).is_err());
    }

    #[test]
    fn standalone_no_text_bundles_are_allowed() {
        let b = normalize(&s(&["like", "like"])).unwrap();
        assert_eq!(b, vec![ActionKind::Like]);
        assert_eq!(draft_kind(&b), "actions_only");
        assert!(text_action(&b).is_none());
    }
}
