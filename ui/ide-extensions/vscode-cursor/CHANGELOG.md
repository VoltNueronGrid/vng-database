# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] - 2026-04-16

### Added
- Query workflow tests for connection -> execution -> results -> history behavior.
- SQL intelligence helper extraction with dedicated tests for autocomplete/search logic.
- Configurable query result cache settings:
  - `voltnuerongrid.query.cache.enabled`
  - `voltnuerongrid.query.cache.ttlSeconds`
  - `voltnuerongrid.query.cache.maxEntries`

### Changed
- Query execution now supports in-memory caching of successful query results for repeated requests.
- SQL diagnostics startup path now avoids eager full-document scanning on activation.
- SQL table-reference shaping now reuses per-connection cached refs when schema timestamp is unchanged.
- Extension version bumped to `0.3.0`.

### Fixed
- Query execution/history ID collisions under rapid consecutive executions.
- `executeMultiple` now assigns stable per-statement execution IDs when group execution IDs are used.

### Quality
- Test suite expanded and passing with explicit coverage validation from Node test coverage output.
