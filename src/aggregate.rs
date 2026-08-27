//! Grouping and top-N aggregation.
//!
//! Counting stays in a sorted map so equal counts have a deterministic order
//! (highest count first, then field value). A log tool whose top list reshuffles
//! between identical runs is hard to trust.

use std::collections::BTreeMap;

use crate::value::{Record, Value};

/// One group in a top-N list.
#[derive(Clone, Debug, PartialEq)]
pub struct TopEntry {
    pub key: String,
    pub count: usize,
}

/// Fields whose every present value is numeric.
///
/// The rule is intentionally strict: a field that is numeric in most records
/// and text in one is not a numeric field, and averaging it would hide the
/// inconsistency instead of reporting it.
pub fn numeric_fields(records: &[Record]) -> Vec<String> {
    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for record in records {
        for (field, value) in record {
            let entry = stats.entry(field.clone()).or_insert((0, 0));
            if value.as_number().is_some() {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    stats
        .into_iter()
        .filter(|(_, (numeric, other))| *numeric > 0 && *other == 0)
        .map(|(field, _)| field)
        .collect()
}

/// Values of one field from records where it is numeric.
pub fn numeric_values(records: &[Record], field: &str) -> Vec<f64> {
    records
        .iter()
        .filter_map(|record| record.get(field))
        .filter_map(Value::as_number)
        .collect()
}

/// Count records per value of `field`, sorted by count descending then key.
/// Records without the field are skipped; the count is visible in the report.
pub fn top_n(records: &[Record], field: &str, n: usize) -> Vec<TopEntry> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for record in records {
        if let Some(value) = record.get(field) {
            *counts.entry(value.render()).or_insert(0) += 1;
        }
    }
    let mut entries: Vec<TopEntry> = counts
        .into_iter()
        .map(|(key, count)| TopEntry { key, count })
        .collect();
    entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.key.cmp(&right.key))
    });
    entries.truncate(n);
    entries
}

/// Number of records that have the field at all.
pub fn field_present(records: &[Record], field: &str) -> usize {
    records
        .iter()
        .filter(|record| record.contains_key(field))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(entries: &[(&str, Value)]) -> Record {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn top_counts_and_orders_by_count_then_key() {
        let records = vec![
            rec(&[("country", Value::Str("NP".into()))]),
            rec(&[("country", Value::Str("US".into()))]),
            rec(&[("country", Value::Str("NP".into()))]),
            rec(&[("country", Value::Str("IN".into()))]),
            rec(&[("country", Value::Str("US".into()))]),
            rec(&[("country", Value::Str("NP".into()))]),
        ];
        let top = top_n(&records, "country", 10);
        assert_eq!(
            top[0],
            TopEntry {
                key: "NP".into(),
                count: 3
            }
        );
        assert_eq!(
            top[1],
            TopEntry {
                key: "US".into(),
                count: 2
            }
        );
        assert_eq!(
            top[2],
            TopEntry {
                key: "IN".into(),
                count: 1
            }
        );
        assert_eq!(top_n(&records, "country", 2).len(), 2);
    }

    #[test]
    fn equal_counts_sort_by_key_for_deterministic_output() {
        let records = vec![
            rec(&[("country", Value::Str("US".into()))]),
            rec(&[("country", Value::Str("NP".into()))]),
        ];
        let top = top_n(&records, "country", 10);
        assert_eq!(top[0].key, "NP");
        assert_eq!(top[1].key, "US");
    }

    #[test]
    fn missing_fields_are_skipped() {
        let records = vec![rec(&[("other", Value::Str("x".into()))])];
        assert!(top_n(&records, "country", 10).is_empty());
        assert_eq!(field_present(&records, "country"), 0);
    }

    #[test]
    fn numeric_fields_require_every_present_value_to_be_numeric() {
        let records = vec![
            rec(&[
                ("latency", Value::Number(10.0)),
                ("status", Value::Number(200.0)),
            ]),
            rec(&[
                ("latency", Value::Number(20.0)),
                ("status", Value::Str("oops".into())),
            ]),
        ];
        let fields = numeric_fields(&records);
        assert_eq!(fields, vec!["latency".to_string()]);
        assert_eq!(numeric_values(&records, "latency"), vec![10.0, 20.0]);
    }
}
