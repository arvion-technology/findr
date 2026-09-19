//! Log field values.
//!
//! Logs are loosely typed: a field can be a string in one line and a number in
//! the next. Records keep the JSON-ish type so numeric filters and percentile
//! aggregation can work, while display and export render a stable string.

use std::collections::BTreeMap;

/// One value extracted from a log line.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Numeric view, used by comparisons and aggregation.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// String view for equality, grouping and `contains`.
    pub fn render(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => format_number(*number),
            Value::Str(text) => text.clone(),
            Value::Array(_) | Value::Object(_) => self.to_json(),
        }
    }

    /// Compact JSON, used for `--format json` and nested values.
    pub fn to_json(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => format_number(*number),
            Value::Str(text) => quote(text),
            Value::Array(items) => {
                let inner: Vec<String> = items.iter().map(Value::to_json).collect();
                format!("[{}]", inner.join(","))
            }
            Value::Object(entries) => {
                let inner: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| format!("{}:{}", quote(key), value.to_json()))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }
}

/// A log record is a flat map. Nested JSON objects are flattened with dotted
/// keys (`http.status_code`), which is what makes `--field http.status_code`
/// work the way an operator expects.
pub type Record = BTreeMap<String, Value>;

/// Format a float without a pointless trailing `.0`.
pub fn format_number(number: f64) -> String {
    if number.is_finite() && number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        let mut text = format!("{number}");
        if text.contains('e') {
            text = format!("{number:.6}");
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        text
    }
}

/// Escape a string as a JSON string literal.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Serialize a record as compact JSON with sorted keys.
pub fn record_to_json(record: &Record) -> String {
    let inner: Vec<String> = record
        .iter()
        .map(|(key, value)| format!("{}:{}", quote(key), value.to_json()))
        .collect();
    format!("{{{}}}", inner.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_display_without_trailing_zero() {
        assert_eq!(Value::Number(42.0).render(), "42");
        assert_eq!(Value::Number(42.5).render(), "42.5");
        assert_eq!(format_number(1.0), "1");
    }

    #[test]
    fn strings_are_quoted_for_json_only() {
        assert_eq!(Value::Str("a \"b\"\n".to_string()).render(), "a \"b\"\n");
        assert_eq!(
            Value::Str("a \"b\"\n".to_string()).to_json(),
            "\"a \\\"b\\\"\\n\""
        );
    }

    #[test]
    fn nested_values_serialize_as_json() {
        let value = Value::Array(vec![Value::Number(1.0), Value::Str("x".to_string())]);
        assert_eq!(value.to_json(), "[1,\"x\"]");
    }

    #[test]
    fn records_serialize_with_sorted_keys() {
        let mut record = Record::new();
        record.insert("b".to_string(), Value::Number(2.0));
        record.insert("a".to_string(), Value::Str("one".to_string()));
        assert_eq!(record_to_json(&record), "{\"a\":\"one\",\"b\":2}");
    }
}
