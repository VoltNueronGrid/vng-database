# Sub-Tasks v2 — VoltNueronGrid VS Code “Pro DB Client” Parity

**Project:** `polap-db` — VS Code extension `ui/ide-extensions/vscode-cursor`  
**Last updated:** 2026-04-17  
**Purpose:** Track work to match **commercial database-client** UX (reference screenshots: connection chrome, deep tree, rich context menus). This is **broader** than the original IDE roadmap: many features need **new HTTP/runtime contracts**, not UI-only changes.

---

## 0) Concepts (read first)

| Term | Meaning in this extension |
|------|---------------------------|
| **Active profile** | The saved connection currently selected for SQL, explorer, and history. |
| **Verified** | Last health probe to `{baseUrl}/health` returned **HTTP 200**. Stored as `Connection.isConnected` in UI copy as **“Verified”**. |
| **Schema tree** | Built from **VoltNueronGrid** schema registry (`/api/v1/ingest/schema/registry`), **not** a direct PostgreSQL wire protocol. Parity with a **native Postgres** extension is only possible where the **runtime exposes equivalent metadata and operations**. |

**How to confirm the DB/runtime is up**

1. Start `voltnuerongridd` locally (see repo README / `PUBLISHING.md`).
2. In VS Code: **VoltNueronGrid → Database** → select profile → **Test Connection** (toolbar) or context **Test Connection**.
3. If verification fails: check **Output → VoltNueronGrid** for `[Health]` lines; fix **base URL**, **admin key** (admin/operator), or **tenant headers** (tenant mode).

---

## 1) Connection lifecycle & verification (foundation)

| ID | Task | Status | Notes |
|----|------|--------|--------|
| V2-1.1 | Auto-verify active profile on startup, after switch, after save, after activate from Manage panel | ✅ Done (v0.3.2) | Probes `/health`; failures logged to output |
| V2-1.2 | Optional: explicit “Verify now” command vs reusing **Test Connection** | ⏳ Backlog | Avoid duplicate UX |
| V2-1.3 | Surface last verification error as tree **message** node (not only Output) | ⏳ Backlog | Clearer than truncated toast |
| V2-1.4 | Document troubleshooting: firewall, TLS, wrong port, missing `VNG_ADMIN_API_KEY` on server | ⏳ Docs | Link from welcome view |

---

## 2) Connection tree chrome (match screenshots 2–3, 8–9)

| ID | Task | Status | Depends on |
|----|------|--------|------------|
| V2-2.1 | **Grouped tree**: optional `Group` / folder node (e.g. `localmachine`) above connections | ⏳ Not started | UX spec + persisted field on `ConnectionSettings` |
| V2-2.2 | **Inline actions** on connection row: refresh, add, delete, filter (as icons) | ⏳ Not started | `TreeItem` API limits; may use contributed menus + title |
| V2-2.3 | **Status dot** (green = active+verified) using `TreeItem` theme icons / custom SVG | ⏳ Not started | Design assets |
| V2-2.4 | **Context menu** on connection: Edit, Close, Copy Host, Copy Connection JSON, Server Status, View History, Import SQL, Copy Connection Key | ⏳ Partial | Many need backend (status, history store, import pipeline) |

---

## 3) Explorer depth: databases → schemas → tables → sub-nodes

Reference UX shows: **Query** folder, **Types**, **Tables** with row counts, **Columns / Index / Triggers** under each table.

| ID | Task | Status | Depends on |
|----|------|--------|------------|
| V2-3.1 | Extend schema model or adapter: **per-table row estimates**, **table type** buckets | ⏳ Not started | Registry/API must expose counts or approximate |
| V2-3.2 | Add tree levels: **Tables** container; under table: **Columns**, **Indexes**, **Triggers** | ⏳ Not started | Metadata from registry + optional SQL introspection |
| V2-3.3 | **Query** pseudo-node: list `.sql` files or ad-hoc saved queries | ⏳ Not started | Workspace virtual documents or storage |
| V2-3.4 | Column icons: PK, FK, indexed — map from `Column` flags in model | ⏳ Partial | Ensure registry populates `isPrimaryKey`, `isForeignKey`, etc. |

---

## 4) Context menus & operations (screenshots 4–7)

Map each requested command to **implementation type**:

- **A** = client-only (clipboard, open editor, template SQL)  
- **B** = uses existing `execute` / schema APIs  
- **C** = needs **new** service endpoints or privileged ops  

| Operation | Type | Task ID |
|-----------|------|---------|
| Copy Name | A | V2-4.1 ✅ (partial) |
| Edit Connection | A | ✅ |
| Close / Disconnect | A | ✅ |
| Show DDL / SQL Template / Mock / Dump struct | A/B | ✅ partial |
| Drop / Truncate / Add column / Create index | B/C | V2-4.2 |
| Full-text search | C | V2-4.3 |
| Dump struct **and** data | C | V2-4.4 |
| Import SQL | B/C | V2-4.5 |
| Generate document | C | V2-4.6 |
| View History (object-scoped) | B | V2-4.7 |
| Server Status | B | V2-4.8 |

**Epic V2-4** — Register all `package.json` menu entries with `when` clauses per `viewItem`; stub commands that show “Not supported by runtime” until API exists.

---

## 5) Backend / API prerequisites (blocking “full parity”)

| ID | Task | Owner | Notes |
|----|------|-------|--------|
| V2-5.1 | **Contract doc**: which Postgres-style features are in-scope for VNG vs out-of-scope | Arch | Avoid impossible promises |
| V2-5.2 | **Table/index/trigger** introspection beyond current registry | Runtime | May require SQL against tenant catalog |
| V2-5.3 | **Export** dump (structure + data) with safety caps | Runtime | Streaming, size limits, RBAC |
| V2-5.4 | **FTS** or search API | Product | May defer |
| V2-5.5 | **Server status** dashboard payload (connections, lag, version) | Runtime | New JSON for IDE |

---

## 6) Testing & release

| ID | Task | Status |
|----|------|--------|
| V2-6.1 | Integration test: mock HTTP → verified flag toggles, tree expands | ⏳ |
| V2-6.2 | Playwright or VS Code extension test harness for smoke | ⏳ |
| V2-6.3 | CHANGELOG section “Pro client parity” with phased delivery | ⏳ |

---

## 7) Suggested execution order

1. **V2-1.x** — stable connection verification (mostly done).  
2. **V2-3.2 + V2-3.4** — deeper tree with real metadata from registry.  
3. **V2-4.x** — context menus; implement **A** immediately, **B** next, **C** gated on V2-5.x.  
4. **V2-2.x** — visual polish (groups, dots, inline actions).  
5. **V2-5.x** — runtime features for dumps, FTS, server status.

---

## 8) Explicit non-goals (unless product approves)

- Replacing **native PostgreSQL** tools for arbitrary clusters (different security model).  
- Guaranteed feature parity with a **third-party Postgres extension** without equivalent **wire/catalog** access through VNG.

---

*This file complements `sub-tasks.md` (original release roadmap). Use v2 for “enterprise DB UI parity”; keep v1 tasks for core engine/gates.*
