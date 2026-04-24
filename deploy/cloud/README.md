# Cloud Deployment — VoltNueronGrid

**Document:** S11-004  
**Status:** Deferred — cloud deployment is not part of the v0.1.0 release candidate  
**Last updated:** 2026-04-22

---

## Status

Cloud deployment for VoltNueronGrid is **deferred to S12+**. The v0.1.0 release
candidate targets local and on-premises single-node deployments only.

For local installation, see [../local/README.md](../local/README.md).

---

## What is available now

The `deploy/cloud/` directory contains early-stage infrastructure templates:

| Path | Description | Status |
|------|-------------|--------|
| `deploy/cloud/aws/` | AWS CloudFormation / ECS task definitions | Draft — not tested |
| `deploy/cloud/azure/` | Azure Container Apps templates | Draft — not tested |
| `deploy/cloud/gcp/` | GCP Cloud Run / GKE manifests | Draft — not tested |
| `deploy/cloud/helm/` | Helm chart for Kubernetes | Draft — not tested |
| `deploy/cloud/common/` | Shared config fragments | Draft |
| `deploy/cloud/multi-node.yml` | Multi-node compose reference | Draft |
| `deploy/cloud/single-node.yml` | Single-node compose reference | Draft — use local guide for now |

---

## Cloud deployment target (S12+)

When cloud deployment is addressed in Sprint S12+, the following will be delivered:

1. **Docker image build** — `Dockerfile` for `voltnuerongridd` with multi-stage build
2. **Helm chart** — production-ready chart with configurable replicas, TLS, RBAC
3. **AWS reference deployment** — ECS Fargate or EC2 with ALB + RDS data plane
4. **Azure reference deployment** — ACA + Azure Files for data directory
5. **GCP reference deployment** — Cloud Run + GCS-backed storage
6. **Cloud load testing** — validates R-10 (trillion-row scale) in cloud environment
7. **Smoke test suite** — cloud equivalent of local health-check verification

---

## Prerequisites for cloud deployment (when available)

- Docker 24+ or Podman 4+
- Kubernetes 1.27+ (for Helm chart)
- Helm 3.12+ (for Helm chart)
- Cloud CLI configured with appropriate permissions

---

## References

- [Local installation guide](../local/README.md)
- [Security compliance checklist](../../services/voltnuerongridd/reference/security-compliance-checklist-v1.md)
- [Compatibility matrix](../../services/voltnuerongridd/reference/compatibility-matrix-v1.md)
- [NT-S11-001 governance document](../../services/voltnuerongridd/reference/nt-s11-001-dual-transport-governance-v1.md)
