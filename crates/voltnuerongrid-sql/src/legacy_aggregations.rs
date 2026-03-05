#![forbid(unsafe_code)]

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

pub fn is_legacy_aggregation_supported(name: &str) -> bool {
    let normalized = name.trim().to_ascii_uppercase();
    SUPPORTED_LEGACY_AGGREGATIONS
        .iter()
        .any(|candidate| candidate == &normalized)
}
