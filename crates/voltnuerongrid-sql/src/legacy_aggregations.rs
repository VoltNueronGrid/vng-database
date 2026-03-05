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
