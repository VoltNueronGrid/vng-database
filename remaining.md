# `remaining.md` — handoff for next session (v26)

**Last updated:** 2026-05-14 (session 26 — DML proxy now forwards cluster token)
**Branch:** `main`
**Latest commit:** (to be set after commit)
**cargo check -p voltnuerongridd:** clean ✓ (27 warnings — all safe to ignore)
**cargo test -p voltnuerongridd:** 750/750 ✓

---

## TL;DR — what landed this session

### ✅ DML proxy forwards `VNG_CLUSTER_TOKEN` (handlers/sql.rs)

In `sql_execute`, when a follower transparently proxies a DML batch to the current leader, the forwarded request now carries `x-vng-cluster-token: <VNG_CLUSTER_TOKEN>` in addition to the client's operator headers (`x-vng-admin-key`, `x-vng-operator-id`, `authorization`, `x-vng-session-id`, `x-request-id`).

The token is read from `state.cluster_token: Arc<Option<String>>` — the same source that `helpers/raft_loop.rs` uses to sign intra-cluster Raft RPCs. If `VNG_CLUSTER_TOKEN` is unset, no header is added (forwarding remains operator-auth only, as before).

**Design note:** the header is intentionally `x-vng-cluster-token`, not `Authorization: Bearer`. The leader's `/api/v1/sql/execute` is a user-facing endpoint that runs operator auth (`x-vng-admin-key` check in `auth.rs`); overloading `Authorization` would clobber the client's bearer/JWT. A dedicated header is additive and preserves operator auth.

---

## What's still TODO

1. **Leader-side check on `x-vng-cluster-token`** — today the leader only sees the header but doesn't validate it. A follow-on should:
   - In `sql_execute` (or a small middleware), if `x-vng-cluster-token` is present, validate it equals `state.cluster_token` and emit an audit event tagging the request as `intra_cluster_proxied`.
   - Decide policy: reject mismatched tokens hard, or treat it as a trust signal layered on top of operator auth (recommended).

2. **UDF type cleanup** — `UdfExecutionResult`, `UdfFunctionCatalogEntry`, `UdfLanguageGuardPolicy`, `UdfInvocationPlan` still use `&'static str` fields. These are skipped during deserialization (`#[serde(skip_deserializing, default)]`) and are not forwarded through the proxy. A follow-on could change them to `String` for consistency.

3. **`VNG_NODE_URL` in cloud deploy READMEs** — the env var is documented in `deploy/local/vng.env.example` but not yet in `deploy/cloud/aws/README.md`, `deploy/cloud/azure/README.md`, `deploy/cloud/gcp/README.md` (those files cover CI pipeline variables, not service runtime vars; a dedicated runtime vars section could be added).

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/auth.rs
@services/voltnuerongridd/src/handlers/raft.rs
```

Recommended next steps (in priority order):
1. **Leader-side `x-vng-cluster-token` enforcement** — closes the loop on this session's work; without it the forwarded token is informational only
2. **UDF `&'static str` cleanup** — change UDF struct fields from `&'static str` to `String` (no behaviour change, just consistency)

**Environment notes:**
- `VNG_CLUSTER_TOKEN` — shared secret for intra-cluster Raft RPCs (now also forwarded by DML proxy)
- `VNG_RAFT_PEERS` — comma-separated peer base URLs
- `VNG_NODE_URL` — this node's own advertised base URL (leader broadcasts via `x-vng-leader-url` header; followers use it to proxy DML writes)
- `VNG_RBAC_POLICY_PATH` — JSON RBAC privilege matrix
