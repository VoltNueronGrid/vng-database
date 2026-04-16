# Architecture

## Module Layout

The extension source lives under `src/` and is split by responsibility:

- `commands/`: command registration helpers and SQL/schema actions
- `models/`: shared connection, schema, query-result, and table-editor types
- `providers/`: explorer and query-history tree providers plus pure tree-presentation helpers
- `services/`: connection persistence, HTTP execution, schema caching, query execution, and table-editor SQL helpers
- `sql/`: SQL language features such as completion and diagnostics
- `ui/`: webview renderers for connections, settings, results, schema wizards, and table editing
- `test/`: node-based unit and workflow-style tests compiled into `out/test`

## Runtime Flow

1. `src/extension.ts` activates the extension and wires commands, providers, status bar items, and webview panels.
2. `ConnectionManager` persists non-secret profile metadata in workspace-global state and keeps secret values in VS Code SecretStorage.
3. `HttpClient`, `QueryExecutionService`, and `SchemaManager` translate explorer and SQL actions into runtime API calls.
4. Tree providers surface connection and schema state back into the VS Code explorer.
5. Webview panels provide richer interaction surfaces for connection editing, results review, settings, and inline table manipulation.

## Testing Strategy

The extension currently uses lightweight Node.js tests compiled from `src/test/*.test.ts`.

- Pure helpers and model logic are preferred test seams.
- Workflow-style tests exist where the logic can be exercised without a live VS Code window.
- Files that import the `vscode` runtime directly should usually be covered by extracting pure helpers instead of forcing brittle module shims.

This keeps the suite fast and compatible with the existing `npm test` command while the Phase 8 test surface expands.

## Accessibility And UX

Phase 7 introduced:

- Theme-aware explorer icons
- ARIA labels and live regions across the key webviews
- Accessible empty states, toolbars, tables, and pager controls
- Clear connection lifecycle notifications with secret-safe error handling

## Packaging

The extension is built with TypeScript and packaged with `vsce`. Generated artifacts stay at the extension root as `.vsix` files during local release validation.