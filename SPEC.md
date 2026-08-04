# SPEC.md

# dbctx Technical Specification

**Version:** 0.1 (Draft)

## 1. Purpose

This document is the authoritative technical specification for Phase 1
of **dbctx**. It defines the expected behavior of the software. Any
implementation must conform to this specification.

Project philosophy is defined in **VISION.md**. Future enhancements must
be proposed through the **RFC/** process before being incorporated into
this specification.

------------------------------------------------------------------------

# 2. Scope

Phase 1 provides:

-   MySQL, MariaDB and SQL Server schema introspection
-   Rust library and CLI
-   Deterministic schema model
-   JSON export
-   Markdown export
-   Mermaid ER graph export
-   Metadata export
-   Relationship export
-   Schema validation
-   Schema statistics
-   Schema diff
-   Optional analysis mode
-   Optional LLM context generation

Out of scope:

-   Schema migrations
-   SQL execution
-   ORM generation
-   Database modification
-   Template engines
-   Plugin systems

------------------------------------------------------------------------

# 3. Design Principles

1.  Facts first.
2.  Deterministic output.
3.  Stable format versions.
4.  Human-readable artifacts.
5.  AI-provider neutrality.
6.  Library-first architecture.
7.  Zero hidden network dependencies.

------------------------------------------------------------------------

# 4. Architecture

    CLI
     │
     ▼
    Core Library
     │
     ├── Configuration
     ├── Connection Discovery
     ├── Introspection
     ├── Schema Model
     ├── Validation
     ├── Analysis (optional)
     └── Exporters

Layering:

    Database
       │
    Facts
       │
    Analysis
       │
    AI

Higher layers may consume lower layers only.

------------------------------------------------------------------------

# 5. CLI

## Commands

``` text
dbctx inspect
dbctx validate
dbctx graph
dbctx diff
dbctx stats
dbctx init
```

## Common Options

``` text
--env
--host
--port
--user
--password
--database
--driver
--compose-service
--docker-container
--output
--format
--analyze
--llm
--verbose
```

------------------------------------------------------------------------

# 6. Connection Discovery

Priority:

1.  Explicit CLI options
2.  Docker Compose autodiscovery
3.  `.dbctx.toml`
4.  .env
5.  Environment variables
6.  Interactive prompt
7.  Error

`.dbctx.toml` is the project configuration file written by `dbctx init`. It
is read from the working directory when present and is never required. It
ranks below autodiscovery, so a discovered container still wins, and above
`.env`, so a setting committed to the project outranks a developer's local
environment.

Supported `.env` variables:

-   DB_CONNECTION
-   DB_HOST
-   DB_PORT
-   DB_DATABASE
-   DB_USERNAME
-   DB_PASSWORD

`DB_CONNECTION` selects the engine (`mysql`, `mariadb`, `sqlsrv`) and is
the environment equivalent of `--driver`. When absent, the engine is
detected from the connection.

Default port when unspecified:

-   3306 for MySQL and MariaDB
-   1433 for SQL Server

------------------------------------------------------------------------

# 7. Supported Databases

Phase 1:

-   MySQL
-   MariaDB
-   SQL Server

Introspection reads catalog metadata only. SQL is never parsed and access
is read-only.

INFORMATION_SCHEMA is the primary source for tables, columns, views and
constraints on every engine. Where INFORMATION_SCHEMA does not expose a
required fact, the engine's native catalog is used:

| Fact | MySQL / MariaDB | SQL Server |
|---|---|---|
| Indexes | `INFORMATION_SCHEMA.STATISTICS` | `sys.indexes`, `sys.index_columns` |
| Foreign key targets | `KEY_COLUMN_USAGE.REFERENCED_*` | `sys.foreign_keys`, `sys.foreign_key_columns` |
| Auto increment | `COLUMNS.EXTRA` | `sys.identity_columns` |
| Comments | `COLUMNS.COLUMN_COMMENT` | `sys.extended_properties` (`MS_Description`) |

On SQL Server every schema in the target database is introspected. System
catalog objects are excluded because they are absent from
INFORMATION_SCHEMA.

------------------------------------------------------------------------

# 8. Internal Schema Model

Database

-   Metadata
-   Tables
-   Views
-   Relationships

Table

-   Schema
-   Name
-   Engine (MySQL and MariaDB only)
-   Charset (MySQL and MariaDB only)
-   Collation (MySQL and MariaDB only)
-   Columns
-   Indexes
-   Foreign Keys

Column

-   Name
-   Type
-   Nullable
-   Default
-   Auto Increment
-   Comment

`Schema` is the object namespace: the database name on MySQL and MariaDB,
the SQL Server schema (for example `dbo`) on SQL Server.

`Auto Increment` covers MySQL and MariaDB `AUTO_INCREMENT` and SQL Server
`IDENTITY`.

Engine-specific fields are null when the source engine does not provide
them. They are never omitted and never given placeholder values.

------------------------------------------------------------------------

# 9. Exporters

## JSON

Canonical machine-readable format.

## Markdown

Human-readable documentation.

## Mermaid

Relationship graph.

## Metadata

Generator information.

------------------------------------------------------------------------

# 10. Output Layout

``` text
.ai/dbctx/
├── schema.json
├── schema.md
├── metadata.json
├── relationships.json
├── graph.mmd
└── tables/
```

Files in `tables/` are named `<table>.json` on MySQL and MariaDB, where
the schema is the database itself, and `<schema>.<table>.json` on SQL
Server, where two schemas may hold tables of the same name.

------------------------------------------------------------------------

# 11. Format Versioning

Every exported document includes:

-   format
-   format_version
-   generator
-   generated_at

Application version and document format version are independent.

------------------------------------------------------------------------

# 12. Validation

Validation detects:

-   Missing primary keys
-   Broken foreign keys
-   Duplicate indexes
-   Circular references
-   Invalid metadata

Validation reports findings only.

No automatic fixes.

------------------------------------------------------------------------

# 13. Analysis Mode

Enabled with:

``` bash
dbctx inspect --analyze
```

Deterministic heuristics only.

Examples:

-   Junction tables
-   Lookup tables
-   Audit tables
-   Soft deletes
-   Timestamp conventions

No AI.

------------------------------------------------------------------------

# 14. LLM Mode

Enabled with:

``` bash
dbctx inspect --llm
```

Adds:

-   Context summaries
-   Relationship narratives
-   Entry-point suggestions

Every generated section must be labeled as AI-generated.

------------------------------------------------------------------------

# 15. Performance Goals

-   Startup \<250 ms (excluding DB connection)
-   500-table schema under 5 seconds on commodity hardware
-   Streaming exporters where practical
-   Memory usage proportional to schema size

------------------------------------------------------------------------

# 16. Error Handling

Library: - Typed errors

CLI: - Human-readable diagnostics - Exit codes - Verbose mode

------------------------------------------------------------------------

# 17. Logging

Structured logging using `tracing`.

Default output remains quiet.

------------------------------------------------------------------------

# 18. Testing

Required:

-   Unit tests
-   Integration tests
-   Snapshot tests
-   Golden output tests
-   Docker-based compatibility tests

------------------------------------------------------------------------

# 19. Compatibility

Phase 1 guarantees:

-   Stable CLI behavior within major versions
-   Stable document formats within format_version
-   Backward-compatible JSON readers

------------------------------------------------------------------------

# 20. Security

-   No credential persistence
-   Read-only database access
-   No outbound network calls
-   No telemetry
-   No analytics

------------------------------------------------------------------------

# 21. Licensing

MIT OR Apache-2.0.

------------------------------------------------------------------------

# 22. Future Work

Tracked through RFCs.

Examples:

-   PostgreSQL
-   SQLite
-   Template engine
-   Plugin architecture
-   IDE integrations
-   MCP resources

This document defines the implementation contract for Phase 1.
