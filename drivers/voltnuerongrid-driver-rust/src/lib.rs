#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-driver-rust";

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    pub base_url: String,
    pub session_id: String,
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub admin_api_key: Option<String>,
    pub operator_id: Option<String>,
    pub route_hint: Option<String>,
}

impl DriverConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("base_url must not be empty".to_string());
        }
        if self.session_id.trim().is_empty() {
            return Err("session_id must not be empty".to_string());
        }
        if self.tenant_id.is_some() != self.user_id.is_some() {
            return Err("tenant_id and user_id must be set together".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverRoutingConfigContract {
    pub base_url: String,
    pub session_header_name: String,
    pub tenant_header_name: String,
    pub user_header_name: String,
    pub route_hint_header_name: String,
    pub admin_header_name: String,
    pub operator_header_name: String,
    pub pool_min_connections: u32,
    pub pool_max_connections: u32,
    pub request_timeout_ms: u64,
}

impl DriverRoutingConfigContract {
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("base_url must not be empty".to_string());
        }
        for header in [
            &self.session_header_name,
            &self.tenant_header_name,
            &self.user_header_name,
            &self.route_hint_header_name,
            &self.admin_header_name,
            &self.operator_header_name,
        ] {
            if header.trim().is_empty() || !header.starts_with("x-") {
                return Err("all header names must start with 'x-'".to_string());
            }
        }
        if self.pool_min_connections == 0 {
            return Err("pool_min_connections must be >= 1".to_string());
        }
        if self.pool_max_connections < self.pool_min_connections {
            return Err("pool_max_connections must be >= pool_min_connections".to_string());
        }
        if self.request_timeout_ms < 100 {
            return Err("request_timeout_ms must be >= 100".to_string());
        }
        Ok(())
    }

    pub fn from_json_str(input: &str) -> Result<Self, String> {
        let contract =
            serde_json::from_str::<Self>(input).map_err(|e| format!("json parse failed: {e}"))?;
        contract.validate()?;
        Ok(contract)
    }

    pub fn from_yaml_str(input: &str) -> Result<Self, String> {
        let contract =
            serde_yaml::from_str::<Self>(input).map_err(|e| format!("yaml parse failed: {e}"))?;
        contract.validate()?;
        Ok(contract)
    }

    pub fn from_properties_str(input: &str) -> Result<Self, String> {
        let mut map = BTreeMap::<String, String>::new();
        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }

        let parse_u32 = |key: &str| -> Result<u32, String> {
            map.get(key)
                .ok_or_else(|| format!("missing {key}"))?
                .parse::<u32>()
                .map_err(|_| format!("{key} must be integer"))
        };
        let parse_u64 = |key: &str| -> Result<u64, String> {
            map.get(key)
                .ok_or_else(|| format!("missing {key}"))?
                .parse::<u64>()
                .map_err(|_| format!("{key} must be integer"))
        };

        let contract = Self {
            base_url: map
                .get("driver.baseUrl")
                .ok_or_else(|| "missing driver.baseUrl".to_string())?
                .to_string(),
            session_header_name: map
                .get("driver.sessionHeaderName")
                .ok_or_else(|| "missing driver.sessionHeaderName".to_string())?
                .to_string(),
            tenant_header_name: map
                .get("driver.tenantHeaderName")
                .ok_or_else(|| "missing driver.tenantHeaderName".to_string())?
                .to_string(),
            user_header_name: map
                .get("driver.userHeaderName")
                .ok_or_else(|| "missing driver.userHeaderName".to_string())?
                .to_string(),
            route_hint_header_name: map
                .get("driver.routeHintHeaderName")
                .ok_or_else(|| "missing driver.routeHintHeaderName".to_string())?
                .to_string(),
            admin_header_name: map
                .get("driver.adminHeaderName")
                .ok_or_else(|| "missing driver.adminHeaderName".to_string())?
                .to_string(),
            operator_header_name: map
                .get("driver.operatorHeaderName")
                .ok_or_else(|| "missing driver.operatorHeaderName".to_string())?
                .to_string(),
            pool_min_connections: parse_u32("driver.pool.minConnections")?,
            pool_max_connections: parse_u32("driver.pool.maxConnections")?,
            request_timeout_ms: parse_u64("driver.requestTimeoutMs")?,
        };
        contract.validate()?;
        Ok(contract)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body_json: String,
}

#[derive(Debug, Clone)]
pub struct VoltNueronGridDriver {
    config: DriverConfig,
}

