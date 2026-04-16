# Troubleshooting

## Connection Test Fails Immediately

Check the following first:

- The `baseUrl` points at a reachable VoltNueronGrid runtime
- The selected auth mode matches the headers required by that environment
- Admin mode includes a valid admin API key in SecretStorage
- Operator and tenant modes include the required identity fields

## Explorer Shows No Databases

Common causes:

- The connection profile is saved but not active
- The runtime returned auth failures while loading schema metadata
- The schema cache is stale and needs a refresh from the explorer title action

Try disconnecting and reconnecting the profile, then use the explorer refresh action.

## Query Results Show Errors

Look at both the notification and the results webview error box.

- Secret-like fields are intentionally redacted in surfaced errors
- Timeout and cancellation failures are reported separately from runtime SQL errors
- Multi-statement execution stops early when a statement fails in stop-on-error mode

## Table Editor Cannot Save

The table editor disables save-related actions when:

- The table is read-only
- Required key columns are missing for updates or deletes
- One or more cells still have validation errors

If a partial save occurs, use the pending SQL action to inspect unapplied statements.

## Packaging Or Smoke Test Issues

Local build and validation commands:

```bash
npm run build
npm test
npm run package
```

Local smoke test:

```powershell
pwsh ./smoke-test.ps1 -BaseUrl "http://127.0.0.1:8080" -AdminKey "secret"
```

## Manual UX Pass Checklist

When validating the extension in a live VS Code or Cursor window, walk this sequence:

1. Create a new connection and verify the dedicated editor opens beside the explorer.
2. Save and activate the connection from the explorer tree.
3. Confirm themed icons render for the connection, database, schema, table, and column nodes.
4. Use keyboard navigation across explorer actions and webview controls.
5. Run a `.sql` file and verify the results webview announces summary and error states correctly.
6. Open the table editor and confirm toolbar, notices, and pagination remain readable with keyboard focus.

This checklist is the intended live follow-up to the code-level accessibility work from Phase 7.