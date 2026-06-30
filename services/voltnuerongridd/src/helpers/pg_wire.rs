//! D-1: PostgreSQL v3 wire-protocol front-end (simple query protocol).
//!
//! This module implements the message codec for the subset of the PostgreSQL
//! frontend/backend protocol needed to let BI tools, `psql`, and JDBC/ODBC
//! Postgres drivers connect and run `SELECT`/`INSERT`/DDL against the existing
//! VoltNueronGrid SQL engine:
//!
//! Frontend → backend:  StartupMessage, PasswordMessage, Query, Terminate
//! Backend  → frontend:  AuthenticationOk, ParameterStatus, BackendKeyData,
//!                       ReadyForQuery, RowDescription, DataRow, CommandComplete,
//!                       ErrorResponse
//!
//! The codec here is pure and unit-tested; the async TCP listener that drives it
//! lives in `pg_listener.rs`. Live `psql`/driver smoke validation is tracked
//! under E-5.

/// PostgreSQL protocol version 3.0 encoded as a single i32 (`0x00030000`).
pub(crate) const PROTOCOL_VERSION_3: i32 = 196608;

/// SSLRequest magic code sent by clients probing for TLS (`80877103`).
pub(crate) const SSL_REQUEST_CODE: i32 = 80877103;

/// Transaction status reported in `ReadyForQuery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionStatus {
    /// Idle (not in a transaction block).
    Idle,
    /// In a transaction block.
    InTransaction,
    /// In a failed transaction block.
    Failed,
}

impl TransactionStatus {
    pub(crate) fn byte(self) -> u8 {
        match self {
            TransactionStatus::Idle => b'I',
            TransactionStatus::InTransaction => b'T',
            TransactionStatus::Failed => b'E',
        }
    }
}

/// A parsed startup message: the protocol version plus the key/value parameters
/// the client sent (`user`, `database`, `application_name`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupMessage {
    pub(crate) protocol_version: i32,
    pub(crate) parameters: Vec<(String, String)>,
}

impl StartupMessage {
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Result of decoding the leading startup packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupDecode {
    /// A real startup message with version + parameters.
    Startup(StartupMessage),
    /// Client requested SSL negotiation (the 8-byte SSLRequest).
    SslRequest,
    /// Not enough bytes yet — need to read more before decoding.
    Incomplete,
}

/// Decode the startup packet (no message-type byte; a 4-byte length prefix
/// followed by either the SSLRequest code or a protocol version + parameters).
pub(crate) fn decode_startup(buf: &[u8]) -> Result<StartupDecode, String> {
    if buf.len() < 4 {
        return Ok(StartupDecode::Incomplete);
    }
    let len = i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len < 8 {
        return Err(format!("startup length too small: {len}"));
    }
    if buf.len() < len {
        return Ok(StartupDecode::Incomplete);
    }
    let code = i32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if code == SSL_REQUEST_CODE {
        return Ok(StartupDecode::SslRequest);
    }
    // Parameters are NUL-terminated key/value C-strings until a final empty key.
    let mut params = Vec::new();
    let body = &buf[8..len];
    let mut i = 0usize;
    loop {
        // Read key.
        let Some(key_end) = body[i..].iter().position(|&b| b == 0) else {
            break;
        };
        let key = String::from_utf8_lossy(&body[i..i + key_end]).to_string();
        i += key_end + 1;
        if key.is_empty() {
            break;
        }
        // Read value.
        let Some(val_end) = body[i..].iter().position(|&b| b == 0) else {
            return Err("startup parameter value not NUL-terminated".to_string());
        };
        let value = String::from_utf8_lossy(&body[i..i + val_end]).to_string();
        i += val_end + 1;
        params.push((key, value));
    }
    Ok(StartupDecode::Startup(StartupMessage {
        protocol_version: code,
        parameters: params,
    }))
}

/// A decoded regular (typed) frontend message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrontendMessage {
    /// `Q` — simple query.
    Query(String),
    /// `p` — password message (cleartext or md5).
    Password(String),
    /// `X` — terminate.
    Terminate,
    /// Any other message type, captured by its tag byte.
    Other(u8),
}

/// Decode a single typed frontend message from `buf`. Returns `(message,
/// bytes_consumed)` or `None` when more bytes are needed.
pub(crate) fn decode_frontend_message(buf: &[u8]) -> Result<Option<(FrontendMessage, usize)>, String> {
    if buf.len() < 5 {
        return Ok(None);
    }
    let tag = buf[0];
    let len = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
    if len < 4 {
        return Err(format!("frontend message length too small: {len}"));
    }
    let total = 1 + len;
    if buf.len() < total {
        return Ok(None);
    }
    let body = &buf[5..total];
    let msg = match tag {
        b'Q' => {
            // NUL-terminated query string.
            let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
            FrontendMessage::Query(String::from_utf8_lossy(&body[..end]).to_string())
        }
        b'p' => {
            let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
            FrontendMessage::Password(String::from_utf8_lossy(&body[..end]).to_string())
        }
        b'X' => FrontendMessage::Terminate,
        other => FrontendMessage::Other(other),
    };
    Ok(Some((msg, total)))
}

