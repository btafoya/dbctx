# Changelog

All notable changes will be documented here.

The project follows Semantic Versioning.

## \[Unreleased - v1.0.0\]

### Added

-   `dbctx llm-txt` command that emits the project's LLM self-documentation
    guide
-   `dbctx execute-statement` command for safe, read-only SQL execution
    with mutating statements rejected before reaching the database

## \[Unreleased\]

### Added

-   Connection discovery in `dbctx::discovery`: `--compose-service` and
    `--docker-container` read `docker compose ps` and `docker inspect`,
    taking the engine from the image, the port from what the daemon published
    and the credentials from the container environment. A compose service
    resolves through the container running it, so the port reported is the
    one actually listening and a service that is down says so
-   `.dbctx.toml` is read as a connection source; every key is a long command
    line option under a `[dbctx]` table, unknown keys are refused, and a
    `password` key is refused by name
-   Interactive prompt as the last connection source, offered only when a
    terminal is attached and only for settings nothing else supplied
-   Command line interface in `dbctx::cli`: the `inspect`, `validate`,
    `graph`, `diff`, `stats` and `init` commands with the global, connection
    and output options `CLI.md` documents
-   Configuration layer in `dbctx::config`: `ConnectionSource` layers merged
    by `ConnectionConfig::resolve` in the precedence `SPEC.md` §6 fixes, with
    `.env` outranking the process environment
-   `Driver` for `--driver` and `DB_CONNECTION`, supplying the default port
    per engine
-   `ConnectionConfig` is read-only once resolved and redacts the password
    from its `Debug` output
-   `dbctx init` writes `.dbctx.toml` and refuses to replace an existing file
    without `--force`
-   Exit codes: 0 success, 3 invalid configuration, 64 invalid usage
-   `dbctx::Error` in `src/error.rs` unifying the failures each layer raises
-   Structured logging through `tracing`: `-v` to `-vvv` set the level,
    `--quiet` reports errors only, `--log-format json` emits one JSON object
    per record, `--color` controls ANSI. Diagnostics go to stderr, leaving
    stdout for command output; the password is never logged
-   Canonical schema model in `dbctx::model`: `Database`, `DatabaseMetadata`,
    `Table`, `View`, `Column`, `Index`, `ForeignKey`, `Relationship`, `Engine`
-   `Database::sort` applies the deterministic ordering `FORMAT.md` requires
-   `Database::relationships` derives relationships from foreign keys, so the
    two cannot drift apart; the derived list is written as the document's
    `relationships` array and a `relationships` array being read is ignored
-   `DocumentHeader` and `Generator` carry the `format`, `format_version`,
    `generator` and `generated_at` fields every document begins with;
    `Database` serializes as a complete `dbctx.schema` document
-   Cargo project: `dbctx` library plus CLI binary, edition 2024, MSRV 1.85
-   Lint configuration: `unsafe_code` forbidden, `clippy::all` denied,
    rustfmt and cargo-nextest configured
-   GitHub Actions CI: fmt, clippy, docs, nextest and MSRV check
-   Dual MIT and Apache-2.0 licenses
-   SQL Server as a Phase 1 supported database
-   `--driver` option and `DB_CONNECTION` environment variable
-   `schema` on tables, `referenced_schema` on foreign keys, and
    `from_schema`/`to_schema` on relationships

### Changed

-   The driver is required once configuration resolves. It is detected from
    the image of a discovered container; when nothing discovered one and no
    source named one, dbctx reports it rather than guessing an engine
-   `ConnectionConfig::driver` and `::port` are no longer optional, and
    `::host` defaults to `127.0.0.1` rather than being optional
-   `.dbctx.toml` is a connection configuration source, ranked between Docker
    Compose autodiscovery and `.env`; the sources below it renumber
-   Introspection is specified as catalog metadata rather than
    INFORMATION_SCHEMA only; `sys.*` supplies indexes, foreign key
    targets, identity columns and descriptions on SQL Server
-   Tables and views sort by schema, then name
-   `tables/` files are schema-qualified on SQL Server
-   Engine, charset and collation are null on SQL Server

### Fixed

### Removed