impl VoltNueronGridDriver {
    pub fn new(config: DriverConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn build_sql_execute_request(
        &self,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> Result<DriverRequest, String> {
        if sql_batch.trim().is_empty() {
            return Err("sql_batch must not be empty".to_string());
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
            "max_rows": max_rows,
        });
        Ok(self.build_json_post("/api/v1/sql/execute", body))
    }

    pub fn build_sql_analyze_request(&self, sql_batch: &str) -> Result<DriverRequest, String> {
        if sql_batch.trim().is_empty() {
            return Err("sql_batch must not be empty".to_string());
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
        });
        Ok(self.build_json_post("/api/v1/sql/analyze", body))
    }

    pub fn build_sql_route_request(&self, sql_batch: &str) -> Result<DriverRequest, String> {
        if sql_batch.trim().is_empty() {
            return Err("sql_batch must not be empty".to_string());
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
        });
        Ok(self.build_json_post("/api/v1/sql/route", body))
    }

    pub fn build_sql_transaction_request(&self, statements: &[&str]) -> Result<DriverRequest, String> {
        if statements.is_empty() {
            return Err("statements must not be empty".to_string());
        }
        let body = serde_json::json!({
            "statements": statements,
        });
        Ok(self.build_json_post("/api/v1/sql/transaction", body))
    }

    pub fn build_authorize_action_request(
        &self,
        action: &str,
        scope: Option<&str>,
    ) -> Result<DriverRequest, String> {
        if action.trim().is_empty() {
            return Err("action must not be empty".to_string());
        }
        let body = serde_json::json!({
            "action": action,
            "scope": scope,
        });
        Ok(self.build_json_post("/api/v1/autonomous/actions/authorize", body))
    }

    fn build_json_post(&self, path: &str, body: serde_json::Value) -> DriverRequest {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("x-vng-session-id".to_string(), self.config.session_id.clone());
        if let Some(tenant_id) = &self.config.tenant_id {
            headers.insert("x-vng-tenant-id".to_string(), tenant_id.clone());
        }
        if let Some(user_id) = &self.config.user_id {
            headers.insert("x-vng-user-id".to_string(), user_id.clone());
        }
        if let Some(route_hint) = &self.config.route_hint {
            headers.insert("x-vng-route-hint".to_string(), route_hint.clone());
        }
        if let Some(admin_key) = &self.config.admin_api_key {
            headers.insert("x-vng-admin-key".to_string(), admin_key.clone());
        }
        if let Some(operator_id) = &self.config.operator_id {
            headers.insert("x-vng-operator-id".to_string(), operator_id.clone());
        }

        DriverRequest {
            method: "POST".to_string(),
            url: format!(
                "{}{}",
                self.config.base_url.trim_end_matches('/'),
                path
            ),
            headers,
            body_json: body.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolConnectionState {
    Idle,
    Active,
    HealthChecking,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PooledConnection {
    pub connection_id: String,
    pub state: PoolConnectionState,
    pub created_at_ms: u64,
    pub last_used_ms: u64,
    pub use_count: u64,
    pub last_error: Option<String>,
}

impl PooledConnection {
    pub fn new(id: String, now_ms: u64) -> Self {
        Self {
            connection_id: id,
            state: PoolConnectionState::Idle,
            created_at_ms: now_ms,
            last_used_ms: now_ms,
            use_count: 0,
            last_error: None,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == PoolConnectionState::Idle
    }

    pub fn is_healthy(&self, now_ms: u64, idle_timeout_ms: u64) -> bool {
        self.state != PoolConnectionState::Failed
            && now_ms.saturating_sub(self.last_used_ms) < idle_timeout_ms
    }

    pub fn mark_active(&mut self, now_ms: u64) {
        self.state = PoolConnectionState::Active;
        self.last_used_ms = now_ms;
        self.use_count = self.use_count.saturating_add(1);
    }

    pub fn mark_idle(&mut self, now_ms: u64) {
        self.state = PoolConnectionState::Idle;
        self.last_used_ms = now_ms;
    }

    pub fn mark_failed(&mut self, error: String) {
        self.state = PoolConnectionState::Failed;
        self.last_error = Some(error);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolCircuitBreakerState {
    Closed,
    Open { opened_at_ms: u64, failure_count: u32 },
    HalfOpen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolBackpressurePolicy {
    pub max_pool_size: usize,
    pub min_pool_size: usize,
    pub acquire_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub max_queue_depth: usize,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_reset_ms: u64,
    pub storm_detection_window_ms: u64,
    pub storm_detection_rps_threshold: u64,
}

impl Default for PoolBackpressurePolicy {
    fn default() -> Self {
        Self {
            max_pool_size: 50,
            min_pool_size: 5,
            acquire_timeout_ms: 5000,
            idle_timeout_ms: 60000,
            max_queue_depth: 200,
            circuit_breaker_threshold: 10,
            circuit_breaker_reset_ms: 30000,
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StormDetector {
    pub policy: PoolBackpressurePolicy,
    request_timestamps_ms: Vec<u64>,
    pub storm_active: bool,
    pub storm_started_at_ms: Option<u64>,
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub storm_events: u64,
}

impl StormDetector {
    pub fn new(policy: PoolBackpressurePolicy) -> Self {
        Self {
            policy,
            request_timestamps_ms: Vec::new(),
            storm_active: false,
            storm_started_at_ms: None,
            total_requests: 0,
            rejected_requests: 0,
            storm_events: 0,
        }
    }

    pub fn record_request(&mut self, now_ms: u64) -> StormCheckResult {
        self.total_requests = self.total_requests.saturating_add(1);
        self.request_timestamps_ms.push(now_ms);

        let window_ms = self.policy.storm_detection_window_ms.max(1);
        self.request_timestamps_ms
            .retain(|ts| now_ms.saturating_sub(*ts) <= window_ms);

        let window_seconds = (window_ms / 1000).max(1);
        let current_rps = (self.request_timestamps_ms.len() as u64) / window_seconds;
        let threshold = self.policy.storm_detection_rps_threshold;

        if current_rps >= threshold && !self.storm_active {
            self.storm_active = true;
            self.storm_events = self.storm_events.saturating_add(1);
            self.storm_started_at_ms = Some(now_ms);
        } else if current_rps < (threshold / 2) && self.storm_active {
            self.storm_active = false;
            self.storm_started_at_ms = None;
        }

        StormCheckResult {
            storm_active: self.storm_active,
            current_rps,
            threshold_rps: threshold,
        }
    }

    pub fn current_rps(&self, now_ms: u64) -> u64 {
        let window_ms = self.policy.storm_detection_window_ms.max(1);
        let window_seconds = (window_ms / 1000).max(1);
        let requests_in_window = self
            .request_timestamps_ms
            .iter()
            .filter(|ts| now_ms.saturating_sub(**ts) <= window_ms)
            .count() as u64;
        requests_in_window / window_seconds
    }

    pub fn storm_stats(&self) -> StormStats {
        StormStats {
            storm_active: self.storm_active,
            storm_events: self.storm_events,
            total_requests: self.total_requests,
            rejected_requests: self.rejected_requests,
            current_rps: self.current_rps(*self.request_timestamps_ms.last().unwrap_or(&0u64)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StormCheckResult {
    pub storm_active: bool,
    pub current_rps: u64,
    pub threshold_rps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StormStats {
    pub storm_active: bool,
    pub storm_events: u64,
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub current_rps: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionPoolManager {
    connections: Vec<PooledConnection>,
    policy: PoolBackpressurePolicy,
    circuit_breaker: PoolCircuitBreakerState,
    storm_detector: StormDetector,
    circuit_failure_count: u32,
    pub connection_counter: u64,
    pub total_acquired: u64,
    pub total_released: u64,
    pub total_rejected: u64,
    pub total_circuit_opens: u64,
}

impl ConnectionPoolManager {
    pub fn new(policy: PoolBackpressurePolicy) -> Self {
        let mut connections = Vec::with_capacity(policy.max_pool_size);
        for i in 0..policy.min_pool_size {
            connections.push(PooledConnection::new(format!("conn-{}", i + 1), 0u64));
        }
        Self {
            connection_counter: policy.min_pool_size as u64,
            connections,
            circuit_breaker: PoolCircuitBreakerState::Closed,
            storm_detector: StormDetector::new(policy.clone()),
            policy,
            circuit_failure_count: 0,
            total_acquired: 0,
            total_released: 0,
            total_rejected: 0,
            total_circuit_opens: 0,
        }
    }

    pub fn with_default_policy() -> Self {
        Self::new(PoolBackpressurePolicy::default())
    }

    pub fn acquire(&mut self, now_ms: u64) -> Result<String, PoolAcquireError> {
        let storm = self.storm_detector.record_request(now_ms);
        let active_count = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Active)
            .count();

        let queue_pressure_limit = self.policy.max_queue_depth.min(self.policy.max_pool_size);
        if storm.storm_active && active_count >= queue_pressure_limit {
            self.total_rejected = self.total_rejected.saturating_add(1);
            self.storm_detector.rejected_requests =
                self.storm_detector.rejected_requests.saturating_add(1);
            return Err(PoolAcquireError::StormRejection {
                current_rps: storm.current_rps,
                threshold: storm.threshold_rps,
            });
        }

        if let PoolCircuitBreakerState::Open {
            opened_at_ms,
            failure_count,
        } = self.circuit_breaker
        {
            let elapsed = now_ms.saturating_sub(opened_at_ms);
            if elapsed < self.policy.circuit_breaker_reset_ms {
                self.total_rejected = self.total_rejected.saturating_add(1);
                return Err(PoolAcquireError::CircuitOpen {
                    failure_count,
                    reset_in_ms: self.policy.circuit_breaker_reset_ms.saturating_sub(elapsed),
                });
            }
            self.circuit_breaker = PoolCircuitBreakerState::HalfOpen;
            self.circuit_failure_count = 0;
        }

        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.is_idle() && c.is_healthy(now_ms, self.policy.idle_timeout_ms))
        {
            connection.mark_active(now_ms);
            self.total_acquired = self.total_acquired.saturating_add(1);
            if self.circuit_breaker == PoolCircuitBreakerState::HalfOpen {
                self.circuit_breaker = PoolCircuitBreakerState::Closed;
                self.circuit_failure_count = 0;
            }
            return Ok(connection.connection_id.clone());
        }

        if self.connections.len() < self.policy.max_pool_size {
            self.connection_counter = self.connection_counter.saturating_add(1);
            let conn_id = format!("conn-{}", self.connection_counter);
            let mut connection = PooledConnection::new(conn_id.clone(), now_ms);
            connection.mark_active(now_ms);
            self.connections.push(connection);
            self.total_acquired = self.total_acquired.saturating_add(1);
            if self.circuit_breaker == PoolCircuitBreakerState::HalfOpen {
                self.circuit_breaker = PoolCircuitBreakerState::Closed;
                self.circuit_failure_count = 0;
            }
            return Ok(conn_id);
        }

        self.total_rejected = self.total_rejected.saturating_add(1);
        Err(PoolAcquireError::PoolExhausted {
            active_count,
            max: self.policy.max_pool_size,
        })
    }

    pub fn release(&mut self, connection_id: &str, now_ms: u64) -> bool {
        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.connection_id == connection_id)
        {
            connection.mark_idle(now_ms);
            self.total_released = self.total_released.saturating_add(1);
            return true;
        }
        false
    }

    pub fn mark_failed(&mut self, connection_id: &str, error: String, now_ms: u64) {
        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.connection_id == connection_id)
        {
            connection.mark_failed(error);
            self.record_circuit_failure(now_ms);
        }
    }

    pub fn record_circuit_failure(&mut self, now_ms: u64) {
        self.circuit_failure_count = self.circuit_failure_count.saturating_add(1);
        if self.circuit_failure_count >= self.policy.circuit_breaker_threshold {
            self.circuit_breaker = PoolCircuitBreakerState::Open {
                opened_at_ms: now_ms,
                failure_count: self.circuit_failure_count,
            };
            self.total_circuit_opens = self.total_circuit_opens.saturating_add(1);
        }
    }

    pub fn check_circuit_recovery(&mut self, now_ms: u64) -> bool {
        if let PoolCircuitBreakerState::Open {
            opened_at_ms,
            failure_count: _,
        } = self.circuit_breaker
        {
            if now_ms.saturating_sub(opened_at_ms) >= self.policy.circuit_breaker_reset_ms {
                self.circuit_breaker = PoolCircuitBreakerState::HalfOpen;
                self.circuit_failure_count = 0;
                return true;
            }
        }
        false
    }

    pub fn prune_unhealthy(&mut self, now_ms: u64) -> usize {
        let original_len = self.connections.len();
        self.connections
            .retain(|c| c.is_healthy(now_ms, self.policy.idle_timeout_ms));
        original_len.saturating_sub(self.connections.len())
    }

    pub fn pool_stats(&self, now_ms: u64) -> PoolStats {
        let total_connections = self.connections.len();
        let idle_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Idle)
            .count();
        let active_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Active)
            .count();
        let failed_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Failed)
            .count();

        PoolStats {
            total_connections,
            idle_connections,
            active_connections,
            failed_connections,
            circuit_breaker_state: self.circuit_breaker_state_str().to_string(),
            storm_active: self.storm_detector.storm_active,
            current_rps: self.storm_detector.current_rps(now_ms),
            total_acquired: self.total_acquired,
            total_released: self.total_released,
            total_rejected: self.total_rejected,
            total_circuit_opens: self.total_circuit_opens,
        }
    }

    pub fn circuit_breaker_state_str(&self) -> &'static str {
        match self.circuit_breaker {
            PoolCircuitBreakerState::Closed => "closed",
            PoolCircuitBreakerState::Open { .. } => "open",
            PoolCircuitBreakerState::HalfOpen => "half_open",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolAcquireError {
    PoolExhausted { active_count: usize, max: usize },
    CircuitOpen { failure_count: u32, reset_in_ms: u64 },
    StormRejection { current_rps: u64, threshold: u64 },
    AcquireTimeout { waited_ms: u64 },
}

impl std::fmt::Display for PoolAcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolAcquireError::PoolExhausted { active_count, max } => {
                write!(f, "pool exhausted: active={} max={}", active_count, max)
            }
            PoolAcquireError::CircuitOpen {
                failure_count,
                reset_in_ms,
            } => write!(
                f,
                "circuit open: failures={} reset_in_ms={}",
                failure_count, reset_in_ms
            ),
            PoolAcquireError::StormRejection {
                current_rps,
                threshold,
            } => write!(
                f,
                "storm rejection: current_rps={} threshold={}",
                current_rps, threshold
            ),
            PoolAcquireError::AcquireTimeout { waited_ms } => {
                write!(f, "acquire timeout: waited_ms={}", waited_ms)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolStats {
    pub total_connections: usize,
    pub idle_connections: usize,
    pub active_connections: usize,
    pub failed_connections: usize,
    pub circuit_breaker_state: String,
    pub storm_active: bool,
    pub current_rps: u64,
    pub total_acquired: u64,
    pub total_released: u64,
    pub total_rejected: u64,
    pub total_circuit_opens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DriverConfig {
        DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            session_id: "sess-1".to_string(),
            tenant_id: Some("acme".to_string()),
            user_id: Some("analyst-acme".to_string()),
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("operator-a".to_string()),
            route_hint: Some("oltp".to_string()),
        }
    }

    #[test]
    fn builds_sql_execute_request_with_session_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let request = driver
            .build_sql_execute_request("SELECT 1", Some(100))
            .expect("request");
        assert_eq!(request.method, "POST");
        assert!(request.url.ends_with("/api/v1/sql/execute"));
        assert_eq!(
            request.headers.get("x-vng-session-id").map(String::as_str),
            Some("sess-1")
        );
        assert_eq!(
            request.headers.get("x-vng-tenant-id").map(String::as_str),
            Some("acme")
        );
        assert_eq!(
            request.headers.get("x-vng-user-id").map(String::as_str),
            Some("analyst-acme")
        );
        assert_eq!(
            request.headers.get("x-vng-route-hint").map(String::as_str),
            Some("oltp")
        );
        assert!(request.body_json.contains("SELECT 1"));
    }

    #[test]
    fn builds_authorize_request_with_admin_and_operator_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let request = driver
            .build_authorize_action_request("schema_change", Some("database"))
            .expect("request");
        assert!(request.url.ends_with("/api/v1/autonomous/actions/authorize"));
        assert_eq!(
            request.headers.get("x-vng-admin-key").map(String::as_str),
            Some("secret")
        );
        assert_eq!(
            request.headers.get("x-vng-operator-id").map(String::as_str),
            Some("operator-a")
        );
        assert!(request.body_json.contains("schema_change"));
    }

    #[test]
    fn builds_sql_route_and_transaction_requests_with_user_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let route = driver.build_sql_route_request("SELECT 1").expect("route request");
        let tx = driver
            .build_sql_transaction_request(&["BEGIN", "COMMIT"])
            .expect("transaction request");
        assert!(route.url.ends_with("/api/v1/sql/route"));
        assert!(tx.url.ends_with("/api/v1/sql/transaction"));
        assert_eq!(route.headers.get("x-vng-user-id").map(String::as_str), Some("analyst-acme"));
        assert_eq!(tx.headers.get("x-vng-tenant-id").map(String::as_str), Some("acme"));
    }

    #[test]
    fn rejects_partial_tenant_user_config() {
        let err = VoltNueronGridDriver::new(DriverConfig {
            tenant_id: Some("acme".to_string()),
            user_id: None,
            ..config()
        })
        .expect_err("partial tenant user config should fail");
        assert!(err.contains("tenant_id and user_id"));
    }

    #[test]
    fn validates_driver_contract_from_json() {
        let json = r#"{
          "base_url":"http://127.0.0.1:8080",
          "session_header_name":"x-vng-session-id",
                    "tenant_header_name":"x-vng-tenant-id",
                    "user_header_name":"x-vng-user-id",
          "route_hint_header_name":"x-vng-route-hint",
          "admin_header_name":"x-vng-admin-key",
          "operator_header_name":"x-vng-operator-id",
          "pool_min_connections":2,
          "pool_max_connections":16,
          "request_timeout_ms":2500
        }"#;
        let contract = DriverRoutingConfigContract::from_json_str(json).expect("valid");
        assert_eq!(contract.pool_max_connections, 16);
    }

    #[test]
    fn validates_driver_contract_from_properties() {
        let properties = r#"
driver.baseUrl=http://127.0.0.1:8080
driver.sessionHeaderName=x-vng-session-id
driver.tenantHeaderName=x-vng-tenant-id
driver.userHeaderName=x-vng-user-id
driver.routeHintHeaderName=x-vng-route-hint
driver.adminHeaderName=x-vng-admin-key
driver.operatorHeaderName=x-vng-operator-id
driver.pool.minConnections=2
driver.pool.maxConnections=16
driver.requestTimeoutMs=2500
"#;
        let contract =
            DriverRoutingConfigContract::from_properties_str(properties).expect("valid");
        assert_eq!(contract.request_timeout_ms, 2500);
    }

    #[test]
    fn validates_driver_contract_from_yaml() {
        let yaml = r#"
base_url: http://127.0.0.1:8080
session_header_name: x-vng-session-id
tenant_header_name: x-vng-tenant-id
user_header_name: x-vng-user-id
route_hint_header_name: x-vng-route-hint
admin_header_name: x-vng-admin-key
operator_header_name: x-vng-operator-id
pool_min_connections: 2
pool_max_connections: 16
request_timeout_ms: 2500
"#;
        let contract = DriverRoutingConfigContract::from_yaml_str(yaml).expect("valid");
        assert_eq!(contract.pool_min_connections, 2);
    }

    #[test]
    fn test_pool_acquire_and_release() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 2,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        assert!(pool.release(&conn_id, 100u64));
        let stats = pool.pool_stats(100u64);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn test_pool_exhaustion_error() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 0,
            max_pool_size: 1,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let _ = pool.acquire(0u64).expect("first acquire should pass");
        let err = pool.acquire(1u64).expect_err("pool should be exhausted");
        assert!(matches!(err, PoolAcquireError::PoolExhausted { .. }));
    }

    #[test]
    fn test_pool_circuit_breaker_opens() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            circuit_breaker_threshold: 2,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "io failure".to_string(), 10u64);
        pool.mark_failed(&conn_id, "io failure".to_string(), 20u64);

        let err = pool.acquire(21u64).expect_err("circuit should be open");
        assert!(matches!(err, PoolAcquireError::CircuitOpen { .. }));
    }

    #[test]
    fn test_pool_circuit_breaker_half_open() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            circuit_breaker_threshold: 1,
            circuit_breaker_reset_ms: 1000,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "downstream failure".to_string(), 5u64);

        assert!(pool.check_circuit_recovery(1005u64));
        assert_eq!(pool.circuit_breaker_state_str(), "half_open");
    }

    #[test]
    fn test_storm_detector_activates() {
        let policy = PoolBackpressurePolicy {
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 10,
            ..PoolBackpressurePolicy::default()
        };
        let mut detector = StormDetector::new(policy);

        for i in 0u64..10u64 {
            let _ = detector.record_request(i);
        }

        assert!(detector.storm_active);
    }

    #[test]
    fn test_storm_detector_deactivates() {
        let policy = PoolBackpressurePolicy {
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 10,
            ..PoolBackpressurePolicy::default()
        };
        let mut detector = StormDetector::new(policy);

        for i in 0u64..10u64 {
            let _ = detector.record_request(i);
        }
        assert!(detector.storm_active);

        let _ = detector.record_request(3000u64);
        assert!(!detector.storm_active);
    }

    #[test]
    fn test_pool_prune_unhealthy() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "failed health check".to_string(), 10u64);

        let pruned = pool.prune_unhealthy(11u64);
        assert_eq!(pruned, 1);
        assert_eq!(pool.pool_stats(11u64).total_connections, 0);
    }
}
