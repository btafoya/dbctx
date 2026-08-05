# CLI.md

# dbctx Command Line Interface Specification

**Version:** 1.0

## Purpose

This document defines the public command-line interface for `dbctx`. The
CLI is part of the project's public API and should remain stable within
a major release.

------------------------------------------------------------------------

# General Syntax

``` text
dbctx <COMMAND> [OPTIONS]
```

Global options:

``` text
-h, --help
-V, --version
-v, --verbose
-q, --quiet
--color <auto|always|never>
--log-format <text|json>
```

------------------------------------------------------------------------

# Connection Resolution

Unless explicitly overridden, `dbctx` resolves connections in this
order:

1.  CLI options
2.  Docker Compose autodiscovery
3.  `.dbctx.toml`
4.  `.env`
5.  Environment variables
6.  Interactive prompt (TTY only)

Common connection options:

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

`--driver` is detected from the image of a discovered container when
omitted, and required when nothing was discovered. `--port` defaults to
3306 for MySQL and MariaDB, 1433 for SQL Server, and 5432 for PostgreSQL;
SQLite has no port. `--host` defaults to `127.0.0.1` and, like `--port`,
is ignored for SQLite, which connects to a file.

`--compose-service` reads the container the service is running, so the
service must be up; the port dbctx reports is the one actually published,
not the one the compose file declares.

`--socket` applies to MySQL and MariaDB only.

`--database` may be given more than once. Every driver except `sqlite`
accepts exactly one value. For `sqlite`, the first is the main database
file and each further value is attached in order as `attach1`, `attach2`,
and so on:

``` bash
dbctx inspect --driver sqlite --database main.db --database archive.db
```

------------------------------------------------------------------------

# Commands

## inspect

Inspect a database and generate artifacts.

``` bash
dbctx inspect
```

Options:

``` text
--output <DIR>
--stdout
--format <json|markdown|all>
--analyze
--llm
--overwrite
--no-markdown
--no-json
--no-mermaid
```

Default output:

``` text
.ai/dbctx/
```

`--stdout` writes the Markdown document to stdout instead of files; only
one of `--stdout` or `--output` may be supplied.

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       4 Export failed
      64 Invalid CLI usage

------------------------------------------------------------------------

## validate

Validate the inspected schema.

``` bash
dbctx validate
```

Produces validation findings only.

Never modifies the database.

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       5 Validation failed
      64 Invalid CLI usage

------------------------------------------------------------------------

## graph

Generate a Mermaid ER diagram.

``` bash
dbctx graph
```

Options:

``` text
--output <FILE>
```

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       4 Export failure
      64 Invalid CLI usage

------------------------------------------------------------------------

## diff

Compare two exported schemas.

``` bash
dbctx diff old/schema.json new/schema.json
```

Outputs:

-   Added tables
-   Removed tables
-   Column changes
-   Index changes
-   Foreign key changes

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       4 Export failure
      10 Diff detected
      64 Invalid CLI usage

------------------------------------------------------------------------

## stats

Display schema statistics.

``` bash
dbctx stats
```

Example:

``` text
Tables:          42
Views:            3
Columns:        615
Indexes:        108
Foreign Keys:    67
```

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       4 Export failure
      64 Invalid CLI usage

------------------------------------------------------------------------

## init

Initialize a project.

``` bash
dbctx init
```

Creates:

``` text
.dbctx.toml
```

Options:

``` text
--force
```

Does not overwrite existing files unless `--force` is supplied.

Commands that connect read this file as a configuration source, ranked
between Docker Compose autodiscovery and `.env`.

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 General error
       3 Invalid configuration
       4 Export failure
      64 Invalid CLI usage

------------------------------------------------------------------------

## llm-txt

Emit the project's LLM self-documentation guide.

``` bash
dbctx llm-txt
```

Options:

``` text
--mode <stdout|file>
--output <FILE>
--stdout
```

Default behavior prints the guide to standard output. The guide is a
static, hand-written document for AI coding agents; it does not require a
database connection and does not inspect any schema.

Common usages:

