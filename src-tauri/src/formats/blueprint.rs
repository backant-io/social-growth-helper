//! Format blueprints (OGC-02): the structure of a viral post with its subject matter removed.
//! Model output is untrusted, so everything is parsed and validated here before it is stored.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_SLOTS: usize = 8;
/// A blueprint that repeats this many words in a row from its source is a copy, not a template.
const MAX_COPIED_RUN: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatBlueprint {
    pub name: String,
    pub archetype: String,
    pub hook_formula: String,
    pub structural_pattern: String,
    /// The post with every topic-specific part replaced by a `{slot}` placeholder.
    pub skeleton: String,
    pub slots: Vec<Slot>,
    #[serde(default = "default_media_hint")]
    pub media_hint: String,
}

fn default_media_hint() -> String {
    "text_only".into()
}

const MEDIA_HINTS: &[&str] = &["text_only", "screenshot", "image", "video", "meme_crop"];

pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Placeholder names used in a skeleton, in order of first appearance.
pub fn placeholders(skeleton: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = skeleton;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !found.iter().any(|f| f == name) {
            found.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    found
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Longest run of consecutive words that appears in both texts.
pub fn longest_shared_run(a: &str, b: &str) -> usize {
    let (wa, wb) = (words(a), words(b));
    let mut best = 0;
    let mut prev = vec![0usize; wb.len() + 1];
    for i in 1..=wa.len() {
        let mut cur = vec![0usize; wb.len() + 1];
        for j in 1..=wb.len() {
            if wa[i - 1] == wb[j - 1] {
                cur[j] = prev[j - 1] + 1;
                best = best.max(cur[j]);
            }
        }
        prev = cur;
    }
    best
}

impl FormatBlueprint {
    /// Normalises and checks a blueprint. `source_text` (when known) is used to reject copies.
    pub fn validated(mut self, source_text: Option<&str>) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        self.hook_formula = self.hook_formula.trim().to_string();
        self.structural_pattern = self.structural_pattern.trim().to_string();
        self.skeleton = self.skeleton.trim().to_string();
        self.archetype = match slugify(&self.archetype) {
            s if s.is_empty() => "other".to_string(),
            s => s.chars().take(40).collect(),
        };
        if !MEDIA_HINTS.contains(&self.media_hint.as_str()) {
            self.media_hint = default_media_hint();
        }

        if self.name.chars().count() < 3 || self.name.chars().count() > 80 {
            return Err("the format name must be 3 to 80 characters".into());
        }
        if self.hook_formula.is_empty() {
            return Err("the hook formula is empty".into());
        }
        if self.structural_pattern.is_empty() {
            return Err("the structural pattern is empty".into());
        }
        if self.skeleton.is_empty() || self.skeleton.chars().count() > 1_200 {
            return Err("the skeleton must be 1 to 1200 characters".into());
        }
        if self.slots.is_empty() || self.slots.len() > MAX_SLOTS {
            return Err(format!("a format needs 1 to {MAX_SLOTS} fill-in slots"));
        }

        let mut seen = BTreeSet::new();
        for slot in &mut self.slots {
            slot.key = slugify(&slot.key);
            slot.label = slot.label.trim().to_string();
            slot.hint = slot.hint.trim().to_string();
            if slot.key.is_empty() {
                return Err("a slot has no usable key".into());
            }
            if slot.label.is_empty() {
                slot.label = slot.key.replace('_', " ");
            }
            if !seen.insert(slot.key.clone()) {
                return Err(format!("slot \"{}\" is declared twice", slot.key));
            }
        }

        let used = placeholders(&self.skeleton);
        if used.is_empty() {
            return Err("the skeleton has no {slot} placeholders, so it is too generic to reuse".into());
        }
        for u in &used {
            if !seen.contains(u) {
                return Err(format!("the skeleton uses {{{u}}} but no slot with that key is declared"));
            }
        }
        for s in &seen {
            if !used.contains(s) {
                return Err(format!("slot \"{s}\" is never used in the skeleton"));
            }
        }

        if let Some(src) = source_text {
            let copied = longest_shared_run(&self.skeleton, src);
            if copied >= MAX_COPIED_RUN {
                return Err(format!("the skeleton repeats {copied} words in a row from the source post; it should hold structure only"));
            }
        }
        Ok(self)
    }

    /// Fills the skeleton with the user's slot values. Missing slots stay visible as `{key}`.
    pub fn fill(&self, values: &std::collections::BTreeMap<String, String>) -> String {
        let mut out = self.skeleton.clone();
        for slot in &self.slots {
            if let Some(v) = values.get(&slot.key).map(|v| v.trim()).filter(|v| !v.is_empty()) {
                out = out.replace(&format!("{{{}}}", slot.key), v);
            }
        }
        out
    }
}

/// Pulls the first balanced JSON object out of model output, tolerating code fences and chatter.
pub fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let bytes = raw.as_bytes();
    let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_blueprint(raw: &str, source_text: Option<&str>) -> Result<FormatBlueprint, String> {
    let json = extract_json_object(raw).ok_or("the model reply contained no JSON object")?;
    let bp: FormatBlueprint = serde_json::from_str(json).map_err(|e| format!("the blueprint JSON did not match the schema: {e}"))?;
    bp.validated(source_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    pub fn sample() -> FormatBlueprint {
        FormatBlueprint {
            name: "Unpopular truth with proof".into(),
            archetype: "Contrarian Take".into(),
            hook_formula: "State a belief most people hold, then say it is wrong.".into(),
            structural_pattern: "One line hook, blank line, three short proof lines, one-line twist.".into(),
            skeleton: "Unpopular opinion: {common_belief} is wrong.\n\n{proof_one}\n{proof_two}\n{proof_three}\n\n{twist}".into(),
            slots: ["common_belief", "proof_one", "proof_two", "proof_three", "twist"]
                .iter()
                .map(|k| Slot { key: k.to_string(), label: String::new(), hint: String::new() })
                .collect(),
            media_hint: "text_only".into(),
        }
    }

    #[test]
    fn a_good_blueprint_validates_and_normalises() {
        let bp = sample().validated(None).unwrap();
        assert_eq!(bp.archetype, "contrarian_take");
        assert_eq!(bp.slots[0].label, "common belief");
    }

    #[test]
    fn skeleton_without_placeholders_is_too_generic() {
        let mut bp = sample();
        bp.skeleton = "Here is a nice post.".into();
        assert!(bp.validated(None).unwrap_err().contains("too generic"));
    }

    #[test]
    fn undeclared_and_unused_slots_are_rejected() {
        let mut bp = sample();
        bp.skeleton.push_str(" {mystery}");
        assert!(bp.validated(None).unwrap_err().contains("mystery"));
        let mut bp = sample();
        bp.slots.push(Slot { key: "spare".into(), label: "".into(), hint: "".into() });
        assert!(bp.validated(None).unwrap_err().contains("never used"));
    }

    #[test]
    fn copying_the_source_is_rejected() {
        let mut bp = sample();
        bp.skeleton = "Nobody tells you that cold email beats ads every single time {twist}".into();
        bp.slots = vec![Slot { key: "twist".into(), label: "".into(), hint: "".into() }];
        let src = "Nobody tells you that cold email beats ads every single time for small SaaS.";
        assert!(bp.validated(Some(src)).unwrap_err().contains("in a row"));
    }

    #[test]
    fn fill_replaces_known_slots_and_leaves_gaps_visible() {
        let bp = sample().validated(None).unwrap();
        let mut v = BTreeMap::new();
        v.insert("common_belief".to_string(), "hustle culture".to_string());
        let out = bp.fill(&v);
        assert!(out.starts_with("Unpopular opinion: hustle culture is wrong."));
        assert!(out.contains("{twist}"));
    }

    #[test]
    fn json_is_found_inside_fences_and_chatter() {
        let raw = "Sure!\n```json\n{\"a\": \"has } brace\", \"b\": {\"c\": 1}}\n```\nDone.";
        assert_eq!(extract_json_object(raw).unwrap(), "{\"a\": \"has } brace\", \"b\": {\"c\": 1}}");
        assert!(extract_json_object("no json here").is_none());
    }

    #[test]
    fn parse_blueprint_round_trips_model_output() {
        let raw = format!("```json\n{}\n```", serde_json::to_string(&sample()).unwrap());
        assert_eq!(parse_blueprint(&raw, None).unwrap().name, "Unpopular truth with proof");
        assert!(parse_blueprint("{\"name\": 3}", None).is_err());
    }

    #[test]
    fn unknown_media_hint_falls_back_to_text_only() {
        let mut bp = sample();
        bp.media_hint = "hologram".into();
        assert_eq!(bp.validated(None).unwrap().media_hint, "text_only");
    }
}
