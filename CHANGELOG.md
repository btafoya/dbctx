# Changelog

All notable changes will be documented here.

The project follows Semantic Versioning.

## \[Unreleased\]

### Added

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

-   Introspection is specified as catalog metadata rather than
    INFORMATION_SCHEMA only; `sys.*` supplies indexes, foreign key
    targets, identity columns and descriptions on SQL Server
-   Tables and views sort by schema, then name
-   `tables/` files are schema-qualified on SQL Server
-   Engine, charset and collation are null on SQL Server

### Fixed

### Removed
