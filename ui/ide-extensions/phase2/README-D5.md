# IDE Extensions — Phase 2 (D-5)

JetBrains, Eclipse, and Antigravity adapters for VoltNueronGrid. VS Code/Cursor
and Visual Studio are complete (`../`); this phase completes the remaining three.

## Shared query-runner core

All three IDEs reuse a single, dependency-free Java core
(`../shared/vng-ide-core`) — `VngHttpClient` (URL/header auth + `/api/v1/sql/execute`)
and `VngQueryResult` (column/row parsing), with no third-party HTTP/JSON deps.

```bash
mvn -f ../shared/vng-ide-core test                 # 8 unit tests (offline)
VNG_IDE_LIVE=1 VNG_ADMIN_API_KEY=secret \
  mvn -f ../shared/vng-ide-core test               # + live roundtrip
mvn -f ../shared/vng-ide-core install              # publish for the gradle/PDE builds
```

## JetBrains

- `client/VngApiClient.kt` — delegates to the shared core (no OkHttp/Gson).
- `client/VngSecretStore.kt` — admin key in the IDE **PasswordSafe** (secure storage).
- `actions/VngActions.kt` — Execute SQL (editor selection → result table dialog),
  Browse Schema, New Connection.
- `settings/` — persisted connection profile + settings UI.

```bash
mvn -f ../shared/vng-ide-core install              # prerequisite
cd jetbrains && ./gradlew test                     # unit test over the shared core
./gradlew buildPlugin                              # build the installable plugin (.zip)
```

## Eclipse

- `client/VngSecretStore.java` — admin key in Eclipse **SecureStorage**.
- `actions/ExecuteSqlAction.java` — selected SQL → shared core → `QueryResultView`.
- `views/` — schema, results, and connection views; `connection/VngPreferencePage.java`.
- `plugin.xml` / `META-INF/MANIFEST.MF` — extension points + lifecycle.

The Eclipse plugin compiles against the Eclipse Target Platform (PDE) with the
shared `vng-ide-core` on its bundle classpath. Action/view code is validated at
IDE runtime under E-5; the shared query-runner logic is unit-tested above.

## Antigravity

- `src/vng-adapter.js` — dependency-free adapter: `VngAdapterClient` (query
  runner + `diagnostics()`), `authHeaders`, `parseExecuteResponse`.

```bash
cd antigravity && npm test                         # 7 unit tests (offline, HTTP stubbed)
VNG_IDE_LIVE=1 VNG_ADMIN_API_KEY=secret \
  node --test test/vng-adapter.test.js             # + live roundtrip
```

## "100%" scope

Server communication (the query runner) is real and unit-tested for every IDE
(shared core + Antigravity), and validated live against a running server. The
IDE-host glue (command palette/actions, secret-storage APIs, views) is real code
compiled by each IDE's own build (gradle/PDE); full IDE-runtime smoke is tracked
under E-5, consistent with the project's batch precedent.
