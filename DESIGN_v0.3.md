# Design: dbctx v0.3

**Status:** Draft. Awaiting approval before implementation.

This document turns `REQUIREMENTS_v0.3.md` into a concrete design. It covers module layout, data flow, command additions, canonical model changes, database introspection, MCP server, and testing.

------------------------------------------------------------------------

## 1. Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| SQLx runtime/TLS | `runtime-tokio-rustls` | Matches the existing `tokio` runtime used by `mysql_async`, `tiberius`, and the CLI. Avoids pulling in `async-std`. |
| Existing engines | Keep `mysql_async` and `tiberius` | Minimizes scope and risk. Migration to SQLx is a future phase. |
| MCP transport | stdio default, optional SSE | stdio is the simplest and most compatible MCP transport; SSE via `--sse-port` for headless use. |
| MCP SDK | `rmcp` latest `0.1.x` | Required by the requirements; we accept API churn. |
| MCP data model | Cache canonical model at startup, serve from memory | Avoids re-querying the database for every resource request. Explicit refresh tool regenerates the cache. |
| MCP resources | Clean-path URIs (`dbctx://schema`, `dbctx://tables/<schema>.<table>`, etc.) | Required by the requirements. |
| Canonical model extension | Generic `attributes: BTreeMap<String, serde_json::Value>` on all modeled objects | Backward compatible; keeps engine-specific facts out of fixed fields. |
| Format version | Stay at `1.0` | Per `FORMAT.md` readers must ignore unknown fields. |
| SQLite attachment naming | `main`, `attach1`, `attach2`, ... in `--database` order | Simple and deterministic. |
| Table file naming for SQLite | Always schema-qualified (`main.customers.json`) | Matches SQL Server convention and avoids collisions. |

------------------------------------------------------------------------

## 2. Module Layout

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
│   ├── postgres.rs          # new
│   └── sqlite.rs            # new
├── model.rs                 # extended with attributes
├── validation.rs            # + engine-specific rules
├── analysis.rs              # unchanged behavior
├── ai.rs                    # unchanged behavior
├── stats.rs                 # unchanged behavior
├── diff.rs                  # unchanged behavior
├── export.rs                # unchanged behavior
├── execution.rs             # + postgres, sqlite backends
├── mcp.rs                   # new: dbctx mcp subcommand handler
├── mcp_server.rs            # new: rmcp server implementation
└── error.rs
```

The `mcp` module is the CLI entry point; `mcp_server` is the rmcp protocol implementation. This keeps protocol details out of `main.rs`.

------------------------------------------------------------------------

## 3. Canonical Model Extension

### 3.1 Add `Engine::Postgres` and `Engine::SQLite`

In `src/model.rs`:

``` rust
pub enum Engine {
    Mysql,
    Mariadb,
    Sqlserver,
    Postgres,
    Sqlite,
}
```

Serialization as lowercase: `postgres`, `sqlite`.

### 3.2 Add `attributes` to all modeled objects

Add to:

- `Database`
- `DatabaseMetadata`
- `Table`
- `Column`
- `Index`
- `ForeignKey`
- `View`

``` rust
#[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
pub attributes: BTreeMap<String, serde_json::Value>,
```

Notes:

- Use `BTreeMap` for deterministic ordering.
- Skip serialization when empty so existing engine output is unchanged.
- Default to empty on deserialization so older documents still load.
- Engine-specific facts live here; fixed fields remain unchanged.

### 3.3 Snapshot Impact

- Existing snapshots for MySQL/MariaDB/SQL Server must not change (attributes are empty and skipped).
- New snapshots for Postgres/SQLite will include non-empty attributes where the engine provides them.

------------------------------------------------------------------------

## 4. Configuration Changes

### 4.1 Add `Driver::Postgres` and `Driver::Sqlite`

In `src/config.rs`:

``` rust
pub enum Driver {
    Mysql,
    Mariadb,
    Sqlsrv,
    Postgres,
    Sqlite,
}
```

- CLI names: `postgres`, `sqlite`.
- Default ports:
  - `Postgres`: 5432
  - `Sqlite`: no default (host/port ignored)

### 4.2 Multiple `--database` for SQLite

`ConnectionArgs.database` becomes `Vec<String>`. The first is the main database; subsequent values are attached databases.

This affects:

- `ConnectionArgs::source()`
- `ConnectionSource::database` type (from `Option<String>` to `Vec<String>`)
- `ConnectionConfig::resolve()` — for SQLite, the first entry is the database; for other engines, exactly one entry is required and validated.
- `ConnectionConfig::missing()` — checks whether any database is supplied.
- `ProjectConfig::database` in `.dbctx.toml` stays a single string for the main database.

### 4.3 SQLite Attachments in `.dbctx.toml`

Attachments are configured under `[dbctx.sqlite.attach]` as `name = "path"` entries. Example:

``` toml
[dbctx]
driver = "sqlite"
database = "main.db"

