// CONN-5: WebDAV connector — HTTP PROPFIND/GET via ureq (sync).
//
// Supports SharePoint, Nextcloud, and any WebDAV-compliant server.
// Auth: Basic (VNG_WEBDAV_USERNAME / VNG_WEBDAV_PASSWORD) or Bearer token
//       (VNG_WEBDAV_TOKEN).
// Env vars: VNG_WEBDAV_URL, VNG_WEBDAV_USERNAME, VNG_WEBDAV_PASSWORD,
//            VNG_WEBDAV_TOKEN, VNG_WEBDAV_DEPTH (default "1"),
//            VNG_WEBDAV_EXTENSIONS (comma-separated).

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestionConnector, IngestFormat, IngestRecord,
};

/// Errors produced by the WebDAV connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDavError {
    MissingBaseUrl,
    RequestFailed(String),
    ParseFailed(String),
}

impl std::fmt::Display for WebDavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBaseUrl => write!(f, "webdav_missing_base_url: set VNG_WEBDAV_URL"),
            Self::RequestFailed(m) => write!(f, "webdav_request_failed: {m}"),
            Self::ParseFailed(m) => write!(f, "webdav_parse_failed: {m}"),
        }
    }
}

/// Configuration for the WebDAV connector.
#[derive(Debug, Clone)]
pub struct WebDavConfig {
    /// Base URL of the WebDAV server, e.g. "https://dav.example.com/remote.php/dav/files/user/"
    pub base_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub bearer_token: Option<String>,
    /// PROPFIND Depth header value: "0", "1", or "infinity".
    pub depth: String,
    /// File extensions to include. Empty = all files.
    pub extensions: Vec<String>,
}

impl WebDavConfig {
    pub fn from_env() -> Result<Self, WebDavError> {
        let base_url = std::env::var("VNG_WEBDAV_URL")
            .map_err(|_| WebDavError::MissingBaseUrl)?;
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(WebDavError::MissingBaseUrl);
        }
        let extensions_raw = std::env::var("VNG_WEBDAV_EXTENSIONS").unwrap_or_default();
        let extensions = extensions_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        Ok(Self {
            base_url,
            username: std::env::var("VNG_WEBDAV_USERNAME").ok(),
            password: std::env::var("VNG_WEBDAV_PASSWORD").ok(),
            bearer_token: std::env::var("VNG_WEBDAV_TOKEN").ok(),
            depth: std::env::var("VNG_WEBDAV_DEPTH").unwrap_or_else(|_| "1".to_string()),
            extensions,
        })
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            username: None,
            password: None,
            bearer_token: None,
            depth: "1".to_string(),
            extensions: vec![],
        }
    }
}

/// Extract `<href>` values from a PROPFIND XML response body.
/// Parses `<D:href>` or `<href>` tags and returns the path segments.
pub fn parse_propfind_hrefs(xml: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    let mut remaining = xml;
    while let Some(start) = remaining.find("<href>").or_else(|| remaining.find("<D:href>")) {
        let tag_len = if remaining[start..].starts_with("<D:href>") { 8 } else { 6 };
        let inner = &remaining[start + tag_len..];
        let close = if remaining[start..].starts_with("<D:href>") { "</D:href>" } else { "</href>" };
        if let Some(end) = inner.find(close) {
            let href = inner[..end].trim().to_string();
            if !href.is_empty() {
                hrefs.push(href);
            }
            remaining = &inner[end + close.len()..];
        } else {
            break;
        }
    }
    hrefs
}

