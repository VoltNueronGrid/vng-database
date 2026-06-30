# Deployment Parity Matrix — Local vs Cloud

> **Task E-8** (Tasks-7). Documents capability parity between the local
> single-/multi-node developer topology and the cloud (AWS / Azure / GCP)
> production topology, and explicitly flags the gaps. The companion validation
> gate `tests/kpi/scripts/run-e8-parity-gate.sh` runs the **same** smoke suite
> against local single-node and local multi-node so the developer topology is
> exercised with the identical checks used for cloud overlays.

## 1. Topologies

| Topology | Source | Bring-up |
|---|---|---|
| Local single-node | `deploy/local/single-node.yml` | `cargo run -p voltnuerongridd` or compose |
| Local multi-node (3) | `deploy/local/multi-node.yml` | `docker compose -f deploy/local/multi-node.yml up` |
| Cloud AWS | `deploy/cloud/aws/*` + `deploy/helm/voltnuerongrid` | Helm + overlay |
| Cloud Azure | `deploy/cloud/azure/*` + Helm | Helm + overlay |
| Cloud GCP | `deploy/cloud/gcp/*` + Helm | Helm + overlay |

All topologies share the same binary and the same multi-cloud profile contract
(`deploy/cloud/common/profile-contract.yaml`).

## 2. Capability Parity Matrix

Legend: ✅ supported · ⚠️ supported with caveats · ☁️ cloud-only · 🔧 local-only

| Capability | Local single | Local multi | Cloud | Notes |
|---|:---:|:---:|:---:|---|
| HTTP control/data plane (`:8080`) | ✅ | ✅ | ✅ | Identical handler surface. |
| PostgreSQL wire (`:5432`) | ✅ | ✅ | ✅ | Same pgwire listener. |
| SQL OLTP / OLAP / HTAP routing | ✅ | ✅ | ✅ | Same router across topologies. |
| Raft consensus + leader election | ⚠️ single-node self-elect | ✅ | ✅ | Multi-node needed for real quorum. |
| Failover RTO ≤ 30 s / RPO = 0 | n/a | ✅ (E-5) | ✅ | Validated live by `run-e5-failover-gate.sh`. |
| RBAC (admin/operator/tenant) | ✅ | ✅ | ✅ | Same auth enforcement order. |
| Autonomous actions (audited + policy) | ✅ | ✅ | ✅ | E-7 coverage = 100%. |
| Encryption-at-rest (KMS) | ⚠️ in-memory refs | ⚠️ in-memory refs | ✅ provider KMS | Cloud resolves `VNG_KMS_*` to provider KMS. |
| TLS / mTLS native listener | ⚠️ optional/self-signed | ⚠️ optional/self-signed | ✅ managed certs | Local disables via `VNG_NATIVE_LISTENER_ENABLED=false`. |
| Object-storage cold tier | 🔧 local FS | 🔧 local FS | ☁️ S3 / Blob / GCS | **Gap:** object storage is cloud-only. |
| Managed autoscaling | ❌ | ❌ | ☁️ HPA / node pools | **Gap:** elastic scale is cloud-only. |
| Multi-AZ / region replication | ❌ | ⚠️ same-host | ☁️ | **Gap:** cross-AZ is cloud-only. |
| Cluster-scale KPI targets¹ | ❌ | ⚠️ scaled-hw | ☁️ | See §3. |

¹ See the KPI profile note below.

## 3. KPI profile parity (E-1 … E-4)

The README headline KPI numbers (OLTP p95 ≤ 20 ms at ≥ 64 sustained
connections, HTAP ≥ 25 000 rqps, ingest linear scaling) are **cluster-aggregate
targets on production-class hardware**. The KPI harness (`tests/kpi-harness`,
binary `vng-kpi`) measures them with real concurrent load and ships two gate
profiles so parity is explicit rather than implied:

| Profile | Used by | Targets | Where it runs |
|---|---|---|---|
| `local` | `run-e-all-gate.sh local` | Per-node sustainable load (latency SLA met at low concurrency) | Dev laptop / single node |
| `cluster` | `run-e-all-gate.sh cluster` | Full README cluster-aggregate targets | Sharded cloud cluster on scaled hardware |

**Gap flagged:** a single local node meets the latency SLA (p95 ≈ 4–14 ms at
1–4 concurrent connections) but cannot reach the ≥ 64-connection / 25 k-rqps
aggregate throughput — those require horizontal sharding across cloud nodes.
The harness logic is unit-tested and the live runs emit real artifacts under
`tests/kpi/results/e/`; the `cluster` profile carries the README targets to the
cloud topology where the hardware can satisfy them.

## 4. Validation

`tests/kpi/scripts/run-e8-parity-gate.sh` runs an identical **smoke suite**
against local single-node and local multi-node:

1. `/health` readiness
2. SQL roundtrip — `CREATE TABLE` → `INSERT` → `SELECT` returns the row
3. RBAC — unauthenticated control-plane call is rejected (401)
4. Raft status reachable (and, for multi-node, a leader is elected)

The same logical checks back the cloud smoke suite (the cloud overlays differ
only in image source, secrets wiring, and managed-service endpoints — all
captured by the profile contract). The gate emits
`tests/kpi/results/e/e8-parity-validation.json` and a gate summary; any check
that passes locally but is cloud-only is recorded in the matrix above rather
than silently skipped.

## 5. Explicit parity gaps (summary)

- **Object-storage cold tier** — cloud-only (S3 / Azure Blob / GCS); local uses the filesystem tier.
- **Managed autoscaling / elasticity** — cloud-only (Kubernetes HPA + node pools).
- **Multi-AZ / cross-region replication** — cloud-only; local multi-node co-locates on one host.
- **Provider-managed KMS + managed TLS certs** — cloud-only resolution; local uses in-memory key refs and optional self-signed TLS.
- **Cluster-scale KPI throughput targets** — require sharded cloud hardware; local validates the per-node latency SLA + correctness, the `cluster` profile carries the aggregate targets.
