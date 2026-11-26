//! Style Studio domain: versioned style guides, FTS5-indexed few-shot examples,
//! candidate import (paste or read-only browser), and Style Test Bench (STY-01..STY-06, STY-09).

pub mod bench;
pub mod repo;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmojiPolicy {
    None,
    Minimal,
    Allowed,
}

impl EmojiPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Allowed => "allowed",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "allowed" => Ok(Self::Allowed),
            other => Err(format!("invalid emoji_policy '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashtagPolicy {
    None,
    Max1,
    Allowed,
}

impl HashtagPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Max1 => "max_1",
            Self::Allowed => "allowed",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "max_1" => Ok(Self::Max1),
            "allowed" => Ok(Self::Allowed),
            other => Err(format!("invalid hashtag_policy '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExampleKind {
    Reply,
    Ogc,
}

impl ExampleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Ogc => "ogc",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "reply" => Ok(Self::Reply),
            "ogc" => Ok(Self::Ogc),
            other => Err(format!("invalid example kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleGuide {
    pub id: String,
    pub profile_id: String,
    pub version: u32,
    pub free_text_guide: String,
    pub learned_rules: Vec<String>,
    pub banned_patterns: Vec<String>,
    pub max_reply_chars: u32,
    pub emoji_policy: EmojiPolicy,
    pub hashtag_policy: HashtagPolicy,
    pub note: Option<String>,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleGuideInput {
    pub free_text_guide: String,
    #[serde(default)]
    pub learned_rules: Vec<String>,
    #[serde(default)]
    pub banned_patterns: Vec<String>,
    #[serde(default = "default_max_reply_chars")]
    pub max_reply_chars: u32,
    #[serde(default = "default_emoji_policy")]
    pub emoji_policy: EmojiPolicy,
    #[serde(default = "default_hashtag_policy")]
    pub hashtag_policy: HashtagPolicy,
    pub note: Option<String>,
}

fn default_max_reply_chars() -> u32 {
    220
}
fn default_emoji_policy() -> EmojiPolicy {
    EmojiPolicy::Minimal
}
fn default_hashtag_policy() -> HashtagPolicy {
    HashtagPolicy::None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleExample {
    pub id: String,
    pub profile_id: String,
    pub kind: ExampleKind,
    pub source_context_text: Option<String>,
    pub example_text: String,
    pub why_it_works: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub source: String,
    pub performance_score: Option<f64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleExampleInput {
    pub id: Option<String>,
    pub kind: ExampleKind,
    pub source_context_text: Option<String>,
    pub example_text: String,
    pub why_it_works: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    pub source: Option<String>,
    pub performance_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedExample {
    pub example: StyleExample,
    pub reason: String,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCandidate {
    pub kind: ExampleKind,
    pub source_context_text: Option<String>,
    pub example_text: String,
    pub tags: Vec<String>,
    pub source: String,
    pub author_handle: Option<String>,
    pub external_id: Option<String>,
}
