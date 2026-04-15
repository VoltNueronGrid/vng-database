# Status TODO - IDE + MCP Closeout

Last updated: 2026-04-15

## 1) Current Completion Snapshot

### Completed
- IDE-001: Phase 1 target selected (VSCode/Cursor).
- IDE-002: Connection wizard implemented with mode-based auth inputs.
- IDE-003: Query runner, query diagnostics, schema registry commands implemented.
- IDE-004: Auth-aware permission handling and user feedback implemented.
- IDE-005 (partial): VSIX built successfully:
  - `ui/ide-extensions/vscode-cursor/voltnuerongrid-vscode-cursor-0.1.0.vsix`
- IDE-006 (partial): Phase 2 scaffolds for AntiGravity, Windsor, Eclipse, Jetbrains added.

### In Progress / Pending
- IDE-005 final publish to private feed.
- IDE-006 full native implementation per IDE SDK (currently scaffold-only).

---

## 2) Blockers On User (Inputs/Access Needed)

These are the exact blockers requiring your input or environment changes.

1. Private feed endpoint details
- Azure DevOps organization URL
- Project name
- Feed name (or exact artifact destination)

2. Publish authentication source
- PAT token source to use in this session
  - Either secure manual input
  - Or environment variable name already set
- PAT permissions/scope for feed write

3. Azure DevOps CLI extension readiness (if publish via CLI)
- Current machine shows Azure DevOps extension install issue (pip status 1).
- Need permission to remediate Python/pip/CLI extension environment or switch to non-CLI publish path.

4. Preferred publish route
- Option A: Azure CLI automated publish
- Option B: Web UI/manual upload to private feed (fastest if CLI remains unstable)

---

## 3) Comprehensive Local Test Steps (Runtime + MCP + VSCode/Cursor)

This runbook assumes Windows PowerShell and local workspace at `D:\by\polap-db`.

### 3.1 Preflight: clean shell and ports

Because profile scripts on this machine can inject conda errors, prefer `pwsh -NoProfile` for gate/smoke scripts.

```powershell
Set-Location "D:\by\polap-db"

# Optional: free runtime ports first
$ports = 8080,8081
$listeners = Get-NetTCPConnection -LocalPort $ports -State Listen -ErrorAction SilentlyContinue
$procIds = @($listeners | Select-Object -ExpandProperty OwningProcess -Unique)
foreach ($id in $procIds) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
```

### 3.2 Start runtime locally

Open Terminal A:

```powershell
Set-Location "D:\by\polap-db"
$env:VNG_ADMIN_API_KEY = "secret"
$env:VNG_HTTP_BIND = "127.0.0.1:8080"
cargo run -p voltnuerongridd
```

Health probe (Terminal B):

```powershell
Invoke-RestMethod -Uri "http://127.0.0.1:8080/health"
```

Expected: JSON includes status ok.

### 3.3 Core workspace validation

```powershell
Set-Location "D:\by\polap-db"

cargo check -p voltnuerongridd
cargo test -p voltnuerongridd ws2_catalog_table_columns -- --test-threads=1
cargo test -p voltnuerongrid-sql
cargo test -p voltnuerongrid-store
cargo test -p voltnuerongrid-ingest
cargo test -p voltnuerongrid-mcp
```

Optional full suite:

```powershell
cargo test --workspace -- --test-threads=1
```

### 3.4 MCP local validation

#### 3.4.1 MCP crate tests

```powershell
Set-Location "D:\by\polap-db"

cargo test --lib -p voltnuerongrid-mcp
cargo test --test integration_tests -p voltnuerongrid-mcp
```

#### 3.4.2 MCP HTTP endpoint checks against running runtime

Admin health:

```powershell
$headers = @{ "Content-Type" = "application/json"; "x-vng-admin-key" = "secret" }
$body = '{"jsonrpc":"2.0","id":"1","method":"tools/health","params":{}}'
Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8080/api/v1/mcp" -Headers $headers -Body $body
```

Operator schema:

