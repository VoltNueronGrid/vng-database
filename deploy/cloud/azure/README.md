# Azure Deployment Runbook (WS13)

## Artifacts

- Profile contract: `deploy/cloud/azure/profile.yaml`
- Single-node overlay: `deploy/cloud/azure/single-node-overlay.yaml`
- Multi-node overlay: `deploy/cloud/azure/multi-node-overlay.yaml`
- Helm overrides: `deploy/cloud/azure/helm-values.yaml`

## Environment variable matrix

| Variable | Purpose |
|---|---|
| `VNG_AZURE_REGION` | Azure region for deployment target |
| `VNG_AZURE_IMAGE_TAG` | Runtime image tag |
| `VNG_AZURE_ADMIN_API_KEY` | Admin API key injected to runtime |
| `VNG_AZURE_BASE_URL` | KPI/runtime gateway endpoint |
| `VNG_AZURE_SQL_URL` | KPI SQL endpoint |
| `VNG_AZURE_BEARER_TOKEN` | KPI auth token for remote smoke |

## Quick start (placeholder)

1. Export env vars from matrix.
2. Apply profile + overlay to your deployment toolchain.
3. Deploy Helm chart using `deploy/cloud/azure/helm-values.yaml`.
4. Run KPI cloud smoke against configured Azure endpoints.
