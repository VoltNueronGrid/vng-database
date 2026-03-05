#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-driver-rust";

use std::collections::BTreeMap;

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
}
