# SPEC.md

# dbctx Technical Specification

**Version:** 1.0

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

-   MySQL, MariaDB, SQL Server, PostgreSQL and SQLite schema introspection
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
-   An MCP server exposing the schema to MCP clients (`dbctx mcp`)

Out of scope:

-   Schema migrations
-   SQL execution (except the read-only `execute-statement` command
    introduced in v1.0.0)
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
dbctx llm-txt
dbctx execute-statement
dbctx mcp
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

`--database` may be repeated. Every driver except `sqlite` requires exactly
one value; for `sqlite` the first is the main database file and the rest
are attached databases, in order.

## MCP Server

`dbctx mcp` resolves the connection exactly like every other command, reads
the schema once, and serves it to MCP clients from an in-memory cache. It
never re-queries the database except when the `refresh-schema` tool is
called.

Transport: stdio by default; `--sse-port <PORT>` serves the MCP Streamable
HTTP transport instead, bound to `127.0.0.1:<PORT>`. `--introspection-timeout
<SECONDS>` (default 30) bounds the initial read and every `refresh-schema`
call.

Resources, read from the cache: `dbctx://schema`, `dbctx://metadata`,
`dbctx://graph`, `dbctx://relationships`, and one
`dbctx://tables/<schema>.<table>` per table.

Tools: `execute-statement` (`sql` required, `timeout` optional seconds),
reusing the same read-only whitelist as `dbctx execute-statement`; and
`refresh-schema`, which re-reads the database and replaces the cache.

Prompts, all zero-argument and deterministic: `summarize-schema`,
`describe-table`, `explain-relationships`.

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

`DB_CONNECTION` selects the engine (`mysql`, `mariadb`, `sqlsrv`, `postgres`,
`sqlite`) and is the environment equivalent of `--driver`. When absent, the
engine is detected from the connection.

Default port when unspecified:

-   3306 for MySQL and MariaDB
-   1433 for SQL Server
-   5432 for PostgreSQL
-   none for SQLite, which has no port

Default host when unspecified: `127.0.0.1`. The address rather than
`localhost`, which some MySQL clients resolve to a Unix socket and others to
a TCP port. A configured socket takes precedence over the host. Host and
port are ignored for SQLite, which connects to a file.

The engine is not defaulted. It is detected from the image of a discovered
container; where nothing was discovered and no source named one, resolution
fails rather than guessing.

`.dbctx.toml` accepts an additional `[dbctx.sqlite.attach]` table of
`name = "path"` entries, parsed into named SQLite attachments. This table
is validated but not yet consulted during connection resolution: it has no
effect on which databases are attached today. Attached databases are
resolved solely from repeated `--database` values or Docker Compose mount
discovery, in `main`, `attach1`, `attach2`, ... order.

------------------------------------------------------------------------

# 7. Supported Databases

Phase 1:

-   MySQL
-   MariaDB
-   SQL Server
-   PostgreSQL
-   SQLite

Introspection reads catalog metadata only. SQL is never parsed and access
is read-only. MySQL, MariaDB and SQL Server connect through `mysql_async`
and `tiberius` respectively; PostgreSQL and SQLite connect through `sqlx`.

INFORMATION_SCHEMA is the primary source for tables, columns, views and
constraints on every engine that has one. Where INFORMATION_SCHEMA does not
expose a required fact, the engine's native catalog is used:

| Fact | MySQL / MariaDB | SQL Server | PostgreSQL |
|---|---|---|---|
| Indexes | `INFORMATION_SCHEMA.STATISTICS` | `sys.indexes`, `sys.index_columns` | `pg_index`, `pg_class` |
| Foreign key targets | `KEY_COLUMN_USAGE.REFERENCED_*` | `sys.foreign_keys`, `sys.foreign_key_columns` | `pg_constraint` |
| Auto increment | `COLUMNS.EXTRA` | `sys.identity_columns` | `pg_attribute.attidentity`, `nextval(...)` defaults |
| Comments | `COLUMNS.COLUMN_COMMENT` | `sys.extended_properties` (`MS_Description`) | `col_description`, `obj_description` |

PostgreSQL columns, indexes and foreign keys are read from `pg_catalog`
(`pg_attribute`, `pg_index`, `pg_constraint`) rather than
INFORMATION_SCHEMA: `pg_catalog` gives an accurate bare/full type split via
`format_type` and ordered multi-column index and foreign key definitions in
a single query each, which INFORMATION_SCHEMA only offers by joining
several views together for the same result. Tables and views are still
enumerated from INFORMATION_SCHEMA.

SQLite has neither INFORMATION_SCHEMA nor a non-textual catalog. Every fact
comes from `sqlite_master` and the `PRAGMA` family
(`table_xinfo`, `index_list`, `index_info`, `foreign_key_list`).
`WITHOUT ROWID` and `STRICT` exist only as keywords in a table's declared
SQL text, so they are the one fact this engine reads from that text rather
than a structured catalog value.

On SQL Server every schema in the target database is introspected. System
catalog objects are excluded because they are absent from
INFORMATION_SCHEMA. PostgreSQL introspects every schema except
`pg_catalog`, `information_schema` and `pg_toast*`. SQLite introspects the
main database and every attached database named on the connection.

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
-   Attributes

Column

-   Name
-   Type
-   Nullable
-   Default
-   Auto Increment
-   Comment
-   Attributes

`Schema` is the object namespace: the database name on MySQL and MariaDB,
the SQL Server schema (for example `dbo`) on SQL Server, the PostgreSQL
schema (for example `public`) on PostgreSQL, and the SQLite database name
(`main`, `attach1`, ...) on SQLite.

`Auto Increment` covers MySQL and MariaDB `AUTO_INCREMENT`, SQL Server
`IDENTITY`, PostgreSQL identity columns and `nextval(...)` defaults, and a
SQLite `INTEGER PRIMARY KEY` declared `AUTOINCREMENT`.

Engine-specific fields are null when the source engine does not provide
them. They are never omitted and never given placeholder values.

`Attributes` is a map of engine-specific facts that do not fit the fields
above, present on `Database`, `DatabaseMetadata`, `Table`, `Column`,
`Index`, `ForeignKey` and `View`. It is empty and omitted from the
document for MySQL, MariaDB and SQL Server. For PostgreSQL it may hold
`access_method`, `tablespace` and `row_security` on tables, and
`identity_generation` and `collation` on columns. For SQLite it may hold
`without_rowid` and `strict` on tables, `hidden` on columns, and `origin`
on indexes. See FORMAT.md for the full list and when each key is present.

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
Server, PostgreSQL and SQLite, where more than one schema may hold tables
of the same name.

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
-   SQLite `WITHOUT ROWID` tables with no primary key
-   SQLite `STRICT` table columns that are `NOT NULL` with no default

Missing-primary-key detection already applies to every engine, including
PostgreSQL, so there is no separate PostgreSQL-specific rule.

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
-   `execute-statement` rejects mutating SQL before execution and never
    modifies the canonical schema model
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

-   Template engine
-   Plugin architecture
-   IDE integrations
-   Migrating MySQL, MariaDB and SQL Server to `sqlx`

This document defines the implementation contract for Phase 1.
