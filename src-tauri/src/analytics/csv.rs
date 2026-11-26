//! RFC 4180 CSV serialization for dashboard tables and widgets (ANL-13).

use super::sandbox::WidgetQueryResult;

/// Escapes a CSV cell and neutralizes spreadsheet formula injection (`=`, `+`, `-`, `@`).
fn escape_csv_cell(raw: &str) -> String {
    let safe = if let Some(first) = raw.chars().next() {
        if matches!(first, '=' | '+' | '@' | '\t' | '\r')
            || (first == '-' && raw.len() > 1 && !raw[1..].chars().all(|c| c.is_ascii_digit() || c == '.'))
        {
            format!("'{raw}")
        } else {
            raw.to_string()
        }
    } else {
        String::new()
    };

    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

fn value_to_cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn to_csv(result: &WidgetQueryResult) -> String {
    let mut lines = Vec::with_capacity(result.rows.len() + 1);
    let header: Vec<String> = result.columns.iter().map(|c| escape_csv_cell(c)).collect();
    lines.push(header.join(","));

    for row in &result.rows {
        let cells: Vec<String> = result
            .columns
            .iter()
            .map(|c| {
                let raw = row.get(c).map(value_to_cell).unwrap_or_default();
                escape_csv_cell(&raw)
            })
            .collect();
        lines.push(cells.join(","));
    }
    lines.join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn quotes_commas_newlines_and_neutralizes_formulas() {
        let mut r1 = BTreeMap::new();
        r1.insert("text".into(), serde_json::json!("Hello, \"world\"\nnext"));
        r1.insert("formula".into(), serde_json::json!("=CMD()"));
        r1.insert("delta".into(), serde_json::json!(-12));

        let res = WidgetQueryResult {
            columns: vec!["text".into(), "formula".into(), "delta".into()],
            rows: vec![r1],
            row_count: 1,
            truncated: false,
            window_days: 30,
            executed_view: "v_analytics_post_performance".into(),
        };
        let csv = to_csv(&res);
        assert!(csv.starts_with("text,formula,delta\r\n"));
        assert!(csv.contains("\"Hello, \"\"world\"\"\nnext\""));
        assert!(csv.contains("'=CMD()"));
        assert!(csv.ends_with(",-12"));
    }
}
