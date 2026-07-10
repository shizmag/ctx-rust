# ADR 0001: Language backend boundary for ctx-codegraph

## Status

Accepted

## Context

`ctx-codegraph` started as a Rust-only code graph indexer. Rust parser and `rust-analyzer` resolver logic leaked into generic indexing, storage, and service layers. Adding new language backends (e.g. Python, TypeScript) would require major changes to the generic core modules.

## Decision

Introduce a backend boundary with:
* `LanguageBackend`
* `ParserBackend`
* `ResolverBackend`
* `BackendRegistry`
* A generic LSP transport in `ctx-codegraph-resolver`.
* Language backends in dedicated crates (`ctx-lang-rust`, `ctx-lang-python`, …).
* Storage and query logic in `ctx-codegraph-store`.

Move all Rust-specific parser, resolver, and workspace configuration markers into `ctx-lang-rust`. Provide registry injection options (`build_index_with_registry`, `rebuild_index_db_with_registry`) to allow test-only mock registry execution.

## Consequences

### Positive
- New programming languages can be added cleanly as isolated backends without altering core indexing orchestration or the database schema.
- The storage layer preserves and matches dynamic language strings and backend metadata instead of hardcoded Rust-only values.
- Tests can validate the generic indexing pipeline and compatibility checks using a test-only mock backend.

### Tradeoffs
- Introduce slightly more traits and virtual dispatch abstractions.
- Database compatibility checks must handle compound backend versions and config fingerprints.
- The `use_lsp` resolver configuration switch is temporarily global/transitional until per-backend resolver configs are designed.

## Follow-ups

- affected-edge incremental rebuild architecture;
- generic Occurrence and GraphEdge model;
- per-backend resolver configuration;
- second real language backend.
