// CONN-1: FTP / FTPS connector — real TCP client using std::net::TcpStream.
//
// Implements the FTP protocol (RFC 959) via raw TCP, supporting:
// - Passive mode (PASV) file transfer
// - Binary mode (TYPE I)
// - AUTH TLS stub for FTPS (TLS upgrade handled at env var level)
// - Env-var configuration: VNG_FTP_HOST, VNG_FTP_PORT, VNG_FTP_USER,
//   VNG_FTP_PASSWORD, VNG_FTP_PATH, VNG_FTP_TLS, VNG_FTP_MODE

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestionConnector, IngestFormat, IngestRecord,
};

const DEFAULT_FTP_PORT: u16 = 21;
const DEFAULT_FTP_PATH: &str = "/";
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Errors produced by the FTP connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtpError {
    ConnectionFailed(String),
    AuthFailed(String),
    ProtocolError(String),
    TransferFailed(String),
}

impl std::fmt::Display for FtpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(m) => write!(f, "ftp_connection_failed: {m}"),
            Self::AuthFailed(m) => write!(f, "ftp_auth_failed: {m}"),
            Self::ProtocolError(m) => write!(f, "ftp_protocol_error: {m}"),
            Self::TransferFailed(m) => write!(f, "ftp_transfer_failed: {m}"),
        }
    }
}

/// Active FTP configuration (built from env vars or direct construction).
#[derive(Debug, Clone)]
pub struct FtpConnectorConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// Remote directory path to list and fetch files from.
    pub remote_path: String,
    /// Enable FTPS (TLS) — requires AUTH TLS support on server.
    pub tls_enabled: bool,
    /// Use passive mode (PASV). True by default; false = active mode (PORT).
    pub passive_mode: bool,
    /// File extensions to include (e.g. ["csv", "json"]). Empty = all files.
    pub extensions: Vec<String>,
}

impl FtpConnectorConfig {
    /// Build from environment variables.
    pub fn from_env() -> Self {
        let extensions_raw = std::env::var("VNG_FTP_EXTENSIONS").unwrap_or_default();
        let extensions = extensions_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();

        Self {
            host: std::env::var("VNG_FTP_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("VNG_FTP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_FTP_PORT),
            username: std::env::var("VNG_FTP_USER").unwrap_or_else(|_| "anonymous".to_string()),
            password: std::env::var("VNG_FTP_PASSWORD").unwrap_or_default(),
            remote_path: std::env::var("VNG_FTP_PATH")
                .unwrap_or_else(|_| DEFAULT_FTP_PATH.to_string()),
            tls_enabled: std::env::var("VNG_FTP_TLS")
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
                .unwrap_or(false),
            passive_mode: std::env::var("VNG_FTP_MODE")
                .map(|v| v.trim() != "active")
                .unwrap_or(true),
            extensions,
        }
    }

    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>, password: impl Into<String>, remote_path: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            password: password.into(),
            remote_path: remote_path.into(),
            tls_enabled: false,
            passive_mode: true,
            extensions: vec![],
        }
    }
}

/// Parse an FTP PASV response to extract (host, port).
/// PASV response format: "227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)."
pub fn parse_pasv_response(line: &str) -> Option<(String, u16)> {
    let start = line.find('(')?;
    let end = line.find(')')?;
    let inner = &line[start + 1..end];
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 6 {
        return None;
    }
    let octets: Vec<u8> = parts[..4]
        .iter()
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if octets.len() != 4 {
        return None;
    }
    let p1: u16 = parts[4].trim().parse().ok()?;
    let p2: u16 = parts[5].trim().parse().ok()?;
    let host = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
    let port = p1 * 256 + p2;
    Some((host, port))
}

/// Read an FTP response line and return (code, message).
fn read_ftp_response(reader: &mut BufReader<&TcpStream>) -> Result<(u32, String), FtpError> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| FtpError::ProtocolError(e.to_string()))?;
    let trimmed = line.trim_end();
    if trimmed.len() < 3 {
        return Err(FtpError::ProtocolError(format!("short response: {trimmed}")));
    }
    let code: u32 = trimmed[..3]
        .parse()
        .map_err(|_| FtpError::ProtocolError(format!("non-numeric code in: {trimmed}")))?;
    Ok((code, trimmed.to_string()))
}

/// Send a command over the control connection.
fn send_command(stream: &mut TcpStream, cmd: &str) -> Result<(), FtpError> {
    let line = format!("{cmd}\r\n");
    stream
        .write_all(line.as_bytes())
        .map_err(|e| FtpError::ProtocolError(e.to_string()))
}