// ───────────────────────── backend message encoders ─────────────────────────

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_i16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

/// Frame a backend message: `tag` byte + i32 length (covering length + payload).
fn frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(tag);
    put_i32(&mut out, (payload.len() + 4) as i32);
    out.extend_from_slice(payload);
    out
}

/// `R` AuthenticationOk (auth type 0).
pub(crate) fn encode_authentication_ok() -> Vec<u8> {
    let mut payload = Vec::new();
    put_i32(&mut payload, 0);
    frame(b'R', &payload)
}

/// `R` AuthenticationCleartextPassword (auth type 3).
pub(crate) fn encode_authentication_cleartext_password() -> Vec<u8> {
    let mut payload = Vec::new();
    put_i32(&mut payload, 3);
    frame(b'R', &payload)
}

/// `S` ParameterStatus key/value.
pub(crate) fn encode_parameter_status(key: &str, value: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    put_cstr(&mut payload, key);
    put_cstr(&mut payload, value);
    frame(b'S', &payload)
}

/// `K` BackendKeyData (process id + secret key).
pub(crate) fn encode_backend_key_data(process_id: i32, secret_key: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    put_i32(&mut payload, process_id);
    put_i32(&mut payload, secret_key);
    frame(b'K', &payload)
}

/// `Z` ReadyForQuery with the current transaction status.
pub(crate) fn encode_ready_for_query(status: TransactionStatus) -> Vec<u8> {
    frame(b'Z', &[status.byte()])
}

/// One column descriptor for `RowDescription`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldDescription {
    pub(crate) name: String,
    /// PostgreSQL type OID (`25` = text by default).
    pub(crate) type_oid: i32,
}

impl FieldDescription {
    pub(crate) fn text(name: impl Into<String>) -> Self {
        Self { name: name.into(), type_oid: 25 }
    }
}

/// `T` RowDescription for the given columns. All columns are advertised as text
/// (format code 0), which BI tools and Postgres drivers handle natively.
pub(crate) fn encode_row_description(fields: &[FieldDescription]) -> Vec<u8> {
    let mut payload = Vec::new();
    put_i16(&mut payload, fields.len() as i16);
    for f in fields {
        put_cstr(&mut payload, &f.name);
        put_i32(&mut payload, 0); // table OID
        put_i16(&mut payload, 0); // column attribute number
        put_i32(&mut payload, f.type_oid); // type OID
        put_i16(&mut payload, -1); // type size (variable)
        put_i32(&mut payload, -1); // type modifier
        put_i16(&mut payload, 0); // format code (0 = text)
    }
    frame(b'T', &payload)
}

/// `D` DataRow. `None` cells encode as SQL NULL (length -1).
pub(crate) fn encode_data_row(cells: &[Option<String>]) -> Vec<u8> {
    let mut payload = Vec::new();
    put_i16(&mut payload, cells.len() as i16);
    for cell in cells {
        match cell {
            Some(s) => {
                put_i32(&mut payload, s.len() as i32);
                payload.extend_from_slice(s.as_bytes());
            }
            None => put_i32(&mut payload, -1),
        }
    }
    frame(b'D', &payload)
}

/// `C` CommandComplete with a command tag (e.g. `SELECT 3`, `INSERT 0 1`).
pub(crate) fn encode_command_complete(tag: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    put_cstr(&mut payload, tag);
    frame(b'C', &payload)
}

/// `E` ErrorResponse (severity ERROR, SQLSTATE, message).
pub(crate) fn encode_error_response(sqlstate: &str, message: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(b'S');
    put_cstr(&mut payload, "ERROR");
    payload.push(b'C');
    put_cstr(&mut payload, sqlstate);
    payload.push(b'M');
    put_cstr(&mut payload, message);
    payload.push(0); // terminator
    frame(b'E', &payload)
}

/// `N` EmptyQueryResponse.
pub(crate) fn encode_empty_query_response() -> Vec<u8> {
    frame(b'I', &[])
}

