//! Model catalogue per provider, live API refresh, and custom model ID support (LLM-16).

use super::{ProviderCapabilities, ProviderType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    /// Wire names match the UI's `ModelInfo` (`display_name`, `is_custom`).
    #[serde(rename = "display_name", alias = "label")]
    pub label: String,
    pub capabilities: ProviderCapabilities,
    #[serde(rename = "is_custom", alias = "custom")]
    pub custom: bool,
}

pub fn default_models(provider_type: ProviderType) -> Vec<ModelInfo> {
    match provider_type {
        ProviderType::ClaudeCli => vec![
            ModelInfo {
                id: "sonnet".into(),
                label: "Claude Sonnet (CLI alias)".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "opus".into(),
                label: "Claude Opus (CLI alias)".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "haiku".into(),
                label: "Claude Haiku (CLI alias)".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: false },
                custom: false,
            },
        ],
        ProviderType::AnthropicApi => vec![
            ModelInfo {
                id: "claude-sonnet-4-5".into(),
                label: "Claude Sonnet 4.5".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "claude-opus-4-5".into(),
                label: "Claude Opus 4.5".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "claude-haiku-4-5".into(),
                label: "Claude Haiku 4.5".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: false },
                custom: false,
            },
        ],
        ProviderType::XaiOAuth | ProviderType::XaiApi => vec![
            ModelInfo {
                id: "grok-4.6".into(),
                label: "Grok 4.6".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "grok-4.3".into(),
                label: "Grok 4.3".into(),
                capabilities: ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
            ModelInfo {
                id: "grok-3-mini".into(),
                label: "Grok 3 Mini".into(),
                capabilities: ProviderCapabilities { vision: false, json_schema: true, streaming: true, thinking: true },
                custom: false,
            },
        ],
        ProviderType::GrokWeb => vec![
            ModelInfo {
                id: "grok-web".into(),
                label: "Grok Web Tab (x.com/i/grok)".into(),
                capabilities: ProviderCapabilities { vision: false, json_schema: false, streaming: false, thinking: true },
                custom: false,
            },
        ],
    }
}

pub fn merge_catalog(
    provider_type: ProviderType,
    remote_ids: &[String],
    custom_ids: &[String],
) -> Vec<ModelInfo> {
    let mut out = default_models(provider_type);
    let mut seen: HashSet<String> = out.iter().map(|m| m.id.clone()).collect();

    for id in remote_ids {
        let trimmed = id.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        out.push(ModelInfo {
            id: trimmed.to_string(),
            label: trimmed.to_string(),
            capabilities: infer_capabilities(provider_type, trimmed),
            custom: false,
        });
    }

    for id in custom_ids {
        let trimmed = id.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        out.push(ModelInfo {
            id: trimmed.to_string(),
            label: format!("{trimmed} (custom)"),
            capabilities: infer_capabilities(provider_type, trimmed),
            custom: true,
        });
    }

    out
}

pub fn infer_capabilities(provider_type: ProviderType, model_id: &str) -> ProviderCapabilities {
    let lower = model_id.to_lowercase();
    match provider_type {
        ProviderType::ClaudeCli => ProviderCapabilities {
            vision: true,
            json_schema: true,
            streaming: true,
            thinking: !lower.contains("haiku"),
        },
        ProviderType::AnthropicApi => ProviderCapabilities {
            vision: true,
            json_schema: true,
            streaming: true,
            thinking: !lower.contains("haiku"),
        },
        ProviderType::XaiOAuth | ProviderType::XaiApi => ProviderCapabilities {
            vision: !lower.contains("mini") && !lower.contains("text"),
            json_schema: true,
            streaming: true,
            thinking: lower.contains("grok-4") || lower.contains("reasoning") || lower.contains("mini"),
        },
        ProviderType::GrokWeb => ProviderCapabilities {
            vision: false,
            json_schema: false,
            streaming: false,
            thinking: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_remote_and_custom_models_without_duplicates() {
        let models = merge_catalog(
            ProviderType::XaiApi,
            &["grok-4.6".into(), "grok-4.7-preview".into()],
            &["grok-custom-test".into(), "grok-4.7-preview".into()],
        );
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "grok-4.6").count(), 1);
        assert!(ids.contains(&"grok-4.7-preview"));
        assert!(ids.contains(&"grok-custom-test"));
        assert!(models.iter().find(|m| m.id == "grok-custom-test").unwrap().custom);
    }

    #[test]
    fn serializes_with_the_field_names_the_ui_reads() {
        for pt in [ProviderType::ClaudeCli, ProviderType::AnthropicApi, ProviderType::XaiApi, ProviderType::XaiOAuth, ProviderType::GrokWeb] {
            for m in merge_catalog(pt, &["remote-model".into()], &["custom-model".into()]) {
                let v = serde_json::to_value(&m).unwrap();
                let name = v["display_name"].as_str().unwrap_or_default();
                assert!(!name.trim().is_empty(), "{pt:?} {} has no display_name", m.id);
                assert!(v["is_custom"].is_boolean());
                assert!(v.get("label").is_none());
            }
        }
    }
}