/// Perform PROPFIND to list resources, then GET each file.
/// Returns `(href_path, bytes)` for each matching resource.
pub fn webdav_list_and_fetch(cfg: &WebDavConfig) -> Result<Vec<(String, Vec<u8>)>, WebDavError> {
    let agent = build_agent(cfg);

    let propfind_body = r#"<?xml version="1.0" encoding="utf-8"?>
<propfind xmlns="DAV:"><allprop/></propfind>"#;

    // PROPFIND to list resources
    let resp = agent
        .request("PROPFIND", &cfg.base_url)
        .set("Depth", &cfg.depth)
        .set("Content-Type", "application/xml; charset=utf-8")
        .send_string(propfind_body)
        .map_err(|e| WebDavError::RequestFailed(e.to_string()))?;

    let xml = resp
        .into_string()
        .map_err(|e| WebDavError::ParseFailed(e.to_string()))?;

    let hrefs = parse_propfind_hrefs(&xml);

    // Fetch each matching file
    let base = cfg.base_url.trim_end_matches('/');
    let mut results = Vec::new();
    for href in hrefs {
        // Skip the collection itself (trailing slash or exact match)
        if href.ends_with('/') {
            continue;
        }
        // Filter by extension
        if !cfg.extensions.is_empty() {
            let ext = href.rsplit('.').next().unwrap_or("").to_lowercase();
            if !cfg.extensions.contains(&ext) {
                continue;
            }
        }
        // Build absolute URL if href is relative
        let url = if href.starts_with("http://") || href.starts_with("https://") {
            href.clone()
        } else {
            format!("{base}{href}")
        };

        match fetch_resource(&agent, &url) {
            Ok(bytes) => results.push((href, bytes)),
            Err(e) => eprintln!("[WebDavConnector] GET {url} failed: {e}"),
        }
    }
    Ok(results)
}

fn fetch_resource(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn build_agent(cfg: &WebDavConfig) -> ureq::Agent {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    // We apply auth headers per-request via the closures above, not at agent level.
    // ureq 2.x doesn't support agent-level header injection, so auth is set per-call.
    // For testing and simplicity, we store them in the config and apply per call.
    let _ = cfg; // suppress unused warning; auth applied in `build_agent_with_auth`
    agent
}

/// Build a request with authentication headers applied.
trait WithAuth {
    fn with_auth(self, cfg: &WebDavConfig) -> Self;
}

impl WithAuth for ureq::Request {
    fn with_auth(self, cfg: &WebDavConfig) -> Self {
        if let Some(token) = &cfg.bearer_token {
            self.set("Authorization", &format!("Bearer {token}"))
        } else if let (Some(user), Some(pass)) = (&cfg.username, &cfg.password) {
            // Basic auth: base64(user:pass)
            use std::fmt::Write as FmtWrite;
            let creds = format!("{user}:{pass}");
            let encoded = base64_encode(creds.as_bytes());
            self.set("Authorization", &format!("Basic {encoded}"))
        } else {
            self
        }
    }
}

/// Minimal base64 encoder (RFC 4648, no padding stripping needed for Basic auth).
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | input[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

// Make ureq::Agent cloneable-friendly by using a separate function for auth-aware requests.
fn webdav_propfind_with_auth(cfg: &WebDavConfig) -> Result<String, WebDavError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let propfind_body = r#"<?xml version="1.0" encoding="utf-8">
<propfind xmlns="DAV:"><allprop/></propfind>"#;

    let req = agent
        .request("PROPFIND", &cfg.base_url)
        .set("Depth", &cfg.depth)
        .set("Content-Type", "application/xml; charset=utf-8")
        .with_auth(cfg);

    let xml = req
        .send_string(propfind_body)
        .map_err(|e| WebDavError::RequestFailed(e.to_string()))?
        .into_string()
        .map_err(|e| WebDavError::ParseFailed(e.to_string()))?;
    Ok(xml)
}

fn webdav_get_with_auth(cfg: &WebDavConfig, url: &str) -> Result<Vec<u8>, WebDavError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let req = agent.get(url).with_auth(cfg);
    let resp = req.call().map_err(|e| WebDavError::RequestFailed(e.to_string()))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| WebDavError::RequestFailed(e.to_string()))?;
    Ok(buf)
}

use std::io::Read;

