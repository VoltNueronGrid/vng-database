# AWS Deployment Runbook (WS13)

## Artifacts

- Profile contract: `deploy/cloud/aws/profile.yaml`
- Single-node overlay: `deploy/cloud/aws/single-node-overlay.yaml`
- Multi-node overlay: `deploy/cloud/aws/multi-node-overlay.yaml`
- Helm overrides: `deploy/cloud/aws/helm-values.yaml`

## Environment variable matrix

| Variable | Purpose |
|---|---|
| `VNG_AWS_REGION` | AWS region for deployment target |
| `VNG_AWS_IMAGE_TAG` | Runtime image tag |
| `VNG_AWS_ADMIN_API_KEY` | Admin API key injected to runtime |
| `VNG_AWS_BASE_URL` | KPI/runtime gateway endpoint |
| `VNG_AWS_SQL_URL` | KPI SQL endpoint |
| `VNG_AWS_BEARER_TOKEN` | KPI auth token for remote smoke |

## Quick start (placeholder)

1. Export env vars from matrix.
2. Apply profile + overlay to your deployment toolchain.
3. Deploy Helm chart using `deploy/cloud/aws/helm-values.yaml`.
4. Run KPI cloud smoke against configured AWS endpoints.
