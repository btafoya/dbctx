# ARCHITECTURE.md

# dbctx Architecture

**Status:** Stable (Phase 13 complete; Phase 1, v1.0 and v0.3 features implemented)

## Purpose

This document defines the architectural boundaries, module
responsibilities, dependency rules, and data flow for `dbctx`.

The architecture is designed around a single principle:

> **Facts → Analysis → AI**

Each layer depends only on lower layers. Higher layers may enrich data
but must never modify factual metadata.

The `execute-statement` command (v1.0) is a narrow exception: it issues
a read-only SQL query through the connection layer and returns raw
results directly. It never writes to the database and never feeds its
output into the canonical schema model.

------------------------------------------------------------------------

# High-Level Architecture

``` text
                  +----------------------+
                  |      CLI (clap)      |
                  +----------+-----------+
                             |
                             v
                  +----------------------+
                  |    dbctx Library     |
                  +----------+-----------+
                             |
    +------------------------+------------------------+
    |                        |                        |
    v                        v                        v
Configuration         Connection Discovery     Export Pipeline
    |                        |                        |
    +-------------+----------+------------------------+
                  |
                  v
          Database Introspection
                  |
                  v
           Canonical Schema Model
                  |
      +-----------+-----------+
      |                       |
      v                       v
 Validation             Analysis (optional)
                              |
                              v
                         AI Context (optional)
                              |
                              v
                      Exporters (JSON/MD/Mermaid)
```

------------------------------------------------------------------------

# Architectural Layers

## Layer 1 -- Configuration

Responsibilities:

-   CLI parsing
-   `.env`
-   Environment variables
-   Config file (`.dbctx.toml`)

Produces immutable runtime configuration.

------------------------------------------------------------------------

## Layer 2 -- Connection Discovery

Determines how to connect.

Priority:

1.  CLI
2.  Docker Compose
3.  `.dbctx.toml`
4.  `.env`
5.  Environment
6.  Interactive
7.  Fail

No schema logic exists here.

------------------------------------------------------------------------

## Layer 2.5 -- Read-only Execution (v1.0)

A thin, isolated path used only by `execute-statement`. Implemented in
`src/execution.rs`.

-   Accepts a single SQL statement from the CLI.
-   Validates that the statement is read-only before it reaches the
    database.
-   Executes through the same connection layer as introspection.
-   Returns tabular results directly to the user or to an exporter.
-   **Must never mutate the canonical schema model.**

This layer is deliberately separate from introspection so that ad-hoc
queries cannot accidentally become part of the factual record.

------------------------------------------------------------------------

## Layer 3 -- Introspection

Reads catalog metadata. INFORMATION_SCHEMA is the primary source on every
engine that has one; native catalog views (`sys.*` on SQL Server,
`pg_catalog` on PostgreSQL, `PRAGMA`/`sqlite_master` on SQLite) supply
facts INFORMATION_SCHEMA does not expose or does not have. See SPEC.md §7.

MySQL, MariaDB and SQL Server connect through `mysql_async` and `tiberius`
respectively. PostgreSQL and SQLite connect through `sqlx`.

Responsibilities:

-   tables
-   columns
-   indexes
-   foreign keys
-   views

Never performs analysis.

------------------------------------------------------------------------

## Layer 4 -- Canonical Schema Model

This is the heart of the project.

Every exporter consumes this model.

Every analysis consumes this model.

No exporter reads database catalogs directly.

------------------------------------------------------------------------

## Layer 5 -- Validation

Pure rule engine.

Examples:

-   missing PK
-   duplicate indexes
-   circular FK
-   invalid metadata

Produces findings.

Never changes the schema.

------------------------------------------------------------------------

## Layer 6 -- Analysis

Optional deterministic heuristics.

Examples:

-   junction tables
-   lookup tables
-   audit tables
-   soft deletes
-   timestamp conventions

Produces additional metadata.

------------------------------------------------------------------------

## Layer 7 -- AI

Optional.

Consumes:

-   schema
-   validation
-   analysis

Produces:

-   summaries
-   narratives
-   entry-point suggestions

Cannot alter facts.

------------------------------------------------------------------------

## Layer 8 -- Exporters

Phase 1:

-   JSON
-   Markdown
-   Mermaid
-   Metadata

All exporters consume only the canonical model. Implemented in
`src/export.rs`.

------------------------------------------------------------------------