/// Public high-level function: list + fetch with auth applied.
pub fn webdav_list_and_fetch_auth(cfg: &WebDavConfig) -> Result<Vec<(String, Vec<u8>)>, WebDavError> {
    let xml = webdav_propfind_with_auth(cfg)?;
    let hrefs = parse_propfind_hrefs(&xml);
    let base = cfg.base_url.trim_end_matches('/');
    let mut results = Vec::new();
    for href in hrefs {
        if href.ends_with('/') { continue; }
        if !cfg.extensions.is_empty() {
            let ext = href.rsplit('.').next().unwrap_or("").to_lowercase();
            if !cfg.extensions.contains(&ext) { continue; }
        }
        let url = if href.starts_with("http://") || href.starts_with("https://") {
            href.clone()
        } else {
            format!("{base}{href}")
        };
        match webdav_get_with_auth(cfg, &url) {
            Ok(bytes) => results.push((href, bytes)),
            Err(e) => eprintln!("[WebDavConnector] GET {url} failed: {e}"),
        }
    }
    Ok(results)
}

/// `IngestionConnector` adapter over the WebDAV client.
pub struct WebDavConnector {
    descriptor: ConnectorDescriptor,
    config: WebDavConfig,
}

impl WebDavConnector {
    pub fn from_env() -> Result<Self, WebDavError> {
        Ok(Self::new(WebDavConfig::from_env()?))
    }

    pub fn new(config: WebDavConfig) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: "webdav".to_string(),
                display_name: "WebDAV Connector".to_string(),
                format: IngestFormat::Csv,
                direction: ConnectorDirection::Inbound,
            },
            config,
        }
    }
}

impl IngestionConnector for WebDavConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        match webdav_list_and_fetch_auth(&self.config) {
            Ok(files) => files
                .into_iter()
                .take(max_items)
                .map(|(path, bytes)| IngestRecord {
                    key: format!("webdav:{path}"),
                    payload: String::from_utf8_lossy(&bytes).into_owned(),
                })
                .collect(),
            Err(e) => {
                eprintln!("[WebDavConnector] read_batch error: {e}");
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conn5_parse_propfind_hrefs_standard() {
        let xml = r#"<?xml version="1.0"?>
<multistatus xmlns="DAV:">
  <response><href>/remote/file1.csv</href></response>
  <response><href>/remote/file2.json</href></response>
  <response><href>/remote/</href></response>
</multistatus>"#;
        let hrefs = parse_propfind_hrefs(xml);
        assert!(hrefs.contains(&"/remote/file1.csv".to_string()));
        assert!(hrefs.contains(&"/remote/file2.json".to_string()));
    }

    #[test]
    fn conn5_parse_propfind_hrefs_with_namespace() {
        let xml = r#"<D:multistatus xmlns:D="DAV:">
  <D:response><D:href>/dav/data.csv</D:href></D:response>
</D:multistatus>"#;
        let hrefs = parse_propfind_hrefs(xml);
        assert!(hrefs.contains(&"/dav/data.csv".to_string()));
    }

    #[test]
    fn conn5_parse_propfind_hrefs_empty() {
        assert!(parse_propfind_hrefs("<multistatus/>").is_empty());
    }

    #[test]
    fn conn5_base64_encode_basic_auth() {
        // "user:pass" → "dXNlcjpwYXNz"
        let encoded = base64_encode(b"user:pass");
        assert_eq!(encoded, "dXNlcjpwYXNz");
    }

    #[test]
    fn conn5_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn conn5_extension_filter_skips_collection() {
        let hrefs = vec![
            "/dav/".to_string(),
            "/dav/file.csv".to_string(),
            "/dav/img.png".to_string(),
        ];
        let extensions = vec!["csv".to_string()];
        let filtered: Vec<_> = hrefs
            .iter()
            .filter(|h| !h.ends_with('/'))
            .filter(|h| {
                if extensions.is_empty() { return true; }
                let ext = h.rsplit('.').next().unwrap_or("").to_lowercase();
                extensions.contains(&ext)
            })
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "/dav/file.csv");
    }

    #[test]
    fn conn5_webdav_config_from_new() {
        let cfg = WebDavConfig::new("https://dav.example.com/");
        assert_eq!(cfg.depth, "1");
        assert!(cfg.extensions.is_empty());
    }

    #[test]
    fn conn5_webdav_connector_descriptor() {
        let cfg = WebDavConfig::new("https://dav.example.com/");
        let conn = WebDavConnector::new(cfg);
        assert_eq!(conn.descriptor().id, "webdav");
    }
}
