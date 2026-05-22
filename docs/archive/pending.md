# Pending work — VoltNueronGrid DB (`polap-db`)

**Last updated:** 2026-04-13

**Program sign-off:** Recorded as **approved end-to-end from program owner** for governance intent. Remaining items below are **technical execution, environment, and evidence updates** unless your organization still requires a separate formal sign-off record (e.g. ticketing system, audit log, or Release DRI name on a document).

---

## 1. What “65% REQs” and “choose which proof to fund first” means

Many requirements (REQ-07, REQ-08, REQ-10, REQ-19, REQ-21, etc.) are marked **~65%** because this repo already has:

- working **code paths** (APIs, stores, benchmarks scaffolds, smoke scripts), and  
- **automated tests** that prove behavior in a dev/single-process setting.

They are **not** at 100% because the **full product promise** of each REQ still needs **one or more “proofs”** that cost time, hardware, or cloud money—examples:

| Type of proof | What it demonstrates | Typical cost |
|---|---|---|
| **Benchmark tier** | Ingest/query throughput and latency against **real** storage and CPU limits, not synthetic counters in one process. | Engineering time + perf lab or beefy VMs |
| **Cloud** | Deploy actually runs in **AWS/Azure/GCP** with real endpoints, TLS, and ops dashboards—not only `profile.yaml` checks. | Cloud account + credentials + SRE time |
| **Load / sustained** | Many concurrent users or long-running jobs without drift, OOM, or deadlock—beyond a few `ws21_*` tests. | Load harness + environment + analysis time |
| **Trillion-row class** (REQ-10 wording) | Scale story: sharding, disk, or distributed query path proven at agreed **data volume** targets. | Architecture + storage + often weeks of work |

**“Choose which proof to fund first”** means: **you** (program) pick **one primary vertical** to prove next, for example:

1. *“Prove cloud SaaS path first”* → prioritize PR-007 live smoke, REQ-08 end-to-end on a real cluster, runbooks.  
2. *“Prove ingest throughput first”* → prioritize REQ-07/REQ-19 benchmarks on nominated hardware and capture KPI artifacts.  
3. *“Prove concurrency at HTTP layer first”* → prioritize REQ-21 HTTP harness + sustained load tests.

Until one of those **funded proofs** completes, the tracker can honestly keep **65%** even though day-to-day coding is “done” for the scaffold. It is **not** asking you to write more random features—it is asking for a **prioritized validation program**.

---

## 2. R1–R3 and R4 — sign-off approved; what may still be needed

You indicated **approval** for shipping **R1–R3** and for **unblocking R4** from a program perspective. That removes **decision** blockers. The following may still be **required** for a clean release record:

### R1–R3 (after program approval)

1. **Formalize the record** (if auditors/compliance matter): add Release DRI name, date, and pointer to gate JSON paths in your release ticket or changelog.
2. **CI green on main**: ensure the branch that ships matches the commit set used for gate artifacts (or re-run gates on the release SHA and attach artifacts).
3. **Tag / version**: git tag or semver bump per your release process.
4. **Operational handoff**: runbooks, known limitations (e.g. WS8 runtime pack `not_included` in some summaries—confirm acceptable).

### R4 (unblock path)

Program approval helps, but **R4** was **technically** blocked because **H-09 / H-10** release readiness was not yet `ready_for_validation` and ops/game-day items were outstanding. After approval, **concrete work** is still:

1. **H-09**: Execute and document live IDE/runtime parity and permission-negative scenarios; refresh `h09-release-readiness.json` to match program criteria.
2. **H-10**: Complete ARB/deprecation-registry steps your process defines; refresh `h10-release-readiness.json`.
3. **Ops / game-day**: Run RTO/RPO or equivalent drill if still required by the gate narrative; attach evidence.
4. **Re-run** `tests/kpi/scripts/run-release-r4-saas-maturity-gate.ps1` (or the canonical R4 gate script) and confirm `release_readiness` flips to **ready** per your rules.

**If nothing else is needed from you:** once the above evidence exists in-repo and gates pass, R4 can align with your approval. If external auditors require a **named** Release DRI signature on a PDF, that is outside this repo.

---

## 3. Comprehensive steps — items from your list

### 3.1 R4 still blocked (H-09, H-10, ops/game-day)

1. **H-09 — IDE parity / safety**
   - [ ] Inventory IDE surfaces (VS Code/Cursor/JetBrains/etc.) against `ws9a` contract artifacts.
   - [ ] Run live or scripted scenarios: connect, execute SQL, error paths, permission denials.
   - [ ] Record failures/gaps; fix code or document accepted gaps.
   - [ ] Re-run `run-h09-gate.ps1` (or repo equivalent); update `tests/kpi/results/h09/*` and `h09-release-readiness.json` until `ready_for_validation` (or your approved equivalent).

