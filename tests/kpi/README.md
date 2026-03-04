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
- `results/` - output folder for run artifacts

## Usage

PowerShell:

`pwsh ./tests/kpi/scripts/run-kpi.ps1 -BaseUrl "http://localhost:8080" -SqlUrl "http://localhost:8080" -OutputDir "./tests/kpi/results/week1" -TargetsPath "./tests/kpi/config/targets.yaml" -AuthMode "none"`

Bash:

`bash ./tests/kpi/scripts/run-kpi.sh "http://localhost:8080" "http://localhost:8080" "./tests/kpi/results/week1" "./tests/kpi/config/targets.yaml" "none"`

Cloud smoke packs (PowerShell):

`pwsh ./tests/kpi/scripts/run-cloud-smoke.ps1 -OutputRootDir "./tests/kpi/results/20260304-pr007/cloud-profiles" -CloudProfilesPath "./tests/kpi/config/cloud-profiles.yaml" -TargetsPath "./tests/kpi/config/targets.yaml"`

## Notes

- Thresholds are loaded from `config/targets.yaml`.
- Each run writes per-scenario JSON plus a `rollup-summary.json` file.