[dbctx.sqlite.attach]
archive = "archive.db"
```

The `ProjectConfig` struct gains a `sqlite_attach: BTreeMap<String, String>` field parsed from this table. Config file validation rejects unknown keys under `[dbctx]` itself.

### 4.4 `DB_CONNECTION` and `.dbctx.toml`

Both accept `postgres` and `sqlite`. Update `MissingSetting::Driver` prompt text to list all five drivers.

### 4.5 `DB_DATABASE`

Can be a single path for SQLite. For multiple attached SQLite databases, users must use CLI or config; environment variables only support a single database.

------------------------------------------------------------------------

## 5. Connection Discovery

### 5.1 Docker Compose: PostgreSQL

Extend `driver_from_image` in `src/discovery.rs`:

``` rust
if image.contains("postgres") {
    Some(Driver::Postgres)
}
```

`credentials` for PostgreSQL:

- database: `POSTGRES_DB` or `POSTGRES_DATABASE`, else default `postgres`
- user: `POSTGRES_USER`, else `postgres`
- password: `POSTGRES_PASSWORD`

### 5.2 Docker Compose: SQLite (best-effort)

No published port. Discovery looks for:

- image name contains `sqlite`, or
- command references `sqlite3`.

If found, collect any mounted paths ending in `.db`. The first is `main`; the rest become attached databases in mount order. Because SQLite has no default port, the discovered source carries `driver: Some(Sqlite)` and `database: Vec<String>` but no host/port.

### 5.3 `published_host_port` and SQLite

SQLite discovery bypasses the port check. Add a new helper or branch in `source_from_inspect` for drivers that do not need a port.

------------------------------------------------------------------------

## 6. Database Introspection

### 6.1 PostgreSQL (`src/database/postgres.rs`)

Use `sqlx` with `runtime-tokio-rustls`.

Connection:

``` rust
sqlx::postgres::PgPoolOptions::new()
    .max_connections(1)
    .connect(&format!(
        "postgres://{user}:{password}@{host}:{port}/{database}",
        ...
    ))
```

Introspection queries:

1. **Database metadata**
   ``` sql
   SELECT current_database(), version();
   ```

2. **Tables** — `information_schema.tables`
   - Filter `table_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast*')`.
   - `table_name`, `table_schema`.
   - Supplement from `pg_class`/`pg_namespace` for access_method and tablespace.

3. **Columns** — `information_schema.columns`
   - `ordinal_position`, `column_name`, `data_type`, `is_nullable`, `column_default`.
   - `is_identity`, `generation_expression` from `information_schema.columns`.
   - Comments from `col_description`.

4. **Indexes** — `pg_indexes` + `pg_index` + `pg_attribute`
   - Determine uniqueness, index type, columns.

5. **Primary keys / unique constraints** — `information_schema.table_constraints` + `key_column_usage`
   - Mark `primary_key` and `unique` on columns.

6. **Foreign keys** — `information_schema.referential_constraints` + `key_column_usage`
   - `constraint_name`, `on_update`, `on_delete`, local columns, referenced schema/table/columns.

7. **Views** — `information_schema.views`
   - Name, schema, columns.

Engine-specific attributes:

- Table: `access_method`, `tablespace`, `row_security`, `partition_status` (if partitioned).
- Column: `is_identity`, `identity_generation` (ALWAYS/BY DEFAULT), `is_generated`, `generation_expression`, `collation`, `comment`.
- Index: `index_type` (e.g., `btree`, `hash`, `gin`, `gist`), `is_primary`, `is_unique`.
- ForeignKey: `deferrable`, `initially_deferred`.

### 6.2 SQLite (`src/database/sqlite.rs`)

Use `sqlx` with `runtime-tokio-rustls`.

Connection for the main database:

``` rust
sqlx::sqlite::SqlitePoolOptions::new()
    .max_connections(1)
    .connect(&format!("sqlite://{path}"))
```

Attached databases are attached via `ATTACH DATABASE 'path' AS name` on the same connection.

Introspection queries:

1. **Tables** — `SELECT name, sql FROM sqlite_master WHERE type = 'table'`
   - Parse `WITHOUT ROWID` and `STRICT` from `sql`.
   - Schema name is the database name (`main`, `attach1`, ...).

2. **Columns** — `PRAGMA table_info(<schema>.<table>)`
   - cid, name, type, notnull, default value, pk flag.

3. **Indexes** — `PRAGMA index_list(<schema>.<table>)` and `PRAGMA index_info(<index>)`
   - Name, uniqueness, columns, origin (u=unique, c=created by user).

4. **Foreign keys** — `PRAGMA foreign_key_list(<schema>.<table>)`
   - id, seq, table, from, to, on_update, on_delete.

5. **Views** — `SELECT name, sql FROM sqlite_master WHERE type = 'view'`
   - Columns via `PRAGMA table_info(<schema>.<view>)`.

Engine-specific attributes:

- Table: `without_rowid`, `strict`.
- Column: `collation`, `pk` (already mapped to `primary_key`), `hidden`.
- Index: `origin`.
- ForeignKey: nothing beyond fixed fields.

### 6.3 `src/database/mod.rs`

Extend `inspect`:

``` rust
match config.driver() {
    Driver::Mysql | Driver::Mariadb => mysql::inspect(config).await,
    Driver::Sqlsrv => sqlserver::inspect(config).await,
    Driver::Postgres => postgres::inspect(config).await,
    Driver::Sqlite => sqlite::inspect(config).await,
}
```

### 6.4 Error Handling

Both new backends produce `DatabaseError::Connection` and `DatabaseError::Catalog` using existing helpers.

------------------------------------------------------------------------

## 7. Export Changes

No exporter changes are required beyond the model extension. Exporters already consume the canonical model; the new `attributes` field will flow through serialization.

For SQLite, the table file naming in `src/export.rs` must use `format!("{schema}.{table}.json")` unconditionally. Currently SQL Server uses schema-qualified naming and MySQL/MariaDB do not. Add a helper: schema-qualify for SQL Server and SQLite; bare name for MySQL/MariaDB.

------------------------------------------------------------------------

## 8. Validation Changes

Add two engine-specific rules in `src/validation.rs`:

1. **`postgres_missing_primary_key`**: for tables where `metadata.engine == Postgres`, report any table with no column where `primary_key == true`.
2. **`sqlite_without_rowid_missing_primary_key`**: for SQLite tables where `attributes["without_rowid"] == true`, report if no column has `primary_key == true`.
3. **`sqlite_strict_missing_default_on_not_null`**: for SQLite tables where `attributes["strict"] == true`, report any column where `nullable == false`, `default == None`, and the column is not auto-increment.

Rules only produce findings; they never modify the model.

------------------------------------------------------------------------

## 9. Execute-Statement Changes

Add PostgreSQL and SQLite execution paths in `src/execution.rs`.

- PostgreSQL: use `sqlx::query` on a fresh or pooled Postgres connection.
- SQLite: use `sqlx::query` on the SQLite connection.

Value serialization must handle SQLx's `JsonValue` mapping or manual conversion for types not natively mapped. Keep the same read-only whitelist.

------------------------------------------------------------------------

## 10. MCP Server

### 10.1 `dbctx mcp` CLI

Add in `src/cli.rs`:

``` rust
pub enum Command {
    ...
    Mcp(McpArgs),
}

pub struct McpArgs {
    #[command(flatten)]
    pub connection: ConnectionArgs,

    /// Run the MCP server over HTTP/SSE instead of stdio.
    #[arg(long, value_name = "PORT")]
    pub sse_port: Option<u16>,

    /// Seconds before regenerating the cached schema times out.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub introspection_timeout: u64,
}
```

### 10.2 `src/mcp.rs`

CLI entry point for `dbctx mcp`:

1. Resolve configuration from `ConnectionArgs` using `discovery::resolve`.
2. Build a tokio runtime using the same `tokio::runtime::Runtime::new()` pattern as other commands.
3. Call `mcp_server::run(config, options).await`.

The runtime is created per invocation and lives for the MCP server's lifetime.

### 10.3 `src/mcp_server.rs`

Implement an `rmcp`-based server:

- `ServerHandler` trait implementation.
- State holds:
  - `ConnectionConfig`
  - `Arc<RwLock<CachedSchema>>`
  - optional `SqlxPool` for `execute-statement`
- On startup:
  1. Open connection pool.
  2. Run introspection.
  3. Store canonical `Database` model in cache.

#### Resources

Register resources at startup with the `rmcp` server:

| URI | Content |
|---|---|
| `dbctx://schema` | `schema.json` content (header + metadata + tables + views + relationships) |
| `dbctx://metadata` | `metadata.json` content |
| `dbctx://graph` | `graph.mmd` content |
| `dbctx://relationships` | `relationships.json` content |
| `dbctx://tables/<schema>.<table>` | Single table JSON document |

Resource content is generated from the cached `Database` model using the existing exporters, not by re-querying the database.

#### Tools

- `execute-statement`:
  - Arguments: `sql` (string), optional `timeout` (number).
  - Validate read-only using existing `execution::validate_read_only`.
  - Execute against the connection pool.
  - Return JSON serialized `ExecutionResult`.
- `refresh-schema`:
  - No arguments.
  - Re-runs introspection.
  - Updates cached `Database`.
  - Returns success or error.

#### Prompts

- `summarize-schema`: returns a text description of the database, table count, view count, key relationships.
- `describe-table`: returns a text description of every table (no argument; whole schema).
- `explain-relationships`: returns a text narrative of all foreign key relationships.

These prompts are deterministic templates based on the cached model. No LLM is involved.

### 10.4 Transport

- stdio: `rmcp` provides stdio transport out of the box.
- SSE: implemented with `axum` on the port from `--sse-port`. The axum server exposes the MCP SSE endpoint and forwards JSON-RPC messages to the same `ServerHandler` used for stdio.

### 10.5 Dependencies

Add to `Cargo.toml`:

``` toml
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "sqlite"] }
rmcp = { version = "0.1", features = ["server"] }
axum = { version = "0.8", default-features = false, features = ["tokio"] }
```

`axum` is used only for the optional `--sse-port` transport. stdio mode does not start an HTTP stack.

------------------------------------------------------------------------

## 11. Init Command Changes

Update `CONFIG_TEMPLATE` in `src/main.rs` to list `postgres` and `sqlite` in the driver comment and add example PostgreSQL/SQLite settings.

Interactive prompts in `ConnectionSource::from_prompt`:

- `MissingSetting::Driver` prompt text updates to include `postgres` and `sqlite`.
- For SQLite, prompt for the main database file path. The prompt currently asks for `Database`; for SQLite this is interpreted as a file path.

No repeated attach prompts during init per the requirements.

------------------------------------------------------------------------

## 12. Documentation Updates

Update these files inline (no separate RFC/ADR):

- `SPEC.md` — supported engines, driver names, defaults, `execute-statement` engines.
- `ARCHITECTURE.md` — new database modules, MCP server layer, `attributes` extension.
- `FORMAT.md` — engine enum, `attributes` field, table file naming for SQLite.
- `CLI.md` — `dbctx mcp` command, `--driver postgres|sqlite`, `--sse-port`, multiple `--database` semantics.
- `TESTING.md` — PostgreSQL Docker matrix (14, 15, 16, 17), SQLite local integration tests, MCP server testing approach.
- `ROADMAP.md` — v0.3 complete; move MCP resources out of Future.
- `README.md` and `LLM.md` — update feature lists if they mention supported engines.

