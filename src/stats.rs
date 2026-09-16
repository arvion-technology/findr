//! Percentiles and summary statistics.
//!
//! Percentiles use linear interpolation between ranks. For a latency series
//! this is the value an operator expects: p50 is the median, p95 has 5% of
//! observations above it, and small series do not snap to the lowest value.

/// Summary of a numeric field.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Interpolated percentile. `p` is in `0..=100`; the slice must be sorted.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let clamped = p.clamp(0.0, 100.0);
    let rank = (sorted.len() - 1) as f64 * clamped / 100.0;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        return Some(sorted[lower]);
    }
    let weight = rank - lower as f64;
    Some(sorted[lower] * (1.0 - weight) + sorted[upper] * weight)
}

/// Summarize values. Returns `None` for an empty series.
pub fn summarize(values: Vec<f64>) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values;
    sorted.sort_by(|left, right| left.total_cmp(right));
    let count = sorted.len();
    let sum: f64 = sorted.iter().sum();
    Some(Summary {
        count,
        min: sorted[0],
        max: sorted[count - 1],
        sum,
        mean: sum / count as f64,
        p50: percentile(&sorted, 50.0).unwrap_or(0.0),
        p95: percentile(&sorted, 95.0).unwrap_or(0.0),
        p99: percentile(&sorted, 99.0).unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_interpolates_between_ranks() {
        let values = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(percentile(&values, 0.0), Some(10.0));
        assert_eq!(percentile(&values, 50.0), Some(25.0));
        assert_eq!(percentile(&values, 100.0), Some(40.0));
        assert_eq!(percentile(&values, 25.0), Some(17.5));
    }

    #[test]
    fn single_value_percentiles_are_that_value() {
        let values = [7.5];
        assert_eq!(percentile(&values, 95.0), Some(7.5));
        assert_eq!(percentile(&values, 100.0), Some(7.5));
    }

    #[test]
    fn summarize_sorts_and_computes_spread() {
        let summary = summarize(vec![40.0, 10.0, 30.0, 20.0]).unwrap();
        assert_eq!(summary.count, 4);
        assert_eq!(summary.min, 10.0);
        assert_eq!(summary.max, 40.0);
        assert_eq!(summary.sum, 100.0);
        assert_eq!(summary.mean, 25.0);
        assert_eq!(summary.p50, 25.0);
        assert!((summary.p95 - 38.5).abs() < 1e-9);
    }

    #[test]
    fn empty_series_has_no_summary() {
        assert_eq!(summarize(Vec::new()), None);
        assert_eq!(percentile(&[], 50.0), None);
    }
}
