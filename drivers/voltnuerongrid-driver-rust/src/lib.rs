#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-driver-rust";

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    pub base_url: String,
    pub session_id: String,
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
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverRoutingConfigContract {
    pub base_url: String,
    pub session_header_name: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DriverConfig {
        DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            session_id: "sess-1".to_string(),
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
    fn validates_driver_contract_from_json() {
        let json = r#"{
          "base_url":"http://127.0.0.1:8080",
          "session_header_name":"x-vng-session-id",
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
}