------------------------------------------------------------------------

## 13. Testing Strategy

### 13.1 Unit Tests

- `model.rs`: `attributes` round-trip, default empty, serialization skip.
- `config.rs`: parse `postgres` and `sqlite`, default ports, multiple `--database`, unknown driver error message.
- `cli.rs`: `dbctx mcp` parses; `--sse-port` parses; multiple `--database` parses.
- `discovery.rs`: `driver_from_image` for Postgres and SQLite.
- `validation.rs`: new engine-specific rules with positive/negative/edge cases.
- `execution.rs`: read-only validation still works for Postgres/SQLite statements.

### 13.2 Integration Tests

Create new files under `tests/integration/`:

- `postgres.rs`: spin up Postgres via testcontainers or the existing Docker helper; create schema; run `inspect`; assert JSON output.
- `sqlite.rs`: create temp `.db` files with attached databases; run `inspect`; assert schema names and table file names.
- `mcp.rs`: spawn `dbctx mcp` as a subprocess over stdio; send JSON-RPC `initialize`; read resources; verify `execute-statement` tool.

### 13.3 Docker Matrix

Add PostgreSQL entries to the CI matrix:

``` text
postgres:14
postgres:15
postgres:16
postgres:17
```

SQLite tests run locally without Docker.

### 13.4 Snapshot Tests

Add golden snapshots:

- `postgres-basic/`
- `sqlite-basic/`
- `sqlite-attached/`

Update existing snapshots only if the `attributes` field unintentionally appears (it should be skipped when empty).

### 13.5 Performance

No new performance targets. The MCP cache avoids repeated introspection; explicit refresh re-runs the same 500-table target.

------------------------------------------------------------------------

## 14. Risks and Mitigations

| Risk | Mitigation |
|---|---|
| `rmcp` 0.1.x API churn | Pin to the latest version; add a single module that wraps rmcp so changes are localized. |
| SQLx tokio-rustls feature not matching other dependencies | Verify `cargo tree` shows only one tokio runtime; address duplicates early. |
| SQLite `PRAGMA` behavior with attached schemas | Always qualify pragmas with schema (`main.table_info(...)`), attach databases explicitly before introspection. |
| PostgreSQL schema vs database semantics | Use `current_database()` for `metadata.database`; use `table_schema` for `Table.schema`. |
| MCP SSE implementation complexity | Start with stdio only; add SSE only after stdio works. Document SSE as optional. |
| `attributes` field leaking into all snapshots | Use `#[serde(skip_serializing_if = "BTreeMap::is_empty")]`; add a test that empty attributes are not emitted. |
| Multiple `--database` breaks existing CLI tests | Update `ConnectionArgs::database` carefully; ensure single-value cases still work for other engines. |

------------------------------------------------------------------------

## 15. Open Design Questions — Resolved

1. **MCP runtime pattern.** — Reuse the existing ad-hoc `tokio::runtime::Runtime::new()` pattern.
2. **Exact `rmcp` API.** — To be determined once the dependency is added and its examples are reviewed.
3. **SSE transport.** — Use `axum` on `--sse-port`; stdio is the default.
4. **SQLite attachments in `.dbctx.toml`.** — Use `[dbctx.sqlite.attach]` with `name = "path"` entries; `database` remains the main file.

------------------------------------------------------------------------

## 16. Approval Checklist

- [ ] Module layout accepted.
- [ ] Canonical model `attributes` approach accepted.
- [ ] SQLite `--database` multiple-value approach accepted.
- [ ] MCP server caching approach accepted.
- [ ] SQLx feature flags accepted.
- [ ] Risk mitigations accepted or overridden.
- [ ] Specification document update scope accepted.
- [ ] SQLite `.dbctx.toml` attachment approach accepted.
- [ ] SSE via `axum` accepted.
- [ ] MCP runtime reuse accepted.

Once approved, the next step is `/sc:implement`.
