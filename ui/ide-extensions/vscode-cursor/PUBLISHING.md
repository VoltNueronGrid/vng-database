# VoltNueronGrid VSCode/Cursor Publishing

## Prerequisites

- Node.js and npm installed
- VSCE installed: npm install -g @vscode/vsce
- Publisher identity created for the private feed
- Valid publisher access token

## Credential scopes

- Runtime credentials are for database API access (admin/operator/tenant headers).
- Publishing credentials are for package distribution (npm/feed/publisher tokens).
- Runtime credentials do not grant package publish rights, and publish tokens do not grant runtime data access.

## Package

```powershell
npm install
npm run build
vsce package
```

Expected output: a .vsix package in this folder.

Current artifact produced:

- `voltnuerongrid-vscode-cursor-0.1.0.vsix`

## Local install smoke

```powershell
code --install-extension .\voltnuerongrid-vscode-cursor-0.1.0.vsix --force
pwsh .\smoke-test.ps1 -BaseUrl "http://127.0.0.1:8080" -AdminKey "secret"
```

## Publish to private feed

Use your private feed process with the produced .vsix package.

This is the only remaining external step for IDE-005 because feed credentials and target registry are environment-specific.

For Azure DevOps Artifacts path, required runtime parameters are:

- Organization URL
- Project name
- Feed name
- PAT token (or authenticated non-interactive CLI session)

## Troubleshooting

- npm install returns 401:
	- Your current npm registry auth is invalid or missing.
	- Re-authenticate against your configured registry and retry.
- tsc or vsce not found:
	- Run npm install successfully first so local binaries are available.
- `az extension add --name azure-devops --yes` fails:
	- Azure DevOps CLI extension is not installed correctly.
	- Retry with `az extension add --name azure-devops --yes --debug` and fix pip/extension errors first.
