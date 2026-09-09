//! Log line parsing.
//!
//! Two formats are supported, and detection needs no configuration:
//!
//! * JSON lines, when the trimmed line starts with `{`.
//! * key=value logfmt, when at least one `key=value` pair is present.
//! * anything else becomes a plain record with a single `message` field.
//!
//! Malformed input is an error rather than a silently empty record: the caller
//! decides whether to warn and skip, or stop with `--strict`.

use crate::json;
use crate::value::{Record, Value};

/// Detected format of a parsed line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    KeyValue,
    Plain,
}

/// One parsed log line plus enough metadata to report where it came from.
#[derive(Clone, Debug)]
pub struct ParsedLine {
    pub record: Record,
    pub raw: String,
    pub line_no: usize,
    pub format: Format,
}

/// Parse a single line. Empty lines are rejected; the reader skips them first.
pub fn parse_line(line: &str, line_no: usize) -> Result<ParsedLine, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty line".to_string());
    }

    if trimmed.starts_with('{') {
        let entries = json::parse_object(trimmed)?;
        let mut record = Record::new();
        for (key, value) in entries {
            flatten_into(&key, &value, &mut record);
        }
        return Ok(ParsedLine {
            record,
            raw: line.trim_end_matches('\r').to_string(),
            line_no,
            format: Format::Json,
        });
    }

    let (record, is_pairs) = parse_logfmt(trimmed);
    Ok(ParsedLine {
        record,
        raw: line.trim_end_matches('\r').to_string(),
        line_no,
        format: if is_pairs {
            Format::KeyValue
        } else {
            Format::Plain
        },
    })
}

/// Flatten nested JSON objects into dotted keys. Arrays stay as arrays.
fn flatten_into(prefix: &str, value: &Value, record: &mut Record) {
    match value {
        Value::Object(entries) => {
            for (key, child) in entries {
                let next = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(&next, child, record);
            }
        }
        other => {
            record.insert(prefix.to_string(), other.clone());
        }
    }
}

/// Parse a logfmt line. Returns the record and whether any pair was found.
fn parse_logfmt(line: &str) -> (Record, bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    let mut record = Record::new();
    let mut loose: Vec<String> = Vec::new();
    let mut pairs = 0;

    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }

        if chars[index] == '"' {
            // A quoted token outside a pair is part of the plain message.
            let (text, next) = read_quoted(&chars, index);
            loose.push(text);
            index = next;
            continue;
        }

        let key_start = index;
        while index < chars.len() && !chars[index].is_whitespace() && chars[index] != '=' {
            index += 1;
        }
        let key: String = chars[key_start..index].iter().collect();

        if index < chars.len() && chars[index] == '=' {
            index += 1;
            let (raw, next) = if index < chars.len() && chars[index] == '"' {
                read_quoted(&chars, index)
            } else {
                let value_start = index;
                while index < chars.len() && !chars[index].is_whitespace() {
                    index += 1;
                }
                (chars[value_start..index].iter().collect::<String>(), index)
            };
            index = next;
            if !key.is_empty() {
                record.insert(key, typed_value(&raw));
                pairs += 1;
            } else {
                loose.push(raw);
            }
        } else if !key.is_empty() {
            loose.push(key);
        }
    }

    if !loose.is_empty() {
        record.insert("message".to_string(), Value::Str(loose.join(" ")));
    } else if pairs == 0 {
        record.insert("message".to_string(), Value::Str(line.to_string()));
    }

    (record, pairs > 0)
}

fn read_quoted(chars: &[char], start: usize) -> (String, usize) {
    let mut out = String::new();
    let mut index = start + 1;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' && index + 1 < chars.len() {
            let escaped = chars[index + 1];
            out.push(match escaped {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            index += 2;
            continue;
        }
        if ch == '"' {
            return (out, index + 1);
        }
        out.push(ch);
        index += 1;
    }
    (out, index)
}

/// Turn a logfmt value into a typed value. Only unambiguous scalars are
/// converted; everything else stays a string so an id like `007` keeps its
/// leading zeros.
fn typed_value(raw: &str) -> Value {
    if let Ok(number) = raw.parse::<f64>() {
        if raw == number.to_string()
            && raw
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '-')
        {
            return Value::Number(number);
        }
    }
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "" => Value::Null,
        _ => Value::Str(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_lines_and_flattens_nested_objects() {
        let parsed = parse_line(
            r#"{"ts":"2026-01-01T10:00:00Z","level":"info","http":{"status":200,"path":"/a"}}"#,
            4,
        )
        .unwrap();
        assert_eq!(parsed.format, Format::Json);
        assert_eq!(parsed.line_no, 4);
        assert_eq!(
            parsed.record.get("http.status").and_then(Value::as_number),
            Some(200.0)
        );
        assert_eq!(parsed.record.get("level").unwrap().render(), "info");
    }

    #[test]
    fn parses_logfmt_with_quoted_values_and_numbers() {
        let parsed = parse_line(
            r#"ts=2026-01-01T10:00:00Z level=warn status=503 latency=217.5 msg="pool exhausted""#,
            1,
        )
        .unwrap();
        assert_eq!(parsed.format, Format::KeyValue);
        assert_eq!(
            parsed.record.get("status").and_then(Value::as_number),
            Some(503.0)
        );
        assert_eq!(
            parsed.record.get("latency").and_then(Value::as_number),
            Some(217.5)
        );
        assert_eq!(parsed.record.get("msg").unwrap().render(), "pool exhausted");
    }

    #[test]
    fn quoted_values_can_contain_spaces_and_escapes() {
        let parsed = parse_line(r#"msg="retrying \"upload\" now" attempt=2"#, 2).unwrap();
        assert_eq!(
            parsed.record.get("msg").unwrap().render(),
            "retrying \"upload\" now"
        );
        assert_eq!(
            parsed.record.get("attempt").and_then(Value::as_number),
            Some(2.0)
        );
    }

    #[test]
    fn plain_lines_become_a_message_record() {
        let parsed = parse_line("starting shutdown sequence", 3).unwrap();
        assert_eq!(parsed.format, Format::Plain);
        assert_eq!(
            parsed.record.get("message").unwrap().render(),
            "starting shutdown sequence"
        );
    }

    #[test]
    fn logfmt_can_mix_a_timestamp_prefix_with_pairs() {
        let parsed = parse_line("2026-01-01T10:00:00Z level=info request=abc", 5).unwrap();
        assert_eq!(parsed.format, Format::KeyValue);
        assert_eq!(parsed.record.get("level").unwrap().render(), "info");
        assert_eq!(
            parsed.record.get("message").unwrap().render(),
            "2026-01-01T10:00:00Z"
        );
    }

    #[test]
    fn malformed_json_is_an_error_not_a_plain_line() {
        assert!(parse_line(r#"{"level":"info""#, 1).is_err());
        assert!(parse_line(r#"{"level":}"#, 1).is_err());
    }

    #[test]
    fn empty_lines_are_rejected() {
        assert!(parse_line("   ", 1).is_err());
    }

    #[test]
    fn leading_zero_ids_stay_strings() {
        let parsed = parse_line("order=007 status=200", 1).unwrap();
        assert_eq!(parsed.record.get("order").unwrap().render(), "007");
    }
}
