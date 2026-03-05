# GCP Deployment Runbook (WS13)

## Artifacts

- Profile contract: `deploy/cloud/gcp/profile.yaml`
- Single-node overlay: `deploy/cloud/gcp/single-node-overlay.yaml`
- Multi-node overlay: `deploy/cloud/gcp/multi-node-overlay.yaml`
- Helm overrides: `deploy/cloud/gcp/helm-values.yaml`

## Environment variable matrix

| Variable | Purpose |
|---|---|
| `VNG_GCP_REGION` | GCP region for deployment target |
| `VNG_GCP_IMAGE_TAG` | Runtime image tag |
| `VNG_GCP_ADMIN_API_KEY` | Admin API key injected to runtime |
| `VNG_GCP_BASE_URL` | KPI/runtime gateway endpoint |
| `VNG_GCP_SQL_URL` | KPI SQL endpoint |
| `VNG_GCP_BEARER_TOKEN` | KPI auth token for remote smoke |

## Quick start (placeholder)

1. Export env vars from matrix.
2. Apply profile + overlay to your deployment toolchain.
3. Deploy Helm chart using `deploy/cloud/gcp/helm-values.yaml`.
4. Run KPI cloud smoke against configured GCP endpoints.
