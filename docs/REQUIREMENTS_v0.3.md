# Requirements: dbctx v0.3

**Scope:** PostgreSQL introspection, SQLite introspection, and an MCP server, shipped together as v0.3.0.

**Status:** Requirements discovery complete. Open questions resolved below. Awaiting `/sc:design` for architecture and implementation planning.

------------------------------------------------------------------------

## 1. Goals

- Add PostgreSQL 14, 15, 16, and 17 to the set of supported engines.
- Add file-based SQLite support, including multiple attached databases.
- Add an MCP server subcommand that exposes dbctx schema context to MCP clients over stdio (default) and optional SSE.
- Keep the canonical schema model as the single source of truth.
- Preserve deterministic, versioned output formats.
- Maintain the existing dependency and layering rules from `ARCHITECTURE.md`.

------------------------------------------------------------------------

## 2. Non-Goals

- Cloud-managed PostgreSQL variants (RDS, AlloyDB, etc.) are not explicitly targeted; only self-hosted local/Docker Postgres.
- SQLite `:memory:` databases are not a primary target in v0.3.
- No PostgreSQL-specific auth beyond TCP username/password.
- No new public library API surface beyond what the new commands require.

------------------------------------------------------------------------

## 3. Functional Requirements

### 3.1 PostgreSQL Support

- Driver name: `postgres`.
- Default host: `127.0.0.1`; default port: `5432`.
- Authentication: username/password over TCP only.
- Introspection source: `information_schema` as primary; native `pg_catalog` views for:
  - access_method
  - tablespace
  - identity columns
  - generated columns
  - comments (`obj_description`, `col_description`)
- Populated model objects: tables, columns, indexes, foreign keys, views, relationships.
- Schema mapping: the canonical `schema` field is the PostgreSQL schema name (e.g., `public`).
- Docker Compose discovery recognizes `postgres:*` images.
- Integration tests run against PostgreSQL 14, 15, 16, and 17 in the Docker matrix.

### 3.2 SQLite Support

- Driver name: `sqlite`.
- Connection: `--driver sqlite` with `--database /path/to/file.db`.
- Multiple `--database` values are allowed; the first is `main`, subsequent databases are attached as `attach1`, `attach2`, etc.
- Host/port are ignored for SQLite.
- Introspection source: `sqlite_master` / `sqlite_schema` per attached database.
- Populated model objects: tables, columns, indexes, foreign keys, views, relationships.
- Schema mapping: the canonical `schema` field is the SQLite database name (`main`, `attach1`, ...).
- Table files in `.ai/dbctx/tables/` are always schema-qualified as `<schema>.<table>.json`.
- Docker Compose discovery is best-effort for SQLite, using image-name and command-line heuristics.
- Integration tests run locally against temp files and attached databases; no Docker container required.

### 3.3 Canonical Model Extension

- Add an `attributes` map (`BTreeMap<String, serde_json::Value>`) to every modeled object:
  - `Database`
  - `Table`
  - `Column`
  - `Index`
  - `ForeignKey`
  - `View`
- `attributes` holds engine-specific facts that do not fit existing fields.
- Examples:
  - PostgreSQL: `access_method`, `tablespace`.
  - SQLite: `without_rowid`, `strict`.
- Existing fields remain unchanged; engine-specific facts are never invented.
- Format version stays `1.0`; the addition is backward compatible per `FORMAT.md` readers-must-ignore-unknown-fields rule.

### 3.4 Command Surface

All existing commands must work for PostgreSQL and SQLite unless noted:

- `dbctx inspect`
- `dbctx validate` — plus new engine-specific rules
- `dbctx graph`
- `dbctx diff`
- `dbctx stats`
- `dbctx init` — interactive prompts for PostgreSQL and SQLite when no Compose service is found
- `dbctx execute-statement` — supports both engines with existing whitelist and 30-second timeout
- `dbctx llm-txt` — unchanged
- `dbctx mcp` — new subcommand

### 3.5 MCP Server

