# voltnuerongrid-audit-companion

Operator-facing CLI over the VoltNueronGrid audit trail (Tasks-7 **A-9**). It
queries audit events, verifies the tamper-evident hash chain, and exports a
portable evidence bundle.

## Build

```bash
cargo build -p voltnuerongrid-audit-companion
```

## Usage

The `--audit-file` argument accepts **either** a local JSON file (an array of
`AuditEvent`, or an API response object with an `events` field) **or** a live
runtime API URL (`http://…` / `https://…`).

### List events

```bash
audit-companion list --audit-file http://127.0.0.1:8080/api/v1/audit/export
audit-companion list --audit-file ./events.json --action ai_tune_apply_index --limit 50
```

### Verify the hash chain (surfaces the tamper point)

```bash
audit-companion verify --audit-file http://127.0.0.1:8080/api/v1/audit/export
# chain_valid: true (42 events verified)
# — or, on tampering, exit code 2:
# chain_valid: false — TAMPER DETECTED at event_id 17 (of 42 events)
```

### Export an evidence bundle (JSON lines + manifest)

```bash
audit-companion export \
  --audit-file http://127.0.0.1:8080/api/v1/audit/export \
  --out-dir ./audit-evidence-bundle
# writes ./audit-evidence-bundle/events.jsonl and ./audit-evidence-bundle/manifest.json
```

`manifest.json` records `event_count`, `chain_valid`, and
`tamper_point_event_id` (the first broken link, or `null` when intact).

### Legacy report mode (back-compat)

Invoking with `--audit-file` **and** `--action-file` (no subcommand) produces the
original correlation report:

```bash
audit-companion --audit-file events.json --action-file actions.json --out report.json
```

## Smoke test

```bash
cargo test -p voltnuerongrid-audit-companion
```

The bundled tests cover bundle export, manifest integrity, and tamper-point
detection.