``` bash
dbctx llm-txt                    # print to stdout
dbctx llm-txt --stdout           # explicit stdout
dbctx llm-txt --mode file        # write LLM.md in the working directory
dbctx llm-txt --output guide.md  # write to a specific file
```

`--output <FILE>` implies `--mode file`; `--stdout` implies `--mode stdout`.
For schema-aware agent context, use `dbctx inspect --llm` or `dbctx mcp`.

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 Runtime error
       4 Export failure
      64 Invalid CLI usage

------------------------------------------------------------------------

## execute-statement

Execute a single read-only SQL statement against the resolved
connection and print the result as JSON.

``` bash
dbctx execute-statement "SELECT COUNT(*) FROM users"
```

The command uses the same connection resolution as other database
commands. The statement is verified to be read-only before execution;
any mutating statement is rejected without contacting the database.

Options:

``` text
--query <SQL>
--timeout <SECONDS>
```

Exit codes:

    Code Meaning
  ------ ---------------------------
       0 Success
       1 General error
       2 Connection failed
       3 Invalid configuration
       7 Statement execution failed
       8 Write operation rejected
      64 Invalid CLI usage

------------------------------------------------------------------------

## mcp

Run an MCP server exposing the schema to MCP clients.

``` bash
dbctx mcp
dbctx mcp --sse-port 8080
```

The command resolves the connection exactly like every other database
command, reads the schema once, and serves it from an in-memory cache.
Only the `refresh-schema` tool re-reads the database; `execute-statement`
always talks to it directly.

Options:

``` text
--sse-port <PORT>
--introspection-timeout <SECONDS>
```

`--sse-port` serves the MCP Streamable HTTP transport on
`127.0.0.1:<PORT>` instead of the default stdio transport.
`--introspection-timeout` (default 30) bounds the initial schema read and
every `refresh-schema` call.

Resources: `dbctx://schema`, `dbctx://metadata`, `dbctx://graph`,
`dbctx://relationships`, `dbctx://tables/<schema>.<table>`.

Tools: `execute-statement`, `refresh-schema`.

Prompts: `summarize-schema`, `describe-table`, `explain-relationships`.

Exit codes:

    Code Meaning
  ------ ---------------------------
       0 Success
       1 Server could not start or exited with an error
       2 Connection failed
       3 Invalid configuration
      64 Invalid CLI usage

------------------------------------------------------------------------

# Output Options

``` text
--output <DIR>
--stdout
--no-markdown
--no-json
--no-mermaid
```

Only one of `--stdout` or `--output` may be specified.

------------------------------------------------------------------------

# Logging

``` text
-v
-vv
-vvv
```

Increasing verbosity reveals:

-   Connection discovery
-   SQL metadata queries
-   Export timing
-   Validation timing

------------------------------------------------------------------------

# Environment Variables

Supported:

``` text
DB_CONNECTION
DB_HOST
DB_PORT
DB_DATABASE
DB_USERNAME
DB_PASSWORD
```

`DB_CONNECTION` is the environment equivalent of `--driver`.

CLI options always take precedence.

------------------------------------------------------------------------

# Exit Codes

    Code Description
  ------ -----------------------
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

------------------------------------------------------------------------

# CLI Stability Policy

The following are considered stable:

-   Command names
-   Long option names
-   Exit codes
-   Output directory layout

Aliases may be added in minor releases.

Breaking CLI changes require a major version.

------------------------------------------------------------------------

# Examples

Inspect:

``` bash
dbctx inspect
```

Analyze:

``` bash
dbctx inspect --analyze
```

Generate AI context:

``` bash
dbctx inspect --llm
```

Docker Compose:

``` bash
dbctx inspect --compose-service mariadb
```

Custom output:

``` bash
dbctx inspect --output docs/database
```

Diff:

``` bash
dbctx diff previous/schema.json current/schema.json
```

Validate:

``` bash
dbctx validate
```

Graph:

``` bash
dbctx graph --output graph.mmd
```

MCP server:

``` bash
dbctx mcp
```

------------------------------------------------------------------------

# Future Commands (Out of Scope for Phase 1)

Reserved for future RFCs:

-   doctor
-   verify
-   doctor --fix
-   export
-   serve
-   explain
-   plugins
