# VoltNueronGrid VS Code/Cursor Extension

Professional database-client tooling for VoltNueronGrid DB inside VS Code and Cursor.

## Current Scope

The extension now covers the full Phase 1 through Phase 7 roadmap slice for the VS Code/Cursor target:

- Connection profile management with secure secret storage
- Connection-centric explorer with saved profiles, databases, schemas, tables, and columns
- SQL execution from `.sql` files with query analysis hooks
- Results webview with filtering, sorting, pagination, and export
- Query history sidebar with re-run and search support
- Inline table editor with staged row edits and partial-save recovery
- Schema create/alter wizards with generated DDL preview
- Settings panel for editor, results, and connection behavior
- Themed explorer icons plus accessibility improvements across primary webviews

## Install And Build

```bash
npm install
npm run build
```

Run the local test suite:

```bash
npm test
```

Package the extension:

```bash
npm run package
```

## Quick Start

1. Start a local VoltNueronGrid runtime, or point the extension at an existing deployment.
2. Open the command palette and run `VoltNueronGrid: Create New Connection`.
3. Save a connection profile with the appropriate auth mode:
   - Admin
   - Operator
   - Tenant
4. Activate or connect the profile from the explorer tree.
5. Open a `.sql` file and use `Ctrl+Enter` to execute the current selection or file.
6. Review results in the query-results webview, or inspect schema objects from the explorer.

## Core Commands

- `VoltNueronGrid: Create New Connection`
- `VoltNueronGrid: Edit Connection`
- `VoltNueronGrid: Manage Connections`
- `VoltNueronGrid: Quick Switch Connection`
- `VoltNueronGrid: Execute SQL (Selection/File)`
- `VoltNueronGrid: Analyze SQL (Selection/File)`
- `VoltNueronGrid: Open Table Editor (Pick Table)`
- `VoltNueronGrid: Create Table Wizard`
- `VoltNueronGrid: Alter Table Wizard`
- `VoltNueronGrid: Settings`

## Key Shortcuts

- `Ctrl+Enter`: execute SQL selection or file
- `Ctrl+Shift+Enter`: analyze SQL selection or file
- `Ctrl+Shift+D`: quick switch active connection
- `Ctrl+Shift+F`: open table editor picker from the database explorer
- `Ctrl+Alt+C`: open create-table wizard from the database explorer
- `Ctrl+S`: save changes in the table editor webview
- `Ctrl+Shift+N`: add a row in the table editor webview
- `Ctrl+R`: refresh table editor data
- `Ctrl+,`: open VoltNueronGrid settings

## Supporting Docs

- [FEATURE_GUIDE.md](FEATURE_GUIDE.md): end-user workflows and UI behavior
- [ARCHITECTURE.md](ARCHITECTURE.md): module layout and runtime flow
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md): common failures, diagnostics, and local checks
- [PUBLISHING.md](PUBLISHING.md): packaging and feed publication notes
- [REFACTORING.md](REFACTORING.md): earlier architecture-refactor background

## Local Smoke Test

```powershell
pwsh ./smoke-test.ps1 -BaseUrl "http://127.0.0.1:8080" -AdminKey "secret"
```

## Notes

- Sensitive credentials remain in VS Code SecretStorage instead of workspace settings.
- The extension currently targets VS Code and Cursor only.
- Multi-IDE adapters and marketplace publishing remain later roadmap items.