# Module Layout

``` text
src/
├── main.rs
├── lib.rs
├── cli.rs
├── config.rs
├── discovery.rs
├── database/
│   ├── mod.rs
│   ├── mysql.rs
│   ├── sqlserver.rs
│   ├── postgres.rs
│   └── sqlite.rs
├── model.rs
├── validation.rs
├── analysis.rs
├── ai.rs
├── stats.rs
├── diff.rs
├── export.rs
├── execution.rs
├── mcp.rs
├── mcp_server.rs
└── error.rs
```

`mcp.rs` is the `dbctx mcp` CLI entry point: it resolves the connection the
same way every other command does and builds its own tokio runtime.
`mcp_server.rs` is the `rmcp` `ServerHandler` implementation: it holds the
cached canonical model behind an `Arc<RwLock<Database>>`, serves resources
and prompts from it, and delegates `execute-statement` tool calls straight
to `src/execution.rs`, bypassing the cache, since a cached result set would
be a contradiction in terms. It reuses `src/export.rs`'s serialization
functions to render resource content instead of duplicating them.

------------------------------------------------------------------------

# Public API

The public library should remain intentionally small.

Example:

``` rust
let config = Config::discover()?;
let schema = dbctx::inspect(config)?;
dbctx::export(schema)?;
```

Implementation details remain internal.

------------------------------------------------------------------------

# Dependency Rules

Allowed:

    CLI
     ↓
    Config
     ↓
    Discovery
     ↓
    Database
     ↓
    Model
     ↓
    Validation
     ↓
    Analysis
     ↓
    AI
     ↓
    Exporters

The MCP server (`mcp.rs`, `mcp_server.rs`) sits alongside the CLI as
another entry point rather than inside this chain: it depends on Config,
Discovery, Database, Model, Exporters and the read-only Execution layer,
and nothing depends on it. It never writes to the canonical model; the
`refresh-schema` tool replaces the cached `Database` wholesale by
re-running introspection, the same as an ordinary `dbctx inspect`.

Forbidden:

-   Exporters querying databases
-   AI querying databases
-   Validation modifying schema
-   Analysis modifying facts
-   Circular module dependencies

------------------------------------------------------------------------

# Canonical Schema Model

``` text
Database
├── Metadata
├── Tables (schema-qualified)
│   ├── Columns
│   ├── Indexes
│   ├── Foreign Keys
│   └── Constraints
├── Views
└── Relationships
```

Every output format derives from this model.

`Database`, `DatabaseMetadata`, `Table`, `Column`, `Index`, `ForeignKey`
and `View` each carry an `attributes: BTreeMap<String, serde_json::Value>`
field for engine-specific facts that do not fit the fixed fields above, so
one engine's peculiarity never forces a field onto every other engine. It
is skipped from serialized output when empty and defaults to empty when
absent from a document being read, so it never appears for MySQL, MariaDB
or SQL Server and never changes their existing snapshots.

------------------------------------------------------------------------

# Error Architecture

Library:

-   typed errors

CLI:

-   formatted diagnostics
-   exit codes

No panics for recoverable failures.

------------------------------------------------------------------------

# Concurrency

Phase 1 should support parallel metadata collection where beneficial
while preserving deterministic ordering in the final model.

Ordering must be stable regardless of execution order.

------------------------------------------------------------------------

# Testing Strategy

Each architectural layer has independent tests.

-   Configuration tests
-   Discovery tests
-   Introspection tests
-   Model tests
-   Validation tests
-   Analysis tests
-   Export snapshot tests

End-to-end Docker integration tests validate supported database
versions.

------------------------------------------------------------------------

# Extension Strategy

Future databases implement the same introspection interface and populate
the canonical model.

Future exporters consume the canonical model without requiring database
changes.

------------------------------------------------------------------------

# Architecture Decision Records

Significant architectural changes require an ADR or RFC before
implementation.

Examples:

-   new database backend
-   new document format
-   plugin system
-   breaking schema model changes

------------------------------------------------------------------------

# Invariants

These rules must never be violated:

1.  Facts are immutable.
2.  Analysis is deterministic.
3.  AI is optional.
4.  Exporters are read-only.
5.  Canonical model is the single source of truth.
6.  Output formats are versioned.
7.  Stable ordering is preserved.

These invariants are the foundation of dbctx's long-term
maintainability.
