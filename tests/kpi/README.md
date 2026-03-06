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
- `scripts/run-sql-execute-udf-smoke.ps1` - WS1 runtime UDF execution smoke via `/api/v1/sql/execute` (polyglot path + function catalog contract + per-language guard policies + execution-plan routing evidence)
- `scripts/run-ws1-udf-contract-smoke.ps1` - WS1 UDF contract smoke (Rust/JS/Python language declaration + create-function classification hooks)
- `scripts/run-ws1-udf-coverage-export.ps1` - WS1 post-gate artifact export (UDF contract/runtime coverage matrix)
- `scripts/run-ws1-gate-trend-compare.ps1` - WS1 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws1-udf-stability-badge.ps1` - WS1 post-gate artifact export (UDF stability badge JSON)
- `scripts/run-ws1-release-summary.ps1` - WS1 release-facing UDF readiness summary (single artifact from WS1 summary + coverage + trend + badge)
- `scripts/run-ws1-gate.ps1` - WS1 gate orchestrator (SQL core + UDF contract/runtime packs + coverage/trend/badge/release summary)
- `scripts/run-ws1-closure-gate.ps1` - WS1 closure gate (validation-posture check from WS1 gate + UDF runtime/coverage/trend/badge/release artifacts)
- `scripts/run-release-r1-sql-udf-gate.ps1` - R1 release SQL/UDF/locking readiness linkage gate (R1 checklist + WS1 closure + WS22 closure posture)
- `scripts/run-release-r3-udf-runtime-gate.ps1` - R3 release UDF runtime readiness linkage gate (WS1 closure + R3 autonomous baseline)
- `scripts/run-legacy-aggregation-parity.ps1` - WS1A legacy aggregation parity harness
- `scripts/run-legacy-aggregation-gap-report.ps1` - WS1A bucketed P0/P1/P2 gap report generator
- `scripts/run-ws1a-udf-contract-bridge-smoke.ps1` - WS1A bridge smoke (legacy aggregation parity hooks + polyglot UDF contract test linkage)
- `scripts/run-ws1a-gate.ps1` - WS1A gate orchestrator (parity + bucketed gap report + UDF bridge summary)
- `scripts/run-store-durability-smoke.ps1` - WS2 durability bootstrap validation smoke
- `scripts/run-ws2-disk-wal-smoke.ps1` - WS2 disk-backed WAL adapter skeleton smoke
- `scripts/run-ws2a-row-sync-origin-smoke.ps1` - WS2A row-store sync-origin scaffold smoke
- `scripts/run-ws2-checkpoint-restart-smoke.ps1` - WS2 checkpoint/restart continuity smoke
- `scripts/run-ws2-gate.ps1` - WS2 gate orchestrator (durability + WAL + checkpoint/restart summary)
- `scripts/run-ws2a-gate.ps1` - WS2A gate orchestrator (row-sync-origin summary)
- `scripts/run-h02-sync-fault-injection.ps1` - H-02 sequence-gap fault-injection harness
- `scripts/run-h02-reorder-duplicate-faults.ps1` - H-02 reorder/duplicate fault harness
- `scripts/run-h02-restart-replay-matrix.ps1` - H-02 restart+replay fault matrix harness
- `scripts/run-ws3-query-routing-smoke.ps1` - WS3 HTAP route-decision + runtime dispatch scaffold smoke
- `scripts/run-ws3-htap-target-contract-smoke.ps1` - WS3 HTAP throughput target contract smoke (targets + scenario parity)
- `scripts/run-ws3-performance-score.ps1` - WS3 performance score artifact (weighted gate-control scoring)
- `scripts/run-ws3-gate-trend-compare.ps1` - WS3 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws3-performance-stability-badge.ps1` - WS3 post-gate artifact export (performance stability badge JSON)
- `scripts/run-ws3-release-summary.ps1` - WS3 release-facing readiness summary (single artifact from WS3 summary + score + trend + badge)
- `scripts/run-ws3-gate.ps1` - WS3 gate orchestrator (query-routing + HTAP target contract packs + score/trend/badge/release summary)
- `scripts/run-ws4-ingest-plugin-smoke.ps1` - WS4 ingestion plugin registry scaffold smoke
- `scripts/run-ws4-gate.ps1` - WS4 gate orchestrator (ingest plugin smoke summary)
- `scripts/run-ws4a-streaming-smoke.ps1` - WS4A streaming in/out + replayable event path scaffold smoke
- `scripts/run-ws4a-replay-cursor-smoke.ps1` - WS4A replay cursor durability bridge smoke
- `scripts/run-ws4a-gate.ps1` - WS4A gate orchestrator (streaming + replay-cursor summary)
- `scripts/run-ws5-operator-auth-smoke.ps1` - WS5 security smoke (operator auth + TLS/encryption contract checks)
- `scripts/run-ws5-gate.ps1` - WS5 gate orchestrator (runs WS5 security smoke and emits one gate summary artifact)
- `scripts/run-ws5-gate-badge.ps1` - WS5 gate badge artifact generator (consumes gate summary and emits CI badge JSON)
- `scripts/run-ws6-failover-sim-smoke.ps1` - WS6 failover simulation scaffold smoke
- `scripts/run-ws6-failover-contract-smoke.ps1` - WS6 failover contract smoke (failover API routes + RTO/RPO target declarations + leader-rotation tests)
- `scripts/run-ws6-dr-failover-smoke.ps1` - WS6 DR failover smoke (automated DR hook failover path test)
- `scripts/run-ws6-handoff-matrix-smoke.ps1` - WS6 multi-node handoff matrix smoke (leader rotation matrix scenarios)
- `scripts/run-ws6-replication-lag-scenarios-smoke.ps1` - WS6 replication-lag failure signal smoke (signal + reconcile scenarios)
- `scripts/run-ws6-rto-rpo-threshold-score.ps1` - WS6 failover threshold scoring pack (targets-driven RTO/RPO score)
- `scripts/run-ws6-node-loss-rejoin-smoke.ps1` - WS6 chaos smoke (node-loss to failover to rejoin reconciliation sequence)
- `scripts/run-ws6-failover-flap-resistance-smoke.ps1` - WS6 chaos smoke (repeated failover cycles for flap-resistance evidence)
- `scripts/run-ws6-reconcile-latency-envelope-smoke.ps1` - WS6 chaos smoke (reconcile p95 latency envelope evidence)
- `scripts/run-ws6-chaos-fault-matrix-export.ps1` - WS6 post-gate artifact export (chaos fault-injection matrix)
- `scripts/run-ws6-gate-trend-compare.ps1` - WS6 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws6-failover-stability-badge.ps1` - WS6 post-gate artifact export (failover stability badge JSON)
- `scripts/run-ws6-release-summary.ps1` - WS6 release-facing readiness summary (single artifact from WS6 summary + chaos matrix + trend + badge)
- `scripts/run-ws6-gate.ps1` - WS6 gate orchestrator (all WS6 packs + chaos fault-matrix export + trend compare + stability badge + release summary)
- `scripts/run-ws6-closure-gate.ps1` - WS6 closure gate (validation posture checks over WS6 gate + release artifacts)
- `scripts/run-release-r2-failover-gate.ps1` - R2 release failover readiness gate (WS6 closure + Ops/Resilience cluster)
- `scripts/run-ws7-plugin-boundary-smoke.ps1` - WS7 connector package registration boundary smoke
- `scripts/run-ws7-manifest-integrity-smoke.ps1` - WS7 plugin manifest integrity smoke (checksum/signature/key trust/revocation checks)
- `scripts/run-ws7-registration-policy-smoke.ps1` - WS7 registration policy smoke (required fields, capabilities, custom hook policy checks)
- `scripts/run-ws7-compliance-matrix-export.ps1` - WS7 post-gate artifact export (plugin compliance matrix)
- `scripts/run-ws7-gate-trend-compare.ps1` - WS7 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws7-plugin-stability-badge.ps1` - WS7 post-gate artifact export (plugin stability badge JSON)
- `scripts/run-ws7-release-summary.ps1` - WS7 release-facing readiness summary (single artifact from WS7 summary + matrix + trend + badge)
- `scripts/run-ws7-gate.ps1` - WS7 gate orchestrator (plugin boundary + integrity + policy packs + matrix/trend/badge/release summary)
- `scripts/run-ws7-closure-gate.ps1` - WS7 closure gate (validation posture checks over WS7 gate + release artifacts)
- `scripts/run-release-r3-plugin-gate.ps1` - R3 release plugin readiness gate (WS7 closure + WS9A gate linkage)
- `scripts/run-ws8-control-plane-smoke.ps1` - WS8 control-plane typed action record baseline smoke
- `scripts/run-ws8-guardrail-policy-smoke.ps1` - WS8 autonomous guardrail policy smoke (route + emergency-stop + trace-id checks)
- `scripts/run-ws8-mode-governance-smoke.ps1` - WS8 autonomous mode-governance smoke (policy deny-mode + blast-radius safeguards)
- `scripts/run-ws8-autonomy-matrix-export.ps1` - WS8 post-gate artifact export (autonomous control-plane compliance matrix)
- `scripts/run-ws8-gate-trend-compare.ps1` - WS8 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws8-autonomy-stability-badge.ps1` - WS8 post-gate artifact export (autonomy stability badge JSON)
- `scripts/run-ws8-release-summary.ps1` - WS8 release-facing readiness summary (single artifact from WS8 summary + matrix + trend + badge)
- `scripts/run-ws8-gate.ps1` - WS8 gate orchestrator (control-plane + guardrail + audit linkage packs + matrix/trend/badge/release summary)
- `scripts/run-ws8-closure-gate.ps1` - WS8 closure gate (validation posture checks over WS8 gate + release artifacts)
- `scripts/run-ws8a-audit-smoke.ps1` - WS8A audit trail baseline smoke
- `scripts/run-ws8a-audit-companion-smoke.ps1` - WS8A audit companion query/export flow smoke
- `scripts/run-ws8a-agent-authoring-smoke.ps1` - WS8A AI agent authoring smoke (object/plugin workflow guardrails + policy hooks)
- `scripts/run-ws8a-agent-authoring-matrix-export.ps1` - WS8A post-gate artifact export (agent authoring controls matrix)
- `scripts/run-ws8a-gate-trend-compare.ps1` - WS8A post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws8a-agent-stability-badge.ps1` - WS8A post-gate artifact export (agent-authoring stability badge JSON)
- `scripts/run-ws8a-release-summary.ps1` - WS8A release-facing readiness summary (single artifact from WS8A summary + matrix + trend + badge)
- `scripts/run-ws8a-gate.ps1` - WS8A gate orchestrator (audit trail + companion + agent authoring packs + matrix/trend/badge/release summary)
- `scripts/run-ws8a-closure-gate.ps1` - WS8A closure gate (validation posture checks over WS8A gate + release artifacts)
- `scripts/run-release-r3-agent-authoring-gate.ps1` - R3 release AI agent-authoring readiness gate (WS8A closure + WS8 + WS7 linkage)
- `scripts/run-release-r3-autonomous-gate.ps1` - R3 release autonomous readiness gate (WS8 closure + WS7 closure linkage)
- `scripts/run-ws9-studio-smoke.ps1` - WS9 Studio API contract smoke (script execution + endpoint/header/type checks)
- `scripts/run-ws9-gate.ps1` - WS9 gate orchestrator (runs studio smoke and emits one gate summary artifact)
- `scripts/run-ws9a-ide-contract-smoke.ps1` - WS9A IDE extension API contract smoke
- `scripts/run-ws9a-gate.ps1` - WS9A gate orchestrator (runs IDE contract smoke and emits one gate summary artifact)
- `scripts/run-ws10-driver-smoke.ps1` - WS10 driver request/session-routing baseline smoke
- `scripts/run-ws10-gate.ps1` - WS10 gate orchestrator (runs driver contract smoke and emits one gate summary artifact)
- `scripts/run-release-dx-api-gate.ps1` - Combined DX/API contract cluster gate (WS5 + WS9 + WS9A + WS10) with release-readiness summary artifact
- `scripts/run-release-ops-resilience-gate.ps1` - Combined Ops/Resilience cluster gate (WS12 + WS13 + WS14) with R2/R3 release-readiness summary artifact
- `scripts/run-ws11-i18n-smoke.ps1` - WS11 i18n/UTF-8 smoke (locale parsing + fallback policy checks)
- `scripts/run-ws11-gate.ps1` - WS11 gate orchestrator (runs i18n smoke and emits one gate summary artifact)
- `scripts/run-ws22-pessimistic-lock-smoke.ps1` - WS22 pessimistic-lock baseline smoke (lock acquire/release route + conflict/ownership contract checks)
- `scripts/run-ws22-gate-trend-compare.ps1` - WS22 post-gate artifact export (current vs prior gate trend comparator)
- `scripts/run-ws22-pessimistic-lock-stability-badge.ps1` - WS22 post-gate artifact export (pessimistic-lock stability badge JSON)
- `scripts/run-ws22-release-summary.ps1` - WS22 release-facing lock readiness summary (single artifact from WS22 summary + smoke + trend + badge)
- `scripts/run-ws22-gate.ps1` - WS22 gate orchestrator (smoke + trend + stability badge + release summary)
- `scripts/run-ws22-closure-gate.ps1` - WS22 closure gate (validation posture checks over WS22 gate + lock contract evidence)
- `scripts/run-ws12-reliability-smoke.ps1` - WS12 reliability/SRE baseline smoke (health + rate-limit + failure-budget alerts + DR hook persistence/scheduler/policy/retry-plan + failure signal reconciliation + gate evaluation/export contracts)
- `scripts/run-ws13-multicloud-profile-smoke.ps1` - WS13 multi-cloud deployment profile baseline smoke (AWS/Azure/GCP profile contracts + KPI cloud config alignment)
- `scripts/run-ws13-overlay-schema-smoke.ps1` - WS13 cloud overlay/schema validation smoke (single-node/multi-node overlays + provider Helm value contracts)
- `scripts/run-ws13-env-matrix-smoke.ps1` - WS13 runbook env-matrix validation smoke (deploy/cloud/*/README.md coverage for required provider env vars)
- `scripts/run-ws13-gate.ps1` - WS13 CI gate orchestrator (runs all three WS13 smoke packs and emits one gate summary artifact)
- `scripts/run-ws14-config-smoke.ps1` - WS14 driver/security config contract baseline smoke
- `scripts/run-ws14-schema-lint-gate.ps1` - WS14 schema lint gate for YAML/JSON/properties config contracts
- `scripts/run-ws14-config-conformance-aggregate.ps1` - WS14 config contract conformance aggregator (cross-format parity and conformance score)
- `scripts/run-ws14-gate.ps1` - WS14 gate orchestrator (runs baseline + schema lint + conformance and emits one gate summary artifact)
- `scripts/run-ws15-competitive-parity-smoke.ps1` - WS15 competitive feature adoption baseline smoke (matrix presence + competitor coverage + feature contract completeness)
- `scripts/run-ws15-backlog-score-smoke.ps1` - WS15 backlog scoring validation smoke (impact/effort coverage + priority formula + owner completeness)
- `scripts/run-ws15-gate.ps1` - WS15 gate orchestrator (runs parity + backlog scoring and emits one gate summary artifact)
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
