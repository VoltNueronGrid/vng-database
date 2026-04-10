#!/usr/bin/env pwsh
# Update Section 5.22 heading + add H-02 row; update sprintwise tracker

$mainFile = "D:\by\polap-db\status_tracker.md"
$sprintFile = "D:\by\polap-db\status-tracker-sprintwise-v1.md"

# --- Update main tracker ---
$lines = Get-Content $mainFile

# Update Section 5.22 heading (line 300, index 299)
$lines[299] = '## 5.22) Gate Reality Check (2026-04-10 Audit — session 126 refresh)'

# Update Section 5.22 intro (line 301, index 300-301)
$lines[300] = ''
$lines[301] = 'Actual gate artifact status verified by code/artifact inspection and live runs. Session 126 additions: H-02 HTAP sync gate orchestrated — all 4 packs passed, `h02-gate-summary.json` and `h02-release-readiness.json` (`ready_for_validation`) created; H-02 promoted from In Progress to Ready for Validation; stale test counts in footer updated to 671/200/70/366 (session 126 audit of `#[test]` annotations). Overrides any stale editorial status above.'

# Find WS5 gate row and insert H-02 row before it
$ws5idx = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
  if ($lines[$i] -match '^\| WS5 gate \|') { $ws5idx = $i; break }
}

Write-Host "WS5 gate at line: $($ws5idx + 1)"

if ($ws5idx -gt 0) {
  $h02row = '| H-02 gate | **passed** (2026-04-10 session 126) | **ready_for_validation** | `h02-gate-summary.json` (4/4 packs: restart-replay-matrix, multi-node-handoff-matrix, sync-fault-injection, reorder-duplicate-faults); all store crate matrix tests pass; `tests/kpi/results/gates/h02-release-readiness.json` generated. |'
  $newLines = [System.Collections.Generic.List[string]]$lines
  $newLines.Insert($ws5idx, $h02row)
  Set-Content $mainFile $newLines -Encoding UTF8
  Write-Host "H-02 row inserted at line $($ws5idx + 1)"
} else {
  Set-Content $mainFile $lines -Encoding UTF8
  Write-Host "WS5 not found; saved other changes"
}

# --- Update sprintwise tracker ---
$sl = Get-Content $sprintFile

# Update Last updated
for ($i = 0; $i -lt $sl.Count; $i++) {
  if ($sl[$i] -match '\*\*Last updated:\*\*') { $sl[$i] = '**Last updated:** 2026-04-10 (session 126)'; break }
}

# Update Sprint 9 status (H-02 now ready)
for ($i = 0; $i -lt $sl.Count; $i++) {
  if ($sl[$i] -match 'Sprint 9.*Competitive.*P0 Hardening') {
    $sl[$i] = $sl[$i] -replace '🔵 Mixed', '🟡 Mixed (H-02 promoted to Ready for Validation 2026-04-10)'
    break
  }
}

# Update H-02 row in workstream table
for ($i = 0; $i -lt $sl.Count; $i++) {
  if ($sl[$i] -match '^\| H-02 \|.*In Progress') {
    $sl[$i] = '| H-02 | Epic 6 | HTAP sync correctness under failures | Storage + Distributed Systems | 🟡 Ready for Validation | WS2, WS6 | Gate orchestrator + release-readiness artifact: 4/4 packs passed (2026-04-10, session 126): restart-replay-matrix (7 checks), multi-node-handoff-matrix (3 checks), sync-fault-injection, reorder-duplicate-faults; `tests/kpi/results/h02/h02-gate-summary.json`; `tests/kpi/results/gates/h02-release-readiness.json` (`ready_for_validation`) |'
    Write-Host "Updated H-02 row in sprintwise"
    break
  }
}

Set-Content $sprintFile $sl -Encoding UTF8
Write-Host "Sprintwise tracker updated"
