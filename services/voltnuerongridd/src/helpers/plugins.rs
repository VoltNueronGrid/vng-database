//! Versioned plugin marketplace registry.
//!
//! Supports install, upgrade, downgrade, uninstall, and list operations.
//! The full install history (every version ever installed for a plugin ID)
//! is persisted to `state/plugin-registry.json` so that downgrade is always
//! possible as long as the prior version was once installed.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ── Semantic version ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct SemVer {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) patch: u32,
}

impl SemVer {
    /// Parse `"major.minor.patch"`.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
        if parts.len() < 3 {
            return None;
        }
        Some(Self {
            major: parts[0],
            minor: parts[1],
            patch: parts[2],
        })
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ── Plugin entry ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PluginState {
    Active,
    Disabled,
    Uninstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PluginEntry {
    /// Stable plugin identifier (e.g. `"connector.kafka"`).
    pub(crate) id: String,
    /// Human-readable name.
    pub(crate) name: String,
    /// Semantic version string (`"1.2.3"`).
    pub(crate) version: String,
    /// SHA-256 hex digest of the plugin archive.
    pub(crate) checksum_sha256: String,
    /// Whether the plugin manifest carries a valid signature.
    pub(crate) signed: bool,
    /// Unix epoch milliseconds when this version was installed.
    pub(crate) installed_at_ms: u64,
    pub(crate) state: PluginState,
}

// ── Persisted state ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct RegistryState {
    /// `plugin_id` → ordered list of all versions ever installed (oldest first).
    history: HashMap<String, Vec<PluginEntry>>,
    /// `plugin_id` → currently active version string.
    current: HashMap<String, String>,
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Thread-safe versioned plugin registry.
#[derive(Default)]
pub(crate) struct PluginRegistry {
    state: RegistryState,
}

impl PluginRegistry {
    /// Create a new registry, loading persisted state from disk if available.
    pub(crate) fn new() -> Self {
        let mut reg = Self::default();
        if let Ok(data) = std::fs::read_to_string("state/plugin-registry.json") {
            if let Ok(saved) = serde_json::from_str::<RegistryState>(&data) {
                reg.state = saved;
            }
        }
        reg
    }

    /// Create a fresh in-memory registry without loading any persisted state.
    /// Used in tests to avoid disk state leaking across test runs.
    #[cfg(test)]
    pub(crate) fn new_empty() -> Self {
        Self::default()
    }

    // ── Mutation operations ───────────────────────────────────────────────────

    /// Install a new plugin.
    ///
    /// Fails if the exact same `(id, version)` is already active.
    pub(crate) fn install(&mut self, entry: PluginEntry) -> Result<(), String> {
        let id = entry.id.clone();
        let version = entry.version.clone();
        if SemVer::parse(&version).is_none() {
            return Err(format!("invalid_semver: '{version}'"));
        }
        if self
            .state
            .current
            .get(&id)
            .map(|v| v == &version)
            .unwrap_or(false)
        {
            return Err(format!("already_installed: {id}@{version}"));
        }
        self.state
            .history
            .entry(id.clone())
            .or_default()
            .push(entry);
        self.state.current.insert(id, version);
        self.persist();
        Ok(())
    }

    /// Upgrade a plugin to a strictly newer version.
    pub(crate) fn upgrade(&mut self, id: &str, new_entry: PluginEntry) -> Result<(), String> {
        let current_ver = self
            .state
            .current
            .get(id)
            .ok_or_else(|| format!("plugin_not_installed: {id}"))?
            .clone();
        let cur_sv = SemVer::parse(&current_ver).ok_or("invalid_current_version")?;
        let new_sv = SemVer::parse(&new_entry.version)
            .ok_or_else(|| format!("invalid_new_version: {}", new_entry.version))?;
        if new_sv <= cur_sv {
            return Err(format!(
                "upgrade_requires_higher_version: current={current_ver} new={}",
                new_entry.version
            ));
        }
        let new_ver = new_entry.version.clone();
        self.state
            .history
            .entry(id.to_string())
            .or_default()
            .push(new_entry);
        self.state.current.insert(id.to_string(), new_ver);
        self.persist();
        Ok(())
    }

    /// Downgrade a plugin to a strictly older version that was previously installed.
    pub(crate) fn downgrade(&mut self, id: &str, target_version: &str) -> Result<(), String> {
        let current_ver = self
            .state
            .current
            .get(id)
            .ok_or_else(|| format!("plugin_not_installed: {id}"))?
            .clone();
        let cur_sv = SemVer::parse(&current_ver).ok_or("invalid_current_version")?;
        let tgt_sv = SemVer::parse(target_version)
            .ok_or_else(|| format!("invalid_target_version: {target_version}"))?;
        if tgt_sv >= cur_sv {
            return Err(format!(
                "downgrade_requires_lower_version: current={current_ver} target={target_version}"
            ));
        }
        // The target version must have been installed before.
        let in_history = self
            .state
            .history
            .get(id)
            .map(|h| h.iter().any(|e| e.version == target_version))
            .unwrap_or(false);
        if !in_history {
            return Err(format!(
                "version_not_in_history: {target_version} (install it first)"
            ));
        }
        self.state
            .current
            .insert(id.to_string(), target_version.to_string());
        self.persist();
        Ok(())
    }

    /// Uninstall a plugin (marks all history entries as `Uninstalled` and
    /// removes it from the active map).
    pub(crate) fn uninstall(&mut self, id: &str) -> Result<(), String> {
        if !self.state.current.contains_key(id) {
            return Err(format!("plugin_not_installed: {id}"));
        }
        if let Some(history) = self.state.history.get_mut(id) {
            for e in history.iter_mut() {
                e.state = PluginState::Uninstalled;
            }
        }
        self.state.current.remove(id);
        self.persist();
        Ok(())
    }

    // ── Query operations ──────────────────────────────────────────────────────

    /// Return all currently active plugin entries.
    pub(crate) fn list_active(&self) -> Vec<PluginEntry> {
        self.state
            .current
            .iter()
            .filter_map(|(id, ver)| {
                self.state
                    .history
                    .get(id)?
                    .iter()
                    .find(|e| &e.version == ver)
                    .cloned()
            })
            .collect()
    }

    /// Return the currently active entry for a plugin, if any.
    pub(crate) fn get_current(&self, id: &str) -> Option<&PluginEntry> {
        let ver = self.state.current.get(id)?;
        self.state
            .history
            .get(id)?
            .iter()
            .find(|e| &e.version == ver)
    }

    /// Return all recorded versions for a plugin (including uninstalled).
    pub(crate) fn history(&self, id: &str) -> &[PluginEntry] {
        self.state
            .history
            .get(id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn is_installed(&self, id: &str) -> bool {
        self.state.current.contains_key(id)
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.state) {
            let _ = std::fs::create_dir_all("state");
            let _ = std::fs::write("state/plugin-registry.json", json);
        }
    }
}

/// Compute the current wall-clock time in milliseconds since the Unix epoch.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Verify a SHA-256 hex checksum against raw bytes.
pub(crate) fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(bytes));
    actual.eq_ignore_ascii_case(expected_hex)
}
