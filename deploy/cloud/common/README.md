# WS13 Multi-Cloud Runbook Contract

This directory defines the shared deployment contract for WS13 multi-cloud profiles.

## Required profile fields

See `profile-contract.yaml` for the source of truth.

- `cloud.provider`
- `cloud.region_env`
- `runtime.kubernetes_namespace`
- `runtime.image_repository`
- `runtime.image_tag_env`
- `runtime.service.port`
- `runtime.dr_hook_state_path`
- `security.auth.mode`
- `security.auth.admin_key_env`

## Provider runbooks

- AWS: `deploy/cloud/aws/README.md`
- Azure: `deploy/cloud/azure/README.md`
- GCP: `deploy/cloud/gcp/README.md`
