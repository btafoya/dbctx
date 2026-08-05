# LLM.md

# dbctx Agent Guide

A hand-written guide for AI coding agents using the `dbctx`
application.

------------------------------------------------------------------------

## Project Purpose

`dbctx` generates accurate, deterministic, versioned database context
for humans and AI coding agents. It reads catalog metadata from
relational databases and exports it as JSON, Markdown, Mermaid ER
diagrams, and related artifacts.

Core promise: **facts first, stable output, no hidden side effects**.

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

Command names, long option names, exit codes, and output directory layout
are stable within a major release. Aliases may be added in minor releases;
breaking changes require a major version.

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

## License

MIT OR Apache-2.0.
