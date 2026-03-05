# VoltNueronGrid KPI Harness (Scaffold)

This folder contains the KPI harness for PR-004/PR-007 execution.

Initial KPI coverage:
- OLTP latency (`oltp-latency`)
- OLAP latency (`olap-latency`)
- HTAP mixed throughput (`htap-mixed-throughput`)
- Failover RTO/RPO (`failover-rto-rpo`)

## Structure

- `config/targets.yaml` - pass/fail thresholds aligned to README KPI table
- `scenarios/*.yaml` - scenario definitions (workload profile + assertions)
- `scripts/run-kpi.ps1` - Windows orchestration entrypoint (threshold-aware + rollup)
- `scripts/run-kpi.sh` - Linux/macOS orchestration entrypoint
- `scripts/run-scenario.ps1` - per-scenario runner (loads thresholds from `config/targets.yaml`)
- `scripts/run-cloud-smoke.ps1` - cloud-profile smoke-pack runner with rollup
- `config/cloud-profiles.yaml` - cloud profile definitions for smoke packs
- `config/cloud-profiles-real.yaml` - env-driven real cloud endpoint profiles
- `scripts/generate-gate-report.ps1` - final gate report + local-baseline deltas
- `scripts/bootstrap-phase3.ps1` - phase-3 orchestrator (env validation + run + report + exit code policy)
- `scripts/run-autonomous-guardrail-smoke.ps1` - H-01 guardrail control smoke test
- `scripts/check-r1-gate.ps1` - executable R1 gate checklist artifact validator
- `scripts/run-sql-analyze-smoke.ps1` - WS1 SQL analyzer API smoke evidence generator
- `scripts/run-legacy-aggregation-parity.ps1` - WS1A legacy aggregation parity harness
- `scripts/run-legacy-aggregation-gap-report.ps1` - WS1A bucketed P0/P1/P2 gap report generator
- `scripts/run-store-durability-smoke.ps1` - WS2 durability bootstrap validation smoke
- `scripts/run-ws2-disk-wal-smoke.ps1` - WS2 disk-backed WAL adapter skeleton smoke
- `scripts/run-ws2a-row-sync-origin-smoke.ps1` - WS2A row-store sync-origin scaffold smoke
- `scripts/run-ws2-checkpoint-restart-smoke.ps1` - WS2 checkpoint/restart continuity smoke
- `scripts/run-h02-sync-fault-injection.ps1` - H-02 sequence-gap fault-injection harness
- `scripts/run-h02-reorder-duplicate-faults.ps1` - H-02 reorder/duplicate fault harness
- `scripts/run-h02-restart-replay-matrix.ps1` - H-02 restart+replay fault matrix harness
- `scripts/run-ws3-query-routing-smoke.ps1` - WS3 HTAP route-decision + runtime dispatch scaffold smoke
- `scripts/run-ws4-ingest-plugin-smoke.ps1` - WS4 ingestion plugin registry scaffold smoke
- `scripts/run-ws4a-streaming-smoke.ps1` - WS4A streaming in/out + replayable event path scaffold smoke
- `scripts/run-ws4a-replay-cursor-smoke.ps1` - WS4A replay cursor durability bridge smoke
- `scripts/run-ws5-operator-auth-smoke.ps1` - WS5 operator auth baseline smoke
- `scripts/run-ws6-failover-sim-smoke.ps1` - WS6 failover simulation scaffold smoke
- `scripts/run-ws7-plugin-boundary-smoke.ps1` - WS7 connector package registration boundary smoke
- `results/` - output folder for run artifacts

## Usage

PowerShell:

`pwsh ./tests/kpi/scripts/run-kpi.ps1 -BaseUrl "http://localhost:8080" -SqlUrl "http://localhost:8080" -OutputDir "./tests/kpi/results/week1" -TargetsPath "./tests/kpi/config/targets.yaml" -AuthMode "none"`

Bash:

`bash ./tests/kpi/scripts/run-kpi.sh "http://localhost:8080" "http://localhost:8080" "./tests/kpi/results/week1" "./tests/kpi/config/targets.yaml" "none"`

Cloud smoke packs (PowerShell):

`pwsh ./tests/kpi/scripts/run-cloud-smoke.ps1 -OutputRootDir "./tests/kpi/results/20260304-pr007/cloud-profiles" -CloudProfilesPath "./tests/kpi/config/cloud-profiles.yaml" -TargetsPath "./tests/kpi/config/targets.yaml"`

Real cloud smoke packs (PowerShell, requires env vars from `cloud-profiles-real.yaml`):

`pwsh ./tests/kpi/scripts/run-cloud-smoke.ps1 -OutputRootDir "./tests/kpi/results/20260304-pr007/cloud-profiles-real" -CloudProfilesPath "./tests/kpi/config/cloud-profiles-real.yaml" -TargetsPath "./tests/kpi/config/targets.yaml"`

Deferred cloud smoke planning mode (no env vars yet; marks profiles as `pending_config` and emits readiness report):

`pwsh ./tests/kpi/scripts/run-cloud-smoke.ps1 -OutputRootDir "./tests/kpi/results/20260304-pr007/cloud-profiles-real" -CloudProfilesPath "./tests/kpi/config/cloud-profiles-real.yaml" -TargetsPath "./tests/kpi/config/targets.yaml" -AllowMissingEnv`

Final gate report with deltas vs local baseline:

`pwsh ./tests/kpi/scripts/generate-gate-report.ps1 -LocalBaselineRoot "./tests/kpi/results/20260304-pr007" -CloudRollupPath "./tests/kpi/results/20260304-pr007/cloud-profiles/cloud-rollup-summary.json" -OutputDir "./tests/kpi/results/20260304-pr007/reports"`

Single-command phase-3 bootstrap:

`pwsh ./tests/kpi/scripts/bootstrap-phase3.ps1 -LocalBaselineRoot "./tests/kpi/results/20260304-pr007"`

## Notes

- Thresholds are loaded from `config/targets.yaml`.
- Each run writes per-scenario JSON plus a `rollup-summary.json` file.
- Real-cloud profile mode resolves endpoint/token values from environment variables.
- Cloud smoke runner now emits `cloud-readiness-report.json` for missing-env readiness tracking.
- `bootstrap-phase3.ps1` exits non-zero only when final KPI gate is truly `failed` (it exits zero for `pending_config`).
