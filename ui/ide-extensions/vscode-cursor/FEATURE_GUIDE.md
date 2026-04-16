# Feature Guide

## Connection Management

Use `VoltNueronGrid: Create New Connection` to open the dedicated connection editor. The editor supports:

- Admin, operator, and tenant auth modes
- Local, Docker, cloud, and custom runtime targets
- SSL/TLS certificate metadata
- Advanced timeout and pooling settings

Saved profiles appear directly in the database explorer. From the explorer you can connect, disconnect, edit, test, delete, and refresh per profile.

## Database Explorer

The explorer is rooted on saved connections instead of a single global runtime target. Once a profile is active, the tree expands through:

- Databases
- Schemas
- Tables
- Columns

Phase 7 added themed icons for each of these levels and accessibility labels for tree items.

## SQL Workflow

Open any `.sql` file and use the editor title actions or keyboard shortcuts to execute or analyze SQL. Query execution supports:

- Selection-or-file execution
- Statement splitting for multi-statement SQL
- Timeout and cancellation handling
- Non-blocking notifications for progress and failures

## Query Results

Results open in a dedicated webview with:

- Row filtering
- Column sorting
- Pagination
- Export to CSV or JSON
- Accessible status, error, and navigation regions

## Query History

The query history sidebar stores the most recent executions per connection. Use it to:

- Re-run a prior query
- Search recent history
- Clear history for the active connection or globally

## Table Editor

Open the table editor from a table node in the explorer. The editor supports:

- Inline cell edits
- Draft row insertion
- Delete and undo-delete toggles
- Partial-save messaging with pending SQL recovery
- Keyboard shortcuts for save, refresh, and row insertion

## Schema Wizards

Schema management commands support create-table and alter-table flows with deterministic DDL generation. The wizards are intended for controlled schema operations rather than ad hoc SQL text editing.

## Settings

Use `VoltNueronGrid: Settings` or `Ctrl+,` to open the settings webview. The panel centralizes:

- SQL execution behavior
- Results display preferences
- Connection defaults
- Editor-oriented toggles