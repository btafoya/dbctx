# LLM.md

# dbctx Agent Guide

A hand-written guide for AI coding agents working on the `dbctx`
project.

------------------------------------------------------------------------

## Project Purpose

`dbctx` generates accurate, deterministic, versioned database context
for humans and AI coding agents. It reads catalog metadata from
relational databases and exports it as JSON, Markdown, Mermaid ER
diagrams, and related artifacts.

Core promise: **facts first, stable output, no hidden side effects**.

------------------------------------------------------------------------

## Architecture

The codebase is layered:

    CLI (clap)
        │
        ▼
    Configuration
        │
        ▼
    Connection Discovery
        │
        ▼
    Database Introspection
        │
        ▼
    Canonical Schema Model
        │
        ├── Validation
        ├── Analysis (optional)
        ├── AI Context (optional)
        └── Exporters

Dependency rules:

-   Higher layers consume lower layers only.
-   Exporters never query databases.
-   Validation and analysis never mutate factual metadata.
-   AI context never overwrites facts.
-   `execute-statement` (v1.0) is an isolated read-only path that returns
    query results directly and never feeds them into the canonical model.

------------------------------------------------------------------------

## Conventions

-   Use `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before
    committing.
-   Write tests before or alongside implementation.
-   Prefer plain data structures; keep models free of engine-specific
    logic.
-   Use `thiserror` in the library and `anyhow` in the CLI.
-   Never panic for recoverable failures.
-   Preserve stable, deterministic ordering in all exported artifacts.
-   Match existing style; do not refactor adjacent code.
-   Use RFC/ADR processes for major changes.

------------------------------------------------------------------------

## File Map

| File | Purpose |
|---|---|
| `VISION.md` | Philosophy and non-goals |
| `SPEC.md` | Behavior contract |
| `ARCHITECTURE.md` | Layers, dependencies, invariants |
| `FORMAT.md` | Output document formats |
| `CLI.md` | Command names, options, exit codes |
| `TESTING.md` | Test strategy and gates |
| `ROADMAP.md` | Release scope |
| `CONTRIBUTING.md` | PR workflow and commit style |
| `LLM.md` | This guide |
| `src/main.rs` | CLI binary entry point |
| `src/lib.rs` | Public library exports |
| `src/cli.rs` | CLI parsing and command dispatch |
| `src/config.rs` | Configuration merging and resolution |
| `src/discovery.rs` | Docker Compose and prompt discovery |
| `src/database/mod.rs` | Introspection traits |
| `src/database/mysql.rs` | MySQL / MariaDB catalog reader |
| `src/database/sqlserver.rs` | SQL Server catalog reader |
| `src/database/postgres.rs` | PostgreSQL catalog reader |
| `src/database/sqlite.rs` | SQLite catalog reader |
| `src/model.rs` | Canonical schema model |
| `src/validation.rs` | Schema validation rules |
| `src/analysis.rs` | Deterministic schema heuristics |
| `src/ai.rs` | Optional, labeled AI context |
| `src/stats.rs` | Schema statistics |
| `src/diff.rs` | Exported schema comparison |
| `src/export.rs` | JSON, Markdown and Mermaid exporters |
| `src/execution.rs` | Read-only `execute-statement` runner |
| `src/mcp.rs` | `dbctx mcp` CLI entry point |
| `src/mcp_server.rs` | MCP server: resources, tools, prompts |
| `src/error.rs` | Unified error type |

------------------------------------------------------------------------

## CLI Contract

Global options:

``` text
-h, --help
-V, --version
-v, --verbose
-q, --quiet
--color <auto|always|never>
--log-format <text|json>
```

Connection resolution order:

1.  CLI options
2.  Docker Compose autodiscovery
3.  `.dbctx.toml`
4.  `.env`
5.  Environment variables
6.  Interactive prompt (TTY only)

Commands:

``` text
dbctx init
dbctx inspect
dbctx validate
dbctx graph
dbctx diff
dbctx stats
dbctx llm-txt
dbctx execute-statement
dbctx mcp
```

### `dbctx llm-txt`

Emit this static agent guide.

Default behavior is to print the guide to standard output so it can be
piped, redirected, or absorbed directly by an AI coding agent:

``` bash
dbctx llm-txt                    # prints to stdout
dbctx llm-txt --stdout           # explicit stdout
dbctx llm-txt --mode file        # writes LLM.md in the working directory
dbctx llm-txt --output guide.md  # writes to a specific file
```

`--mode <stdout|file>` selects the destination. `--output <FILE>` implies
`--mode file`. `--stdout` implies `--mode stdout` and is equivalent to the
default. This command emits only the hand-written project guide; it does
not introspect a database. For schema-aware context, use `dbctx inspect
--llm` or run `dbctx mcp` and read the `dbctx://schema` resource.

### `dbctx init`

Create a `.dbctx.toml` configuration file in the current directory. Use
`--force` to overwrite an existing file. This file ranks below CLI options
and Docker Compose autodiscovery, and above `.env` and environment
variables.

### `dbctx inspect`

Read the database catalog and export artifacts to `.ai/dbctx/` by default.

Common options:

