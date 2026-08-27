//! Filter expressions.
//!
//! Grammar, one condition per comma separated clause:
//!
//! ```text
//! field=value          field!=value
//! field contains text  field>10   field>=10   field<10   field<=10
//! ```
//!
//! Multiple `--expr` flags and comma separated clauses combine with AND.
//! Numeric comparisons only match records where the field is numeric; a
//! missing field never satisfies a comparison. Equality compares numbers
//! numerically and everything else by its rendered string.

use crate::value::{Record, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Compare {
    Equal(String),
    NotEqual(String),
    Contains(String),
    Greater(f64),
    GreaterOrEqual(f64),
    Less(f64),
    LessOrEqual(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub field: String,
    pub compare: Compare,
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    conditions: Vec<Condition>,
}

impl Filter {
    /// Parse a comma separated expression.
    pub fn parse(expression: &str) -> Result<Self, String> {
        let mut conditions = Vec::new();
        for clause in split_clauses(expression) {
            conditions.push(Condition::parse(clause)?);
        }
        if conditions.is_empty() {
            return Err("empty filter expression".to_string());
        }
        Ok(Self { conditions })
    }

    /// Combine repeated expressions with AND.
    pub fn parse_all(expressions: &[String]) -> Result<Self, String> {
        let mut filter = Filter::default();
        for expression in expressions {
            for clause in split_clauses(expression) {
                filter.conditions.push(Condition::parse(clause)?);
            }
        }
        Ok(filter)
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn matches(&self, record: &Record) -> bool {
        self.conditions
            .iter()
            .all(|condition| condition.matches(record))
    }
}

impl Condition {
    pub fn parse(clause: &str) -> Result<Self, String> {
        let clause = clause.trim();
        if clause.is_empty() {
            return Err("empty condition".to_string());
        }

        let lower = clause.to_lowercase();
        if let Some(index) = lower.find(" contains ") {
            let field = clause[..index].trim().to_string();
            let needle = unquote(clause[index + " contains ".len()..].trim());
            require_field(&field)?;
            if needle.is_empty() {
                return Err(format!("contains condition on '{field}' needs a value"));
            }
            return Ok(Self {
                field,
                compare: Compare::Contains(needle.to_lowercase()),
            });
        }

        for (operator, kind) in [
            (">=", "ge"),
            ("<=", "le"),
            ("!=", "ne"),
            (">", "gt"),
            ("<", "lt"),
            ("=", "eq"),
        ] {
            if let Some(index) = clause.find(operator) {
                let field = clause[..index].trim().to_string();
                require_field(&field)?;
                let raw = unquote(clause[index + operator.len()..].trim());
                let compare = match kind {
                    "eq" => Compare::Equal(raw),
                    "ne" => Compare::NotEqual(raw),
                    _ => {
                        let number = raw.parse::<f64>().map_err(|_| {
                            format!("'{raw}' is not a number, required after '{operator}'")
                        })?;
                        match kind {
                            "ge" => Compare::GreaterOrEqual(number),
                            "le" => Compare::LessOrEqual(number),
                            "gt" => Compare::Greater(number),
                            "lt" => Compare::Less(number),
                            _ => unreachable!(),
                        }
                    }
                };
                return Ok(Self { field, compare });
            }
        }

        Err(format!(
            "cannot parse condition '{clause}': expected field=value, field contains text or a numeric comparison"
        ))
    }

    pub fn matches(&self, record: &Record) -> bool {
        let value = record.get(&self.field);
        match (&self.compare, value) {
            (Compare::Equal(wanted), Some(actual)) => equal(actual, wanted),
            (Compare::Equal(_), None) => false,
            (Compare::NotEqual(wanted), Some(actual)) => !equal(actual, wanted),
            (Compare::NotEqual(_), None) => true,
            (Compare::Contains(needle), Some(actual)) => {
                actual.render().to_lowercase().contains(needle)
            }
            (Compare::Contains(_), None) => false,
            (Compare::Greater(wanted), Some(actual)) => {
                numeric(actual).is_some_and(|number| number > *wanted)
            }
            (Compare::GreaterOrEqual(wanted), Some(actual)) => {
                numeric(actual).is_some_and(|number| number >= *wanted)
            }
            (Compare::Less(wanted), Some(actual)) => {
                numeric(actual).is_some_and(|number| number < *wanted)
            }
            (Compare::LessOrEqual(wanted), Some(actual)) => {
                numeric(actual).is_some_and(|number| number <= *wanted)
            }
            (Compare::Greater(_), None)
            | (Compare::GreaterOrEqual(_), None)
            | (Compare::Less(_), None)
            | (Compare::LessOrEqual(_), None) => false,
        }
    }
}

fn numeric(value: &Value) -> Option<f64> {
    value.as_number()
}

fn equal(actual: &Value, wanted: &str) -> bool {
    if let (Some(number), Ok(wanted_number)) = (actual.as_number(), wanted.parse::<f64>()) {
        return (number - wanted_number).abs() < f64::EPSILON;
    }
    actual.render() == wanted
}

fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn require_field(field: &str) -> Result<(), String> {
    if field.is_empty() || field.chars().any(|ch| ch.is_whitespace()) {
        return Err(format!("invalid field name '{field}'"));
    }
    Ok(())
}

fn split_clauses(expression: &str) -> Vec<&str> {
    expression
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn record(entries: &[(&str, Value)]) -> Record {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    fn sample() -> Record {
        record(&[
            ("level", Value::Str("error".into())),
            ("status", Value::Number(503.0)),
            ("latency", Value::Number(217.5)),
            ("msg", Value::Str("pool exhausted".into())),
        ])
    }

    #[test]
    fn parses_equality_and_matches_numbers_and_strings() {
        let filter = Filter::parse("status=503").unwrap();
        assert!(filter.matches(&sample()));
        assert!(!filter.matches(&record(&[("status", Value::Number(200.0))])));

        let text = Filter::parse("level=error").unwrap();
        assert!(text.matches(&sample()));
    }

    #[test]
    fn not_equal_treats_a_missing_field_as_not_equal() {
        let filter = Filter::parse("trace_id!=abc").unwrap();
        assert!(filter.matches(&sample()));
        let present = record(&[("trace_id", Value::Str("abc".into()))]);
        assert!(!filter.matches(&present));
    }

    #[test]
    fn contains_is_case_insensitive() {
        let filter = Filter::parse("msg contains POOL").unwrap();
        assert!(filter.matches(&sample()));
        let missing = Filter::parse("msg contains disk").unwrap();
        assert!(!missing.matches(&sample()));
    }

    #[test]
    fn numeric_comparisons_only_match_numeric_fields() {
        assert!(Filter::parse("latency>200").unwrap().matches(&sample()));
        assert!(Filter::parse("latency>=217.5").unwrap().matches(&sample()));
        assert!(Filter::parse("latency<300").unwrap().matches(&sample()));
        assert!(Filter::parse("latency<=217.5").unwrap().matches(&sample()));
        assert!(!Filter::parse("level<300").unwrap().matches(&sample()));
    }

    #[test]
    fn repeated_and_comma_clauses_combine_with_and() {
        let filter = Filter::parse_all(&[
            "status>=500".to_string(),
            "latency>200, level=error".to_string(),
        ])
        .unwrap();
        assert!(filter.matches(&sample()));
        let mismatch =
            Filter::parse_all(&["status>=500".to_string(), "level=info".to_string()]).unwrap();
        assert!(!mismatch.matches(&sample()));
    }

    #[test]
    fn quoted_values_are_unwrapped() {
        let filter = Filter::parse("msg contains \"pool exhausted\"").unwrap();
        assert!(filter.matches(&sample()));
    }

    #[test]
    fn invalid_expressions_report_why() {
        assert!(Filter::parse("status").is_err());
        assert!(Filter::parse("latency>abc").is_err());
        assert!(Filter::parse("=empty").is_err());
        assert!(Filter::parse("").is_err());
        assert!(Filter::parse("latency>").is_err());
    }
}
