"""Apply Session 11 sprint items to raft.rs and main.rs (CRLF-aware)."""

def cr(s):
    return s.replace("\n", "\r\n")

# ── raft.rs ──────────────────────────────────────────────────────────────────
raft_path = r"d:\by\polap-db\services\voltnuerongridd\src\raft.rs"
with open(raft_path, "rb") as f:
    raft = f.read().decode("utf-8")

orig = raft

# 1. Add fencing_token to RaftStatusSnapshot
raft = raft.replace(cr(
    "    /// Configured election timeout in ticks (S7-WS6-03).\n"
    "    pub election_timeout_ticks: u64,\n"
    "}"
), cr(
    "    /// Configured election timeout in ticks (S7-WS6-03).\n"
    "    pub election_timeout_ticks: u64,\n"
    "    /// S7-WS6-03: Fencing token - advances on each leader election.\n"
    "    pub fencing_token: u64,\n"
    "}"
), 1)

# 2. Add fencing_token to RaftNode struct
raft = raft.replace(cr(
    "    /// S7-WS6-03: election timeout threshold in ticks.\n"
    "    /// Randomised per-node in real deployments; fixed here for deterministic tests.\n"
    "    pub election_timeout_ticks: u64,\n"
    "}"
), cr(
    "    /// S7-WS6-03: election timeout threshold in ticks.\n"
    "    /// Randomised per-node in real deployments; fixed here for deterministic tests.\n"
    "    pub election_timeout_ticks: u64,\n"
    "    /// S7-WS6-03: Fencing token, increments each time this node becomes Leader.\n"
    "    pub fencing_token: u64,\n"
    "}"
), 1)

# 3. Add fencing_token init in new()
raft = raft.replace(cr(
    "            ticks_since_heartbeat: 0,\n"
    "            election_timeout_ticks: 10,\n"
    "        }"
), cr(
    "            ticks_since_heartbeat: 0,\n"
    "            election_timeout_ticks: 10,\n"
    "            fencing_token: 0,\n"
    "        }"
), 1)

# 4. Increment fencing_token in become_leader()
raft = raft.replace(cr(
    "    /// The leader won an election; transition to Leader.\n"
    "    pub fn become_leader(&mut self) {\n"
    "        self.role = RaftRole::Leader;\n"
    "    }"
), cr(
    "    /// The leader won an election; transition to Leader.\n"
    "    pub fn become_leader(&mut self) {\n"
    "        self.fencing_token += 1;\n"
    "        self.role = RaftRole::Leader;\n"
    "    }"
), 1)

# 5. Add fencing_token to status()
raft = raft.replace(cr(
    "            ticks_since_heartbeat: self.ticks_since_heartbeat,\n"
    "            election_timeout_ticks: self.election_timeout_ticks,\n"
    "        }"
), cr(
    "            ticks_since_heartbeat: self.ticks_since_heartbeat,\n"
    "            election_timeout_ticks: self.election_timeout_ticks,\n"
    "            fencing_token: self.fencing_token,\n"
    "        }"
), 1)

assert raft != orig, "raft.rs unchanged"
with open(raft_path, "wb") as f:
    f.write(raft.encode("utf-8"))
print("raft.rs OK")
