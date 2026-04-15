# Credentials and Connection Model

This extension uses two different credential planes.

## 1) Runtime connection credentials (database access)

These are used by the extension to call VoltNueronGrid runtime APIs.

- Base URL: where runtime is deployed
  - Local: http://127.0.0.1:8080
  - Docker: http://host.docker.internal:8080 (or mapped localhost)
  - Cloud: https://your-cloud-vng-endpoint
  - Custom: any HTTP(S) endpoint
- Identity headers (mode-specific):
  - Admin mode: x-vng-admin-key
  - Operator mode: x-vng-admin-key + x-vng-operator-id
  - Tenant mode: x-vng-tenant-id + x-vng-user-id

These are not tied to any specific hosting model. The same extension works for local, docker, cloud, or hybrid as long as the base URL and headers are valid.

## 2) Publishing credentials (extension distribution)

These are only required to package/publish extension artifacts.

- npm registry access for dev dependency install (if registry requires auth)
- VSIX publishing token/process for your private feed
- Optional VS Code publisher token if using VS Code Marketplace tooling

If npm install fails with 401, refresh npm auth (npm login or token configuration for the active registry), then run build/package again.
