# CLI.md

# dbctx Command Line Interface Specification

**Version:** 0.1 (Draft)

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
3.  `.env`
4.  Environment variables
5.  Interactive prompt (TTY only)

Common connection options:

``` text
--host <HOST>
--port <PORT>
--database <NAME>
--user <USER>
--password <PASSWORD>
--driver <mysql|mariadb|sqlsrv>
--socket <PATH>
--env <FILE>
--compose-service <SERVICE>
--docker-container <CONTAINER>
```

`--driver` is detected from the connection when omitted. `--port`
defaults to 3306 for MySQL and MariaDB and 1433 for SQL Server.

`--socket` applies to MySQL and MariaDB only.

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
--format <json|markdown|all>
--analyze
--llm
--overwrite
```

Default output:

``` text
.ai/dbctx/
```

Exit codes:

    Code Meaning
  ------ ---------------------
       0 Success
       1 Runtime error
       2 Connection failure
       3 Configuration error
       4 Export failure

------------------------------------------------------------------------

## validate

Validate the inspected schema.

``` bash
dbctx validate
```

Produces validation findings only.

Never modifies the database.

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

Exit status:

0 = no differences

10 = differences detected

------------------------------------------------------------------------

## stats

Display schema statistics.

Example:

``` text
Tables:          42
Views:            3
Columns:        615
Indexes:        108
Foreign Keys:    67
```

------------------------------------------------------------------------

## init

Initialize a project.

Creates:

``` text
.dbctx.toml
```

Does not overwrite existing files unless `--force` is supplied.

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