2. **H-10 — Governance / maintainability**
   - [ ] Hold or async-complete ARB decision on listed interfaces/deprecations per `reference` docs.
   - [ ] Publish **deprecation registry v1** (file or doc under `reference/` or `docs/` as per convention).
   - [ ] Re-run H-10 gate; refresh `h10-release-readiness.json`.

3. **Ops / game-day**
   - [ ] Schedule drill: failover, backup/restore, or RTO/RPO scenario as required by `release-r4-saas-maturity-readiness.json`.
   - [ ] Capture timeline, outcome, sign-off name (if required).
   - [ ] Link evidence in tracker or `tests/kpi/results/gates/`.

4. **Close R4 gate**
   - [ ] Re-run full R4 release gate script.
   - [ ] Verify `release_readiness` is no longer `blocked`.
   - [ ] Update `status-tracker.md` R4 row.

---

### 3.2 PR-007 — deferred; cloud endpoint + token for remote smoke

1. **Provision** non-prod cloud resources (single- and multi-node as per `deploy/cloud/*` profiles).
2. **Inject secrets** into CI or a secure runner: API keys, kubeconfig, cloud tokens (never commit plaintext).
3. **Configure** env vars expected by `bootstrap-phase3.ps1` / smoke scripts (see `tests/kpi/README.md` and deploy READMEs).
4. **Run** phase-3 smoke without `-AllowMissingEnv` (or with full env); fix failures (network, RBAC, Helm).
5. **Archive** results under `tests/kpi/results/` and mark PR-007 **Done** or raise completion % in tracker.

---

### 3.3 H-05 — Azure Key Vault path deferred

1. **Decide** whether Azure KV drill is in scope for the next milestone.
2. If yes: **create** vault/keys; grant identity used by runtime; supply **key IDs + credentials** via secret store.
3. **Run** `h05` regional failover smokes against live Azure provider.
4. **Update** tracker: H-05 from Deferred → In Progress → Ready for Validation.

---

### 3.4 H-01 / H-03 — `in_progress_with_evidence`

**H-01 (blast radius / autonomy)**

1. [ ] Define “cross-channel” and “resource-scoped RBAC” certification checklist (which roles, which APIs).
2. [ ] Run negative tests: deny paths for tenant vs operator across failover, ingest, SQL, autonomous endpoints.
3. [ ] Document blast-radius matrix; attach to `h01-release-readiness.json` or supporting docs.
4. [ ] Re-run H-01 gate; promote readiness when blockers in JSON are cleared.

**H-03 (control-plane / cluster runtime)**

1. [ ] Deploy **multi-node** or **multi-process** cluster with real transport (not only unit mocks).
2. [ ] Run chaos packs: partition, slow links, degraded failover as defined in WS6/H-03 scripts.
3. [ ] Capture logs and gate artifacts; clear `full_inter_process_transport_backed_cluster_runtime_certification_pending` when done.

---

### 3.5 H-09 / H-10 — 65%; block R4

Covered in **§3.1**. Treat H-09 and H-10 as **gating** for R4 until artifacts show `ready_for_validation`.

---

### 3.6 Many REQs at 65% — benchmarks, cloud, load, scale proof

**Program-level sequence (suggested):**

1. **Pick order** (one primary track): Cloud first **or** Benchmark first **or** Load first (see §1).
2. **For cloud (REQ-08, PR-007):** follow §3.2.
3. **For benchmarks (REQ-07, REQ-10, REQ-19):** nominate machine class; run `run-req10-benchmark-smoke.ps1` and extended ingest/query benchmarks; store JSON under `tests/kpi/results/req10/` (and similar); set targets in KPI tables.
4. **For load (REQ-21):** add or extend HTTP-level load tests (k6, hey, custom) against `voltnuerongridd`; run sustained duration; file results.
5. **For trillion-row / scale (REQ-10):** only after architecture for shard/disk path is agreed; may be a **multi-phase** program—split into MVP scale target first.

6. **Update tracker:** bump REQ % only when evidence exists (avoid speculative uplifts).

---

## 4. Single checklist — “what do I do next?”

If you want one linear order after **program approval**:

1. **Prioritize proof track** (§1): cloud vs benchmark vs load.  
2. **Unblock R4**: finish **H-09**, **H-10**, ops game-day (§3.1).  
3. **Parallel**: PR-007 credentials when cloud is priority (§3.2).  
4. **H-01 / H-03**: schedule env for certification (§3.4).  
5. **H-05**: only if Azure drill is mandatory (§3.3).  
6. **REQ 65%s**: execute funded proofs and refresh KPI artifacts (§3.6).

---

## 5. References (in-repo)

- Gate results: `tests/kpi/results/`, `tests/kpi/scripts/`
- Canonical status: `status-tracker.md`
- KPI harness notes: `tests/kpi/README.md`