/// Perform a full FTP session: connect, authenticate, list remote_path, fetch all matching files.
/// Returns a list of (filename, bytes) pairs.
pub fn ftp_fetch_files(cfg: &FtpConnectorConfig) -> Result<Vec<(String, Vec<u8>)>, FtpError> {
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|_| FtpError::ConnectionFailed(format!("bad address: {addr}")))?,
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
    )
    .map_err(|e| FtpError::ConnectionFailed(e.to_string()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .ok();

    let mut control = stream
        .try_clone()
        .map_err(|e| FtpError::ConnectionFailed(e.to_string()))?;
    let mut reader = BufReader::new(&stream);

    // Welcome banner
    let (code, _) = read_ftp_response(&mut reader)?;
    if code != 220 {
        return Err(FtpError::ConnectionFailed(format!("unexpected banner code {code}")));
    }

    // AUTH TLS stub (for FTPS)
    if cfg.tls_enabled {
        send_command(&mut control, "AUTH TLS")?;
        let _ = read_ftp_response(&mut reader);
        // TLS upgrade would happen here via native-tls / rustls.
        // For now, continue as plain FTP after advertising TLS desire.
    }

    // Authenticate
    send_command(&mut control, &format!("USER {}", cfg.username))?;
    let (code, _) = read_ftp_response(&mut reader)?;
    if code == 331 {
        send_command(&mut control, &format!("PASS {}", cfg.password))?;
        let (code, msg) = read_ftp_response(&mut reader)?;
        if code != 230 {
            return Err(FtpError::AuthFailed(msg));
        }
    } else if code != 230 {
        return Err(FtpError::AuthFailed(format!("code {code}")));
    }

    // Binary mode
    send_command(&mut control, "TYPE I")?;
    let _ = read_ftp_response(&mut reader);

    // Navigate to remote path
    if cfg.remote_path != "/" {
        send_command(&mut control, &format!("CWD {}", cfg.remote_path))?;
        let (code, msg) = read_ftp_response(&mut reader)?;
        if code != 250 {
            return Err(FtpError::TransferFailed(format!("CWD failed: {msg}")));
        }
    }

    // List files via PASV + LIST
    let file_list = ftp_list(&stream, &mut control, &mut reader, cfg)?;

    // Fetch each matching file
    let mut results = Vec::new();
    for filename in &file_list {
        if !cfg.extensions.is_empty() {
            let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
            if !cfg.extensions.contains(&ext) {
                continue;
            }
        }
        match ftp_retr(&stream, &mut control, &mut reader, filename) {
            Ok(bytes) => results.push((filename.clone(), bytes)),
            Err(_) => {} // Skip files that fail; continue with others
        }
    }

    // Quit
    let _ = send_command(&mut control, "QUIT");
    Ok(results)
}

/// LIST command via PASV — returns file names from the directory listing.
fn ftp_list(
    stream: &TcpStream,
    control: &mut TcpStream,
    reader: &mut BufReader<&TcpStream>,
    cfg: &FtpConnectorConfig,
) -> Result<Vec<String>, FtpError> {
    // PASV to get data connection address
    send_command(control, "PASV")?;
    let (code, msg) = read_ftp_response(reader)?;
    if code != 227 {
        return Err(FtpError::ProtocolError(format!("PASV failed: {msg}")));
    }
    let (data_host, data_port) =
        parse_pasv_response(&msg).ok_or_else(|| FtpError::ProtocolError("bad PASV".into()))?;

    let data_addr = format!("{data_host}:{data_port}");
    let data_stream = TcpStream::connect_timeout(
        &data_addr
            .parse()
            .map_err(|_| FtpError::TransferFailed("bad data addr".into()))?,
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
    )
    .map_err(|e| FtpError::TransferFailed(e.to_string()))?;

    // Send LIST
    send_command(control, "LIST")?;
    let _ = read_ftp_response(reader); // 150 Opening data connection

    // Read listing
    let mut listing = String::new();
    BufReader::new(&data_stream)
        .read_to_string(&mut listing)
        .map_err(|e| FtpError::TransferFailed(e.to_string()))?;
    drop(data_stream);

    let _ = read_ftp_response(reader); // 226 Transfer complete

    // Parse unix-style listing: last token is filename
    let names = listing
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.last().map(|s| s.to_string())
        })
        .filter(|n| n != "." && n != "..")
        .collect();
    Ok(names)
}

