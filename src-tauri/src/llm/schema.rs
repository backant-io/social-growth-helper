//! JSON extraction and Draft-07 subset JSON Schema validation (LLM-13).

use serde_json::Value;

/// Extracts a JSON value from raw model output, tolerating markdown code fences or stray wrapper prose.
pub fn extract_json_value(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("model returned an empty response".into());
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }

    // Strip ```json ... ``` or ``` ... ``` fences if present.
    if let Some(fenced) = extract_fenced_block(trimmed) {
        if let Ok(v) = serde_json::from_str::<Value>(fenced.trim()) {
            return Ok(v);
        }
    }

    // Locate balanced top-level `{...}` or `[...]`.
    if let Some(slice) = extract_balanced_json_slice(trimmed) {
        if let Ok(v) = serde_json::from_str::<Value>(slice) {
            return Ok(v);
        }
    }

    Err("response did not contain valid JSON".into())
}

fn extract_fenced_block(s: &str) -> Option<&str> {
    let start = s.find("```")?;
    let after_open = &s[start + 3..];
    let body_start = after_open.find('\n').map(|i| i + 1).unwrap_or(0);
    let rest = &after_open[body_start..];
    let end = rest.rfind("```")?;
    Some(&rest[..end])
}

fn extract_balanced_json_slice(s: &str) -> Option<&str> {
    let obj_idx = s.find('{');
    let arr_idx = s.find('[');
    let (start, open, close) = match (obj_idx, arr_idx) {
        (Some(o), Some(a)) if a < o => (a, '[', ']'),
        (Some(o), _) => (o, '{', '}'),
        (None, Some(a)) => (a, '[', ']'),
        (None, None) => return None,
    };

    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (rel_idx, ch) in s[start..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if ch == '"' {
            in_str = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(&s[start..=start + rel_idx]);
            }
        }
    }
    None
}

/// Validates a JSON value against a JSON Schema subset used across all structured LLM tasks.
pub fn validate_json_schema(schema: &Value, value: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    validate_at("$", schema, value, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_at(path: &str, schema: &Value, value: &Value, errors: &mut Vec<String>) {
    let Some(obj) = schema.as_object() else {
        return;
    };

    if let Some(any_of) = obj.get("anyOf").and_then(Value::as_array) {
        let matched = any_of.iter().any(|sub| {
            let mut sub_err = Vec::new();
            validate_at(path, sub, value, &mut sub_err);
            sub_err.is_empty()
        });
        if !matched {
            errors.push(format!("{path}: did not match anyOf schema options"));
            return;
        }
    }

    if let Some(enum_vals) = obj.get("enum").and_then(Value::as_array) {
        if !enum_vals.iter().any(|e| e == value) {
            errors.push(format!("{path}: value is not one of allowed enum options"));
        }
    }

    if let Some(ty) = obj.get("type") {
        if !matches_type(ty, value) {
            errors.push(format!("{path}: expected type {ty}, got {}", json_type_name(value)));
            return;
        }
    }

    match value {
        Value::Object(map) => {
            if let Some(req) = obj.get("required").and_then(Value::as_array) {
                for r in req {
                    if let Some(k) = r.as_str() {
                        if !map.contains_key(k) {
                            errors.push(format!("{path}.{k}: missing required field"));
                        }
                    }
                }
            }
            let props = obj.get("properties").and_then(Value::as_object);
            let allow_extra = obj.get("additionalProperties").and_then(Value::as_bool).unwrap_or(true);
            for (k, v) in map {
                if let Some(prop_schema) = props.and_then(|p| p.get(k)) {
                    validate_at(&format!("{path}.{k}"), prop_schema, v, errors);
                } else if !allow_extra {
                    errors.push(format!("{path}.{k}: unexpected property"));
                }
            }
        }
        Value::Array(items) => {
            if let Some(min_i) = obj.get("minItems").and_then(Value::as_u64) {
                if (items.len() as u64) < min_i {
                    errors.push(format!("{path}: expected at least {min_i} item(s), got {}", items.len()));
                }
            }
            if let Some(max_i) = obj.get("maxItems").and_then(Value::as_u64) {
                if (items.len() as u64) > max_i {
                    errors.push(format!("{path}: expected at most {max_i} item(s), got {}", items.len()));
                }
            }
            if let Some(item_schema) = obj.get("items") {
                for (i, item) in items.iter().enumerate() {
                    validate_at(&format!("{path}[{i}]"), item_schema, item, errors);
                }
            }
        }
        Value::String(s) => {
            let len = s.chars().count() as u64;
            if let Some(min_l) = obj.get("minLength").and_then(Value::as_u64) {
                if len < min_l {
                    errors.push(format!("{path}: string shorter than minLength {min_l}"));
                }
            }
            if let Some(max_l) = obj.get("maxLength").and_then(Value::as_u64) {
                if len > max_l {
                    errors.push(format!("{path}: string longer than maxLength {max_l}"));
                }
            }
        }
        Value::Number(n) => {
            if let Some(num) = n.as_f64() {
                if let Some(min_v) = obj.get("minimum").and_then(Value::as_f64) {
                    if num < min_v {
                        errors.push(format!("{path}: {num} < minimum {min_v}"));
                    }
                }
                if let Some(max_v) = obj.get("maximum").and_then(Value::as_f64) {
                    if num > max_v {
                        errors.push(format!("{path}: {num} > maximum {max_v}"));
                    }
                }
            }
        }
        _ => {}
    }
}

fn matches_type(ty: &Value, value: &Value) -> bool {
    match ty {
        Value::String(s) => matches_single_type(s, value),
        Value::Array(arr) => arr.iter().filter_map(Value::as_str).any(|s| matches_single_type(s, value)),
        _ => true,
    }
}

fn matches_single_type(ty: &str, value: &Value) -> bool {
    match ty {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_raw_fenced_and_prose_wrapped_json() {
        assert_eq!(extract_json_value(r#"{"ok":true}"#).unwrap(), json!({"ok": true}));
        assert_eq!(
            extract_json_value("Here is the JSON:\n```json\n{\"score\": 88}\n```\nDone.").unwrap(),
            json!({"score": 88})
        );
        assert_eq!(
            extract_json_value("Result: [1, 2, 3] thanks").unwrap(),
            json!([1, 2, 3])
        );
        assert!(extract_json_value("not json at all").is_err());
    }

    #[test]
    fn validates_nested_object_schema_and_reports_paths() {
        let schema = json!({
            "type": "object",
            "required": ["variants"],
            "additionalProperties": false,
            "properties": {
                "variants": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "required": ["text", "tone_tag"],
                        "additionalProperties": false,
                        "properties": {
                            "text": { "type": "string", "minLength": 2 },
                            "tone_tag": { "type": "string" }
                        }
                    }
                }
            }
        });
        assert!(validate_json_schema(&schema, &json!({
            "variants": [{ "text": "Clean take.", "tone_tag": "witty" }]
        }))
        .is_ok());

        let errs = validate_json_schema(
            &schema,
            &json!({ "variants": [{ "text": "" }], "extra": 1 }),
        )
        .unwrap_err();
        assert!(errs.iter().any(|e| e.contains("tone_tag")));
        assert!(errs.iter().any(|e| e.contains("minLength")));
        assert!(errs.iter().any(|e| e.contains("extra")));
    }
}