``` text
--output <DIR>         Write artifacts here instead of .ai/dbctx
--stdout              Write Markdown to stdout instead of files
--format <json|markdown|all>
--analyze             Add deterministic analysis (junction/lookup/audit tables, soft deletes, timestamps)
--llm                 Add labeled AI-generated summaries and narratives
--overwrite           Replace existing artifacts
--no-markdown         Skip schema.md
--no-json             Skip schema.json and per-table JSON
--no-mermaid          Skip graph.mmd
```

Use `--analyze` for deterministic heuristics and `--llm` for optional,
labeled AI context. Neither mutates factual metadata.

### `dbctx validate`

Inspect the database, run validation rules, and print findings as JSON.
Exit code `5` means findings were detected. This command never modifies
the database.

### `dbctx graph`

Inspect the database and emit a Mermaid ER diagram. Write to a file with
`--output <FILE>`; otherwise print to stdout.

### `dbctx diff`

Compare two exported `schema.json` files and print a JSON report. Exit code
`10` means differences were detected.

``` bash
dbctx diff previous/schema.json current/schema.json
```

### `dbctx stats`

Inspect the database and print a short schema statistics summary:

``` text
Tables:          42
Views:            3
Columns:        615
Indexes:        108
Foreign Keys:    67
```

### `dbctx execute-statement`

Run a single read-only SQL statement and print the result as JSON. The
statement is validated to be read-only before contacting the database.
Mutating statements are rejected with exit code `8`.

``` bash
dbctx execute-statement "SELECT COUNT(*) FROM users"
dbctx execute-statement --query "SELECT * FROM orders LIMIT 10" --timeout 10
```

Default timeout is 30 seconds.

### `dbctx mcp`

Run an MCP server exposing the schema to MCP clients. Default transport
is stdio; use `--sse-port <PORT>` for HTTP/SSE.

Resources:

- `dbctx://schema`
- `dbctx://metadata`
- `dbctx://graph`
- `dbctx://relationships`
- `dbctx://tables/<schema>.<table>`

Tools: `execute-statement`, `refresh-schema`.

Prompts: `summarize-schema`, `describe-table`, `explain-relationships`.

### Connection options

Common options accepted by database commands:

``` text
--host <HOST>
--port <PORT>
--database <NAME>
--user <USER>
--password <PASSWORD>
--driver <mysql|mariadb|sqlsrv|postgres|sqlite>
--socket <PATH>
--env <FILE>
--compose-service <SERVICE>
--docker-container <CONTAINER>
```

`--driver` is detected from a discovered container image when omitted.
`--port` defaults to 3306 for MySQL/MariaDB, 1433 for SQL Server, 5432
for PostgreSQL. SQLite ignores host/port and connects to a file.
`--database` may be repeated for SQLite attachments only.

Stable v1.0 commands are `llm-txt` and `execute-statement`. Aliases may
be added in minor releases; breaking changes require a major version.

Exit codes:

``` text
 0 Success
 1 General error
 2 Connection failed
 3 Invalid configuration
 4 Export failed
 5 Validation failed
 6 Unsupported database
 7 Statement execution failed
 8 Write operation rejected
10 Diff detected
64 Invalid CLI usage
```

------------------------------------------------------------------------

## Testing Rules

Required gates:

``` bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
cargo test
```

-   Unit tests cover configuration, model, validation, analysis, and
    export helpers.
-   Integration tests use disposable databases.
-   Snapshot tests use `insta` for JSON, Markdown, and Mermaid output.
-   Golden files live under `testdata/`.
-   Docker matrix: MySQL 8.0/8.4, MariaDB 10.11/11, SQL Server
    2019/2022, PostgreSQL 14/15/16/17. SQLite tests run locally against
    temporary files, no Docker container.
-   Every bug fix requires a regression test.
-   Do not disable failing tests; fix them.

------------------------------------------------------------------------

## Extending dbctx

Adding a new database:

1.  Propose an RFC.
2.  Implement the introspection interface under `src/database/`.
3.  Populate the canonical model only.
4.  Add the database to the Docker test matrix.

Adding a new exporter:

1.  Add an exporter under `src/exporters/`.
2.  Consume the canonical model only.
3.  Add snapshot tests and update golden files.
4.  Update `FORMAT.md`.

Adding a validation rule:

1.  Write a positive, negative, and edge-case test.
2.  Implement the rule in the validation layer.
3.  Never modify schema data.

Adding analysis:

1.  Keep heuristics deterministic.
2.  Do not use LLMs.
3.  Label inferred metadata clearly.

------------------------------------------------------------------------

## Security Stance

-   Read-only database access only.
-   `execute-statement` rejects mutating SQL before execution.
-   No credential persistence.
-   Passwords are redacted from logs and debug output.
-   No outbound network calls.
-   No telemetry or analytics.
-   Do not introduce features that weaken these guarantees without an
    RFC and explicit project approval.

------------------------------------------------------------------------

## AI-Generated Output Sections

When a feature produces AI-generated content:

-   Label every generated section as AI-generated.
-   Preserve all factual metadata unchanged.
-   Make AI sections removable without breaking the document.
-   Keep AI output optional and opt-in.
-   Ensure the artifact remains useful without the AI content.

This applies to `--llm` context and any future AI-assisted exporters.

------------------------------------------------------------------------

## License

MIT OR Apache-2.0.
