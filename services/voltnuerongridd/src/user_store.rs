//! Gap #7: User accounts, bcrypt password hashing, HMAC-SHA256 session tokens.
//!
//! Design:
//! - `UserStore`  — in-memory map of user_id → UserAccount, replayed from DDL WAL at boot.
//! - `SessionStore` — in-memory map of token_hash → SessionEntry (TTL checked on read).
//! - `SessionSigner` — HMAC-SHA256 signer; secret is derived from VNG_CLUSTER_TOKEN at start.
//! - WAL format: `CREATE USER <username>\t<role>\t<tenant_or_null>\t<user_id>\t<created_ms>\t<bcrypt_hash>`
//!
//! Tokens: `base64url(user_id:expires_secs) "." base64url(hmac_sha256(header.secret))`

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

/// A single registered user account.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UserAccount {
    pub(crate) user_id: String,
    pub(crate) username: String,
    /// "dba", "operator", "tenant_user"
    pub(crate) role: String,
    /// Optional tenant id for tenant_user role
    pub(crate) tenant_id: Option<String>,
    pub(crate) created_ms: u64,
    /// bcrypt hash of the password (cost 12)
    pub(crate) password_hash: String,
}

/// In-memory user store; key = lowercase username.
#[derive(Default)]
pub(crate) struct UserStore {
    by_username: HashMap<String, UserAccount>,
    by_id: HashMap<String, String>, // user_id → username
}

impl UserStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&mut self, account: UserAccount) {
        self.by_id.insert(account.user_id.clone(), account.username.clone());
        self.by_username.insert(account.username.to_ascii_lowercase(), account);
    }

    pub(crate) fn get_by_username(&self, username: &str) -> Option<&UserAccount> {
        self.by_username.get(&username.to_ascii_lowercase())
    }

    pub(crate) fn get_by_id(&self, user_id: &str) -> Option<&UserAccount> {
        let username = self.by_id.get(user_id)?;
        self.by_username.get(username)
    }

    pub(crate) fn remove_by_id(&mut self, user_id: &str) -> bool {
        if let Some(username) = self.by_id.remove(user_id) {
            self.by_username.remove(&username);
            return true;
        }
        false
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &UserAccount> {
        self.by_username.values()
    }
}

/// An active session.
#[derive(Clone, Debug)]
pub(crate) struct SessionEntry {
    pub(crate) user_id: String,
    #[allow(dead_code)] // stored for future audit trail / session inspection
    pub(crate) username: String,
    pub(crate) role: String,
    pub(crate) tenant_id: Option<String>,
    pub(crate) expires_at_secs: u64,
}

/// In-memory session store; key = sha256(raw_token) hex string.
#[derive(Default)]
pub(crate) struct SessionStore {
    sessions: HashMap<String, SessionEntry>,
}

impl SessionStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&mut self, token_fingerprint: String, entry: SessionEntry) {
        self.sessions.insert(token_fingerprint, entry);
    }

    pub(crate) fn lookup(&self, token_fingerprint: &str) -> Option<&SessionEntry> {
        let entry = self.sessions.get(token_fingerprint)?;
        let now = now_secs();
        if entry.expires_at_secs < now {
            return None; // expired (will be cleaned up lazily)
        }
        Some(entry)
    }

    pub(crate) fn remove_by_user(&mut self, user_id: &str) {
        self.sessions.retain(|_, v| v.user_id != user_id);
    }

    /// Remove a single session by its token fingerprint.
    /// Used by token rotation to invalidate the old token atomically.
    pub(crate) fn remove_by_fingerprint(&mut self, token_fingerprint: &str) {
        self.sessions.remove(token_fingerprint);
    }

    /// Return all active (non-expired) session fingerprints for a given user.
    pub(crate) fn sessions_for_user(&self, user_id: &str) -> Vec<String> {
        let now = now_secs();
        self.sessions
            .iter()
            .filter(|(_, v)| v.user_id == user_id && v.expires_at_secs >= now)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// HMAC-SHA256 session token signer.
///
/// Token format:
/// `<header_b64>.<sig_b64>` where
/// `header_b64 = base64url("{user_id}:{expires_secs}")` and
/// `sig_b64 = base64url(hmac_sha256(header_b64, secret))`
pub(crate) struct SessionSigner {
    secret: Vec<u8>,
    /// Session TTL in seconds (default 24 h)
    pub(crate) ttl_secs: u64,
}

impl SessionSigner {
    pub(crate) fn new(secret: &str, ttl_secs: u64) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
            ttl_secs,
        }
    }

    /// Issue a new signed session token for the given user.
    pub(crate) fn issue(&self, user_id: &str) -> String {
        let expires_at = now_secs() + self.ttl_secs;
        let header = format!("{user_id}:{expires_at}");
        let header_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
        let sig_b64 = self.sign(&header_b64);
        format!("{header_b64}.{sig_b64}")
    }

    /// Verify a token. Returns `(user_id, expires_at_secs)` if valid, else `None`.
    pub(crate) fn verify(&self, token: &str) -> Option<(String, u64)> {
        let (header_b64, sig_b64) = token.split_once('.')?;
        // Verify signature
        let expected_sig = self.sign(header_b64);
        if expected_sig != sig_b64 {
            return None;
        }
        // Decode header
        let header_bytes = URL_SAFE_NO_PAD.decode(header_b64).ok()?;
        let header = String::from_utf8(header_bytes).ok()?;
        let (user_id, expires_str) = header.split_once(':')?;
        let expires_at: u64 = expires_str.parse().ok()?;
        // Check expiry
        if expires_at < now_secs() {
            return None;
        }
        Some((user_id.to_string(), expires_at))
    }

    /// Compute a stable fingerprint for a raw token (for SessionStore key).
    pub(crate) fn fingerprint(token: &str) -> String {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(token.as_bytes());
        hex_encode(&hash)
    }

    fn sign(&self, data: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC accepts any key length");
        mac.update(data.as_bytes());
        let result = mac.finalize().into_bytes();
        URL_SAFE_NO_PAD.encode(&result)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Serialize a `UserAccount` to WAL format.
/// Format: `CREATE USER <username>\t<role>\t<tenant_or_null>\t<user_id>\t<created_ms>\t<bcrypt_hash>`
pub(crate) fn user_to_wal(account: &UserAccount) -> String {
    let tenant = account.tenant_id.as_deref().unwrap_or("null");
    format!(
        "CREATE USER {}\t{}\t{}\t{}\t{}\t{}",
        account.username,
        account.role,
        tenant,
        account.user_id,
        account.created_ms,
        account.password_hash,
    )
}

/// Parse a WAL line back into a `UserAccount`.
pub(crate) fn user_from_wal(line: &str) -> Option<UserAccount> {
    // "CREATE USER " prefix
    let rest = line.strip_prefix("CREATE USER ")?;
    let parts: Vec<&str> = rest.splitn(6, '\t').collect();
    if parts.len() != 6 {
        return None;
    }
    let username = parts[0].to_string();
    let role = parts[1].to_string();
    let tenant_id = if parts[2] == "null" { None } else { Some(parts[2].to_string()) };
    let user_id = parts[3].to_string();
    let created_ms: u64 = parts[4].parse().ok()?;
    let password_hash = parts[5].to_string();
    Some(UserAccount { user_id, username, role, tenant_id, created_ms, password_hash })
}
