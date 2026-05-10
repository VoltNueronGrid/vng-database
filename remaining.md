# `remaining.md` — handoff for next session (v25)

**Last updated:** 2026-05-10 (session 25 — full proxy response fidelity, VNG_NODE_URL docs)
**Branch:** `main`
**Latest commit:** (to be set after commit)
**cargo check -p voltnuerongridd:** clean ✓ (25 warnings — all safe to ignore)
**cargo test -p voltnuerongridd:** 750/750 ✓

---

## TL;DR — what landed this session

### ✅ Full proxy response fidelity (handlers/sql.rs, helpers/execution.rs, main.rs)

`SqlExecuteResponse.status` changed from `&'static str` to `String`; `#[derive(Serialize, Deserialize)]` added.  The same change cascaded to all nested types that were blocking `Deserialize`:

| Type | Change |
|---|---|
| `SqlExecuteResponse.status` | `&'static str` → `String` |
| `OlapQueryResponse.status` | `&'static str` → `String` |
| `LegacyAggResult.source` | `&'static str` → `String` |
| `SqlTransactionResponse.status` | `&'static str` → `String` |
| `OlapVecAggResult` | added `Deserialize` |
| `LegacyAggResult` | added `Deserialize` |
| `OlapQueryResponse` | added `Deserialize` |
| `SqlTransactionResponse` | added `Deserialize` |

UDF types (`UdfExecutionResult`, `UdfFunctionCatalogEntry`, `UdfLanguageGuardPolicy`, `UdfInvocationPlan`) still have `&'static str` fields; their corresponding fields in `SqlExecuteResponse` are annotated `#[serde(skip_deserializing, default)]` so the outer struct can implement `Deserialize` without touching them.

All `status: "..."` / `source: "..."` constructors across `handlers/sql.rs`, `helpers/execution.rs`, and `main.rs` updated to `.to_string()`.

The transparent proxy in `sql_execute` now fully deserializes the leader's response:

```rust
if let Ok(body) = leader_resp.json::<SqlExecuteResponse>().await {
    return Ok((leader_status, Json(body)));
}
```

This replaces the previous partial `serde_json::Value` parsing that only forwarded `status`, `reason`, `oltp_rows`, `columns`, `rows`.

### ✅ VNG_NODE_URL documented (deploy/local/vng.env.example)

Added commented entries for `VNG_CLUSTER_TOKEN` and `VNG_NODE_URL` to the Multi-node / HA section, with explanations of their purpose and the DML proxy forwarding mechanism.

---

## What's still TODO

1. **UDF type cleanup** — `UdfExecutionResult`, `UdfFunctionCatalogEntry`, `UdfLanguageGuardPolicy`, `UdfInvocationPlan` still use `&'static str` fields.  These are skipped during deserialization (`#[serde(skip_deserializing, default)]`) and are not forwarded through the proxy.  A follow-on could change them to `String` for consistency.

2. **`VNG_NODE_URL` in cloud deploy READMEs** — the env var is documented in `deploy/local/vng.env.example` but not yet in `deploy/cloud/aws/README.md`, `deploy/cloud/azure/README.md`, `deploy/cloud/gcp/README.md` (those files cover CI pipeline variables, not service runtime vars; a dedicated runtime vars section could be added).

3. **Proxy auth token forwarding** — the follower proxy should also forward `VNG_CLUSTER_TOKEN` as a `Bearer` token for internal authentication when forwarding DML to the leader, rather than relying solely on the client's operator headers.

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/execution.rs
@services/voltnuerongridd/src/main.rs
```

Recommended next steps (in priority order):
1. **Proxy cluster token forwarding** — in `sql_execute` proxy path, add `VNG_CLUSTER_TOKEN` as `Authorization: Bearer <token>` when forwarding to leader
2. **UDF `&'static str` cleanup** — change UDF struct fields from `&'static str` to `String` (no behaviour change, just consistency)

**Environment notes:**
- `VNG_CLUSTER_TOKEN` — shared secret for intra-cluster Raft RPCs
- `VNG_RAFT_PEERS` — comma-separated peer base URLs
- `VNG_NODE_URL` — this node's own advertised base URL (leader broadcasts via `x-vng-leader-url` header; followers use it to proxy DML writes)
- `VNG_RBAC_POLICY_PATH` — JSON RBAC privilege matrix
