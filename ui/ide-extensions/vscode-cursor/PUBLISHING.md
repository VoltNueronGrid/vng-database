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

## Install into a VS Code profile (e.g. **Rust**)

Use a named profile so Rust tooling and this extension stay isolated from other setups.

1. Create or select the profile: **File → Preferences → Profiles → Create Profile…** (name it `Rust` if you want parity with common setups).
2. From a shell, install the packaged extension into that profile (replace the `.vsix` name with the file you built):

```powershell
Set-Location "D:\by\polap-db\ui\ide-extensions\vscode-cursor"
npm run package
code --profile "Rust" --install-extension .\voltnuerongrid-vscode-cursor-0.3.1.vsix --force
```

3. Restart VS Code with that profile and open **VoltNueronGrid → Database**. With no saved connections you should see the empty-state welcome and **Create Connection** opening the **Connect to server** editor.

To remove an older side-loaded build first: **Extensions** → find **VoltNueronGrid** → **Uninstall**, then install the new `.vsix` as above.

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