/// Derive the PostgreSQL CommandComplete tag for a SQL statement + affected/row count.
pub(crate) fn command_tag(sql: &str, row_count: usize) -> String {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("SELECT") {
        format!("SELECT {row_count}")
    } else if upper.starts_with("INSERT") {
        format!("INSERT 0 {row_count}")
    } else if upper.starts_with("UPDATE") {
        format!("UPDATE {row_count}")
    } else if upper.starts_with("DELETE") {
        format!("DELETE {row_count}")
    } else if upper.starts_with("CREATE") {
        "CREATE".to_string()
    } else if upper.starts_with("DROP") {
        "DROP".to_string()
    } else if upper.starts_with("BEGIN") || upper.starts_with("START") {
        "BEGIN".to_string()
    } else if upper.starts_with("COMMIT") {
        "COMMIT".to_string()
    } else if upper.starts_with("ROLLBACK") {
        "ROLLBACK".to_string()
    } else {
        // Generic acknowledgement for other statements.
        upper.split_whitespace().next().unwrap_or("OK").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_startup(params: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&PROTOCOL_VERSION_3.to_be_bytes());
        for (k, v) in params {
            body.extend_from_slice(k.as_bytes());
            body.push(0);
            body.extend_from_slice(v.as_bytes());
            body.push(0);
        }
        body.push(0); // final terminator
        let mut msg = Vec::new();
        msg.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        msg.extend_from_slice(&body);
        msg
    }

    #[test]
    fn decode_startup_parses_parameters() {
        let buf = build_startup(&[("user", "analyst"), ("database", "acme")]);
        match decode_startup(&buf).unwrap() {
            StartupDecode::Startup(msg) => {
                assert_eq!(msg.protocol_version, PROTOCOL_VERSION_3);
                assert_eq!(msg.get("user"), Some("analyst"));
                assert_eq!(msg.get("database"), Some("acme"));
                assert_eq!(msg.get("missing"), None);
            }
            other => panic!("expected startup, got {other:?}"),
        }
    }

    #[test]
    fn decode_startup_detects_ssl_request() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&8i32.to_be_bytes());
        buf.extend_from_slice(&SSL_REQUEST_CODE.to_be_bytes());
        assert_eq!(decode_startup(&buf).unwrap(), StartupDecode::SslRequest);
    }

    #[test]
    fn decode_startup_incomplete_when_short() {
        let buf = build_startup(&[("user", "x")]);
        assert_eq!(decode_startup(&buf[..6]).unwrap(), StartupDecode::Incomplete);
    }

    #[test]
    fn decode_query_message() {
        let sql = "SELECT 1";
        let mut body = sql.as_bytes().to_vec();
        body.push(0);
        let mut msg = vec![b'Q'];
        msg.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        msg.extend_from_slice(&body);
        let (decoded, consumed) = decode_frontend_message(&msg).unwrap().unwrap();
        assert_eq!(decoded, FrontendMessage::Query(sql.to_string()));
        assert_eq!(consumed, msg.len());
    }

    #[test]
    fn decode_terminate_and_password() {
        let mut term = vec![b'X'];
        term.extend_from_slice(&4i32.to_be_bytes());
        assert_eq!(
            decode_frontend_message(&term).unwrap().unwrap().0,
            FrontendMessage::Terminate
        );

        let mut body = b"secret".to_vec();
        body.push(0);
        let mut pw = vec![b'p'];
        pw.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        pw.extend_from_slice(&body);
        assert_eq!(
            decode_frontend_message(&pw).unwrap().unwrap().0,
            FrontendMessage::Password("secret".to_string())
        );
    }

    #[test]
    fn decode_frontend_incomplete_returns_none() {
        let msg = vec![b'Q', 0, 0, 0]; // truncated header
        assert!(decode_frontend_message(&msg).unwrap().is_none());
    }

    #[test]
    fn auth_and_ready_frames_have_correct_tags() {
        assert_eq!(encode_authentication_ok()[0], b'R');
        assert_eq!(encode_authentication_cleartext_password()[0], b'R');
        let z = encode_ready_for_query(TransactionStatus::Idle);
        assert_eq!(z[0], b'Z');
        assert_eq!(*z.last().unwrap(), b'I');
    }

    #[test]
    fn row_description_roundtrip_shape() {
        let fields = vec![FieldDescription::text("id"), FieldDescription::text("name")];
        let buf = encode_row_description(&fields);
        assert_eq!(buf[0], b'T');
        // Field count is the first i16 of the payload (after tag + length).
        let count = i16::from_be_bytes([buf[5], buf[6]]);
        assert_eq!(count, 2);
    }

    #[test]
    fn data_row_encodes_null_as_minus_one() {
        let buf = encode_data_row(&[Some("a".to_string()), None]);
        assert_eq!(buf[0], b'D');
        let count = i16::from_be_bytes([buf[5], buf[6]]);
        assert_eq!(count, 2);
    }

    #[test]
    fn command_tags_match_postgres_conventions() {
        assert_eq!(command_tag("SELECT * FROM t", 3), "SELECT 3");
        assert_eq!(command_tag("INSERT INTO t VALUES (1)", 1), "INSERT 0 1");
        assert_eq!(command_tag("UPDATE t SET x=1", 2), "UPDATE 2");
        assert_eq!(command_tag("DELETE FROM t", 4), "DELETE 4");
        assert_eq!(command_tag("CREATE TABLE t (id INT)", 0), "CREATE");
        assert_eq!(command_tag("BEGIN", 0), "BEGIN");
    }

    #[test]
    fn error_response_has_sqlstate_and_message() {
        let buf = encode_error_response("42601", "syntax error");
        assert_eq!(buf[0], b'E');
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("42601"));
        assert!(s.contains("syntax error"));
    }
}