- Delivered as a new `dbctx mcp` subcommand.
- Uses the `rmcp` Rust MCP SDK, latest `0.1.x` version.
- Default transport: stdio.
- Optional transport: HTTP/SSE via a flag.
- Resolves the target database using the same precedence as other CLI commands (Compose, `.dbctx.toml`, `.env`, environment, CLI args).
- Opens a connection pool at startup and reuses connections across requests.
- Loads the canonical schema model into memory once at startup and serves all resources from that cached model.
- Regenerates the cached model only when an explicit refresh is requested (e.g., a `refresh` tool or resource mutation notification).
- Exposes resources with clean-path URIs such as:
  - `dbctx://schema`
  - `dbctx://metadata`
  - `dbctx://tables/<schema>.<table>`
  - `dbctx://graph`
  - `dbctx://relationships`
- Exposes tools:
  - `execute-statement` — reuses existing read-only SQL whitelist and timeout.
- Exposes prompts, all operating on the whole schema with no arguments:
  - `summarize-schema`
  - `describe-table`
  - `explain-relationships`

### 3.6 Validation Engine-Specific Rules

In addition to existing validation rules:

- PostgreSQL: report tables without a primary key.
- SQLite: report `WITHOUT ROWID` tables without a primary key.
- SQLite: report `STRICT` tables where a `NOT NULL` column has no explicit `DEFAULT`.

------------------------------------------------------------------------

## 4. Non-Functional Requirements

- All output remains deterministic and ordered.
- All commits must pass `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and `cargo test`.
- Integration tests must pass for PostgreSQL 14–17 and SQLite.
- Snapshot tests must be updated for any intentional output changes.
- Dependencies (`sqlx`, `rmcp`) are always included; no feature gating in v0.3.
- SQLx uses `runtime-tokio-rustls` to match the existing `tokio` runtime used by `mysql_async`, `tiberius`, and the CLI.
- No new public library API beyond what is required by the CLI and MCP server.
- Read-only access only; no database mutations.
- Application version bumped to `0.3.0`.

------------------------------------------------------------------------

## 5. Existing Engine Policy

MySQL/MariaDB and SQL Server keep their existing `mysql_async` and `tiberius` drivers in v0.3. Migration to SQLx is deferred to a future phase. The generic `attributes` map must still be empty for these engines unless they naturally provide a fact that fits.

------------------------------------------------------------------------

## 6. Specification Updates

The following documents must be updated inline before or alongside implementation:

- `SPEC.md` — add `postgres` and `sqlite` to supported engines; update driver names and defaults; add `execute-statement` engine support.
- `ARCHITECTURE.md` — add `src/database/postgres.rs` and `src/database/sqlite.rs`; add MCP server layer/module; document `attributes` extension.
- `FORMAT.md` — update engine enum (`postgres`, `sqlite`); document `attributes` on modeled objects; clarify schema-qualified table file naming.
- `CLI.md` — add `dbctx mcp` command, `--driver sqlite`/`postgres`, multiple `--database` semantics for SQLite.
- `TESTING.md` — add PostgreSQL Docker matrix entries and SQLite local integration test approach.
- `ROADMAP.md` — mark v0.3 complete, move MCP resources out of "Future."

------------------------------------------------------------------------

## 7. Open Questions — Resolved

1. **Exact `rmcp` version and transport API usage.** — Use latest `rmcp` 0.1.x; stdio default, optional SSE.
2. **Exact SQLx feature flags.** — `runtime-tokio-rustls` to match the existing tokio runtime.
3. **MCP server connection lifecycle.** — Open a connection pool at startup, reuse across requests.
4. **Mapping of MCP resource URIs to canonical model data.** — Load canonical model into memory at startup; serve resources from cached model; regenerate only on explicit refresh.
5. **Signature and arguments of the three MCP prompts.** — No arguments; operate on the whole schema.
6. **Specific `pg_catalog` views beyond `information_schema`.** — Broad enrichment: access_method, tablespace, identity columns, generated columns, comments.
7. **SQLite Docker Compose discovery heuristic.** — Image name containing `sqlite` or command referencing `sqlite3`; mounted `.db` files as candidates.
8. **Migrate MySQL/MariaDB/SQL Server to `sqlx`.** — Keep existing drivers; SQLx for new engines only.
9. **Exact SQLite `STRICT` validation heuristic.** — Report `STRICT` tables where a `NOT NULL` column lacks an explicit `DEFAULT`.
10. **`dbctx init` SQLite attached database prompts.** — Collect main file path only; attached databases configured separately.
