#![forbid(unsafe_code)]

use std::collections::HashSet;

pub const SUPPORTED_LEGACY_AGGREGATIONS: &[&str] = &[
    "SUM",
    "COUNT",
    "MIN",
    "MAX",
    "AVG",
    "COUNT_DISTINCT",
    "MEDIAN",
    "STDDEV",
    "VARIANCE",
    "PERCENTILE",
];

pub const P2_STUB_AGGREGATIONS: &[&str] = &["APPROX_COUNT_DISTINCT", "TOP_N", "BOTTOM_N"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P2StubResult {
    pub aggregation: String,
    pub accepted: bool,
    pub mode: &'static str,
    pub reason: String,
}

pub fn is_legacy_aggregation_supported(name: &str) -> bool {
    let normalized = name.trim().to_ascii_uppercase();
    SUPPORTED_LEGACY_AGGREGATIONS
        .iter()
        .any(|candidate| candidate == &normalized)
}

pub fn is_p2_stub_supported(name: &str) -> bool {
    let normalized = name.trim().to_ascii_uppercase();
    P2_STUB_AGGREGATIONS
        .iter()
        .any(|candidate| candidate == &normalized)
}

pub fn run_p2_stub(aggregation: &str) -> P2StubResult {
    let normalized = aggregation.trim().to_ascii_uppercase();
    if is_p2_stub_supported(&normalized) {
        P2StubResult {
            aggregation: normalized,
            accepted: true,
            mode: "stub",
            reason: "starter stub implementation accepted; full engine semantics pending".to_string(),
        }
    } else {
        P2StubResult {
            aggregation: normalized,
            accepted: false,
            mode: "unsupported",
            reason: "aggregation is not recognized as a P2 stub".to_string(),
        }
    }
}

/// Evaluate a supported legacy aggregation over a finite slice of `f64` samples.
///
/// - `PERCENTILE` requires `percentile` in **0–100** (inclusive); linear interpolation is used
///   between adjacent sorted values (inclusive endpoints).
/// - `VARIANCE` / `STDDEV` use the **sample** variance with divisor *n − 1* when *n ≥ 2*.
pub fn eval_legacy_numeric_aggregation(
    name: &str,
    values: &[f64],
    percentile: Option<f64>,
) -> Result<f64, String> {
    let normalized = name.trim().to_ascii_uppercase();
    if !is_legacy_aggregation_supported(&normalized) {
        return Err(format!("unsupported legacy aggregation: {normalized}"));
    }

    match normalized.as_str() {
        "COUNT" => Ok(values.len() as f64),
        "SUM" => {
            if values.is_empty() {
                return Err("SUM requires at least one value".to_string());
            }
            Ok(values.iter().copied().sum())
        }
        "MIN" => values
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or_else(|| "MIN requires at least one value".to_string()),
        "MAX" => values
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| "MAX requires at least one value".to_string()),
        "AVG" => {
            if values.is_empty() {
                return Err("AVG requires at least one value".to_string());
            }
            let sum: f64 = values.iter().copied().sum();
            Ok(sum / values.len() as f64)
        }
        "COUNT_DISTINCT" => {
            let unique: HashSet<u64> = values.iter().copied().map(f64::to_bits).collect();
            Ok(unique.len() as f64)
        }
        "MEDIAN" => median_sorted(&mut values.to_vec()),
        "VARIANCE" => sample_variance(values),
        "STDDEV" => sample_variance(values).map(f64::sqrt),
        "PERCENTILE" => {
            let p = percentile.ok_or_else(|| {
                "PERCENTILE requires a percentile argument in the range [0, 100]".to_string()
            })?;
            if !p.is_finite() || !(0.0..=100.0).contains(&p) {
                return Err("PERCENTILE argument must be finite and within [0, 100]".to_string());
            }
            percentile_linear(values, p)
        }
        other => Err(format!("internal mapping missing for {other}")),
    }
}

fn median_sorted(values: &mut [f64]) -> Result<f64, String> {
    if values.is_empty() {
        return Err("MEDIAN requires at least one value".to_string());
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        Ok(values[mid])
    } else {
        Ok((values[mid - 1] + values[mid]) / 2.0)
    }
}

fn sample_variance(values: &[f64]) -> Result<f64, String> {
    let n = values.len();
    if n < 2 {
        return Err("VARIANCE/STDDEV require at least two values".to_string());
    }
    let mean: f64 = values.iter().copied().sum::<f64>() / n as f64;
    let sum_sq: f64 = values.iter().map(|value| {
        let d = *value - mean;
        d * d
    }).sum();
    Ok(sum_sq / (n - 1) as f64)
}

fn percentile_linear(values: &[f64], p: f64) -> Result<f64, String> {
    if values.is_empty() {
        return Err("PERCENTILE requires at least one value".to_string());
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if sorted.len() == 1 {
        return Ok(sorted[0]);
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        return Ok(sorted[lower]);
    }
    let frac = rank - lower as f64;
    Ok(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
}

#[cfg(test)]
mod numeric_tests {
    use super::*;

    #[test]
    fn sum_and_avg_basic() {
        let v = [1.0, 2.0, 3.0];
        assert_eq!(eval_legacy_numeric_aggregation("SUM", &v, None).unwrap(), 6.0);
        assert_eq!(eval_legacy_numeric_aggregation("AVG", &v, None).unwrap(), 2.0);
    }

    #[test]
    fn count_and_count_distinct() {
        assert_eq!(
            eval_legacy_numeric_aggregation("COUNT", &[], None).unwrap(),
            0.0
        );
        let v = [1.0, 1.0, 2.0, f64::NAN];
        assert_eq!(eval_legacy_numeric_aggregation("COUNT", &v, None).unwrap(), 4.0);
        // NaN bits are distinct from finite values; two 1.0 collapse to one bucket.
        let distinct = eval_legacy_numeric_aggregation("COUNT_DISTINCT", &v, None).unwrap();
        assert_eq!(distinct, 3.0);
    }

    #[test]
    fn median_even_and_odd() {
        assert_eq!(
            eval_legacy_numeric_aggregation("MEDIAN", &[1.0, 3.0, 2.0], None).unwrap(),
            2.0
        );
        assert_eq!(
            eval_legacy_numeric_aggregation("MEDIAN", &[1.0, 2.0, 3.0, 4.0], None).unwrap(),
            2.5
        );
    }

    #[test]
    fn variance_and_stddev_match_simple_dataset() {
        let v = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let var = eval_legacy_numeric_aggregation("VARIANCE", &v, None).unwrap();
        let std = eval_legacy_numeric_aggregation("STDDEV", &v, None).unwrap();
        assert!((var - 4.571428571428571).abs() < 1e-9);
        assert!((std - var.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn percentile_endpoints_and_mid() {
        let v = [10.0, 20.0, 30.0];
        assert_eq!(
            eval_legacy_numeric_aggregation("PERCENTILE", &v, Some(0.0)).unwrap(),
            10.0
        );
        assert_eq!(
            eval_legacy_numeric_aggregation("PERCENTILE", &v, Some(100.0)).unwrap(),
            30.0
        );
        assert_eq!(
            eval_legacy_numeric_aggregation("PERCENTILE", &v, Some(50.0)).unwrap(),
            20.0
        );
    }

    #[test]
    fn rejects_unsupported_name() {
        assert!(eval_legacy_numeric_aggregation("NOSUCH", &[1.0], None).is_err());
    }

    #[test]
    fn percentile_requires_argument() {
        assert!(eval_legacy_numeric_aggregation("PERCENTILE", &[1.0, 2.0], None).is_err());
    }
}