```powershell
$headers = @{ "Content-Type" = "application/json"; "x-vng-operator-id" = "op-001" }
$body = '{"jsonrpc":"2.0","id":"2","method":"tools/schema","params":{}}'
Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8080/api/v1/mcp" -Headers $headers -Body $body
```

Tenant query:

```powershell
$headers = @{ "Content-Type" = "application/json"; "x-vng-tenant-id" = "tenant-123"; "x-vng-user-id" = "user-456" }
$body = '{"jsonrpc":"2.0","id":"3","method":"tools/query","params":{"sql_query":"SELECT 1"}}'
Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8080/api/v1/mcp" -Headers $headers -Body $body
```

Expected outcomes:
- Valid auth/mode => 200 responses.
- Missing/invalid auth => 401.
- Insufficient privilege => 403.

### 3.5 VSCode/Cursor extension local validation

#### 3.5.1 Build/package extension

```powershell
Set-Location "D:\by\polap-db\ui\ide-extensions\vscode-cursor"

# Use a local npm config if global auth config is noisy
$tempNpmRc = Join-Path (Get-Location) ".npmrc.temp"
"registry=https://registry.npmjs.org/`nalways-auth=false" | Set-Content -Path $tempNpmRc -Encoding UTF8
$env:npm_config_userconfig = $tempNpmRc

npm install
npm run build
npm run package
```

Expected artifact:
- `voltnuerongrid-vscode-cursor-0.1.0.vsix`

#### 3.5.2 Install VSIX into VS Code / Cursor

```powershell
code --install-extension .\voltnuerongrid-vscode-cursor-0.1.0.vsix --force
```

For Cursor, install the same VSIX using Cursor extension install flow.

#### 3.5.3 Extension runtime smoke script

```powershell
pwsh .\smoke-test.ps1 -BaseUrl "http://127.0.0.1:8080" -AdminKey "secret"
```

Expected: all checks pass for /health, /api/v1/sql/execute, /api/v1/ingest/schema/registry.

#### 3.5.4 Manual command validation in IDE

Open command palette and run:
- VoltNueronGrid: Connection Wizard
- VoltNueronGrid: Test Connection
- VoltNueronGrid: Run Query
- VoltNueronGrid: Analyze Query
- VoltNueronGrid: Show Schema Registry

Validate each mode in wizard:
- Admin mode: asks for admin key only
- Operator mode: asks for admin key + operator id
- Tenant mode: asks for tenant id + user id

Validate runtime target presets:
- Local
- Docker
- Cloud
- Custom

### 3.6 Docker/local/cloud endpoint validation matrix

Use the same extension binary with different base URLs:

1. Local
- Base URL: `http://127.0.0.1:8080`

2. Docker
- Base URL: `http://host.docker.internal:8080` (or mapped localhost port)

3. Cloud
- Base URL: `https://<your-cloud-endpoint>`
- Ensure TLS cert trust and network/firewall access from local machine.

4. Custom
- Any valid HTTP(S) runtime URL with required auth headers by mode.

### 3.7 Optional gate validation on local runtime

Run from clean shell:

```powershell
pwsh -NoProfile -File ./tests/kpi/scripts/run-ws1-gate.ps1 -BaseUrl http://127.0.0.1:8080
pwsh -NoProfile -File ./tests/kpi/scripts/run-ws3-gate.ps1 -BaseUrl http://127.0.0.1:8080
pwsh -NoProfile -File ./tests/kpi/scripts/run-ws5-gate.ps1 -BaseUrl http://127.0.0.1:8080 -IncludeRuntimeSmokes
```

Review JSON artifacts under `tests/kpi/results/**` and use artifact `status` fields as source of truth.

---

## 4) Final IDE-005 Closeout Checklist

- [x] VSIX built and verified locally
- [x] Local install smoke path documented
- [ ] Private feed endpoint details provided by user
- [ ] PAT source provided by user
- [ ] Publish executed (CLI or Web path)
- [ ] Post-publish install verification from feed

---

## 5) Recommended Next Action

Provide feed endpoint + PAT source and choose publish route:
- CLI publish (automated)
- Web upload publish (manual but fastest when CLI tooling is unstable)