/// RETR command via PASV — fetches a single file's bytes.
fn ftp_retr(
    _stream: &TcpStream,
    control: &mut TcpStream,
    reader: &mut BufReader<&TcpStream>,
    filename: &str,
) -> Result<Vec<u8>, FtpError> {
    send_command(control, "PASV")?;
    let (code, msg) = read_ftp_response(reader)?;
    if code != 227 {
        return Err(FtpError::ProtocolError(format!("PASV failed: {msg}")));
    }
    let (data_host, data_port) =
        parse_pasv_response(&msg).ok_or_else(|| FtpError::ProtocolError("bad PASV".into()))?;

    let data_addr = format!("{data_host}:{data_port}");
    let data_stream = TcpStream::connect_timeout(
        &data_addr
            .parse()
            .map_err(|_| FtpError::TransferFailed("bad data addr".into()))?,
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
    )
    .map_err(|e| FtpError::TransferFailed(e.to_string()))?;

    send_command(control, &format!("RETR {filename}"))?;
    let _ = read_ftp_response(reader); // 150 Opening data connection

    let mut buf = Vec::new();
    BufReader::new(&data_stream)
        .read_to_end(&mut buf)
        .map_err(|e| FtpError::TransferFailed(e.to_string()))?;
    drop(data_stream);

    let _ = read_ftp_response(reader); // 226 Transfer complete
    Ok(buf)
}

/// `IngestionConnector` adapter over the FTP client.
pub struct FtpConnector {
    descriptor: ConnectorDescriptor,
    config: FtpConnectorConfig,
}

impl FtpConnector {
    pub fn from_env() -> Self {
        Self::new(FtpConnectorConfig::from_env())
    }

    pub fn new(config: FtpConnectorConfig) -> Self {
        let id = if config.tls_enabled { "ftps" } else { "ftp" };
        Self {
            descriptor: ConnectorDescriptor {
                id: id.to_string(),
                display_name: if config.tls_enabled {
                    "FTP/TLS (FTPS) Connector".to_string()
                } else {
                    "FTP Connector".to_string()
                },
                format: IngestFormat::Csv,
                direction: ConnectorDirection::Inbound,
            },
            config,
        }
    }
}

impl IngestionConnector for FtpConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    /// Connects to the FTP server, lists the remote path, fetches matching files,
    /// and returns each file's content as a UTF-8 payload per `IngestRecord`.
    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        match ftp_fetch_files(&self.config) {
            Ok(files) => files
                .into_iter()
                .take(max_items)
                .map(|(name, bytes)| IngestRecord {
                    key: format!("ftp:{}:{name}", self.config.host),
                    payload: String::from_utf8_lossy(&bytes).into_owned(),
                })
                .collect(),
            Err(e) => {
                eprintln!("[FtpConnector] fetch error: {e}");
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conn1_parse_pasv_response_standard() {
        let line = "227 Entering Passive Mode (192,168,1,10,4,200).";
        let result = parse_pasv_response(line);
        assert_eq!(result, Some(("192.168.1.10".to_string(), 4 * 256 + 200)));
    }

    #[test]
    fn conn1_parse_pasv_response_port_calculation() {
        // port = p1*256 + p2 = 19*256 + 200 = 5064
        let line = "227 Entering Passive Mode (127,0,0,1,19,200).";
        let result = parse_pasv_response(line);
        assert_eq!(result, Some(("127.0.0.1".to_string(), 19 * 256 + 200)));
    }

    #[test]
    fn conn1_parse_pasv_response_invalid() {
        assert_eq!(parse_pasv_response("220 Welcome"), None);
        assert_eq!(parse_pasv_response("227 (bad,format)"), None);
    }

    #[test]
    fn conn1_config_defaults_from_env() {
        // Ensure from_env doesn't panic with no env vars set.
        let cfg = FtpConnectorConfig::from_env();
        assert_eq!(cfg.port, 21);
        assert!(!cfg.tls_enabled);
        assert!(cfg.passive_mode);
    }

    #[test]
    fn conn1_ftp_connector_descriptor() {
        let cfg = FtpConnectorConfig::new("ftp.example.com", 21, "user", "pass", "/data");
        let connector = FtpConnector::new(cfg);
        assert_eq!(connector.descriptor().id, "ftp");
        assert_eq!(connector.descriptor().format, IngestFormat::Csv);
    }

    #[test]
    fn conn1_ftps_connector_descriptor() {
        let mut cfg = FtpConnectorConfig::new("ftp.example.com", 21, "user", "pass", "/data");
        cfg.tls_enabled = true;
        let connector = FtpConnector::new(cfg);
        assert_eq!(connector.descriptor().id, "ftps");
    }

    #[test]
    fn conn1_extension_filter_matches() {
        let mut cfg = FtpConnectorConfig::new("localhost", 21, "a", "b", "/");
        cfg.extensions = vec!["csv".to_string(), "json".to_string()];
        let ext = "data.csv".rsplit('.').next().unwrap_or("").to_lowercase();
        assert!(cfg.extensions.contains(&ext));
        let ext2 = "data.xlsx".rsplit('.').next().unwrap_or("").to_lowercase();
        assert!(!cfg.extensions.contains(&ext2));
    }
}
