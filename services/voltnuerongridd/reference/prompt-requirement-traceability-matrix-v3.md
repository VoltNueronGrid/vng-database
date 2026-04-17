# Prompt-to-Requirement Traceability Matrix v3

**Status:** Draft complete (S0-002)  
**Primary source:** `prompts/prompt-1.md`  
**Last updated:** 2026-04-17

---

## Scope

This matrix maps each requirement from `prompt-1.md` to:

- implementation tracks/sprints in `status-tracker-v3.md`
- current coverage state
- acceptance evidence expected before closure

---

## Requirement Matrix (R-01..R-18)

| Req ID | Prompt requirement summary | Planned closure sprint(s) | Current state | Acceptance evidence |
|---|---|---|---|---|
| R-01 | ANSI SQL + AI-assisted chat/extract/ingest/import/export | S7, S11 | In Progress | E2E prompt scenario pack + feature demos |
| R-02 | Create DB/table/view/MV/functions | S7 | In Progress | SQL lifecycle integration tests |
| R-03 | In-DB function languages (Rust, JS, Python) | S7 | In Progress | Language runtime conformance tests |
| R-04 | HA/FT/reliability/elastic/i18n/UTF-8 | S9, S11 | In Progress | Soak + failover + i18n validation reports |
| R-05 | Data files separate from engine | S8 | In Progress | Storage/engine separation architecture tests |
| R-06 | CSV/Parquet/Excel ingest | S8 | In Progress | Format ingestion suite green |
| R-07 | Fast multi-threaded import | S8 | In Progress | Throughput benchmarks against target |
| R-08 | Local laptop + cloud SaaS | S11 | In Progress | Dual deployment runbooks + smoke tests |
| R-09 | Plugin/extensibility ecosystem | S10, S11 | In Progress | Plugin SPI spec + sample plugins |
| R-10 | Trillion-row scale and fast retrieval | S9, S11 | Not Proven | Reproducible benchmark and latency evidence |
| R-11 | Indexes and constraints | S7, S8 | In Progress | DDL/DML correctness and perf tests |
| R-12 | Full trigger model + queue sinks (Kafka/NATS) | S7 | Not Started | Trigger matrix + queue contract tests |
| R-13 | Retrieval optimization at extreme scale | S8, S9 | In Progress | Join/paging stress report |
| R-14 | Seeded function parity + UDF | S7 | In Progress | Function parity checklist complete |
| R-15 | Multi-user roles and authorization | S5, S11 | In Progress | RBAC integration + policy tests |
| R-16 | UI client separate from engine | S3, S4 | In Progress | IDE client uses driver abstraction, not direct runtime coupling |
| R-17 | Native multi-language drivers (must-have) | S1..S11 | Critical Gap (actively closing) | Rust/TS/Python GA + expansion roadmap evidence |
| R-18 | Native local operation for small volumes | S3, S11 | In Progress | Local setup UX and performance smoke report |

---

## Current Gap Hotspots

1. **R-17 (Drivers):** Rust hardening and TS/Python scaffolds exist, but TS/Python full GA behavior and ecosystem breadth remain open.
2. **R-12 (Triggers + queue sinks):** No completed trigger/eventing closure yet.
3. **R-10 (Scale claim):** No final benchmark evidence proving trillion-row objectives.

---

## Traceability Rules

1. Every new sprint task must reference one or more `R-*` IDs.
2. No `R-*` can move to `Done` without linked acceptance evidence.
3. Deferred requirements must include target sprint and reason in the decision log.

