# RFC 0001 -- Read-only SQL Execution

**Status:** Draft

## Summary

Add a single, narrow `execute-statement` command to `dbctx` v1.0.0 that
runs one read-only SQL statement against a resolved database
connection and returns the result as JSON. This crosses the previous
"SQL execution is out of scope" boundary while preserving the project's
read-only, facts-first guarantees.

## Motivation

`dbctx` intentionally avoids SQL execution to prevent accidental schema
or data mutation. However, agents and users occasionally need a safe,
trusted way to ask ad-hoc questions of a database during inspection:
row counts, sample rows, or reference lookups. Providing this inside
`dbctx` keeps the workflow inside a tool whose security stance is
already documented and tested, rather than forcing users to reach for a
separate, less constrained client.

The command is strictly read-only, rejects mutating statements before
execution, and never feeds its output into the canonical schema model.

## Design

### Command surface

``` bash
dbctx execute-statement "SELECT COUNT(*) FROM users"
dbctx execute-statement --query "SELECT COUNT(*) FROM users"
```

Connection resolution uses the same precedence as every other command:
CLI options, Docker Compose autodiscovery, `.dbctx.toml`, `.env`,
environment variables, interactive prompt.

Common connection options (`--host`, `--port`, `--user`, `--password`,
`--driver`, etc.) apply exactly as they do for `inspect`.

### Read-only enforcement

Before the statement is sent to the database, `dbctx` parses it enough
to detect the operation class. The following are rejected with a
dedicated exit code and no database contact:

-   `INSERT`, `UPDATE`, `DELETE`, `MERGE`
-   `CREATE`, `ALTER`, `DROP`, `TRUNCATE`
-   `GRANT`, `REVOKE`, `EXECUTE` where the target is a stored procedure
-   Any statement containing multiple semicolon-separated statements

Only `SELECT` and read-only system information queries are allowed.
Engine-specific read-only helpers (for example, SQL Server
`SELECT ... FROM sys.*`) are permitted because they cannot mutate data.

### Output

Results are emitted as a single JSON document:

``` json
{
  "columns": ["count"],
  "rows": [[42]],
  "row_count": 1,
  "execution_time_ms": 12
}
```

This format is independent of the canonical schema model exporters.

### Isolation from the canonical model

`execute-statement` uses the connection layer but bypasses:

-   Introspection
-   Canonical schema model population
-   Validation
-   Analysis
-   AI context
-   All exporters except the built-in JSON result serializer

This guarantees that ad-hoc queries cannot become part of the factual
record.

### Exit codes

``` text
 0 Success
 1 General error
 2 Connection failed
 3 Invalid configuration
 7 Statement execution failed
 8 Write operation rejected
64 Invalid CLI usage
```

## Alternatives

1.  **Leave SQL execution out of scope.** Rejected because users
    already run ad-hoc queries with untrusted tools; providing a
    constrained, tested path is safer.
2.  **Allow arbitrary SQL with a `--dangerous` flag.** Rejected because
    it conflicts with the project's read-only security stance.
3.  **Return results in Markdown or plain text instead of JSON.**
    Rejected because JSON is predictable, parseable, and consistent with
    the rest of the CLI.

## Drawbacks

-   Adds a parser/validator for SQL operation classes, which is a new
    class of code to maintain.
-   Slightly broadens the CLI surface, increasing documentation and
    test burden.
-   Users may still attempt to bypass the validator with tricky
    syntax; the validator must be reviewed and tested continuously.

## Compatibility

-   No changes to existing commands, options, or output formats.
-   No changes to the canonical schema model.
-   Document formats remain versioned independently.
-   The read-only guarantee is strengthened, not weakened.

## Unresolved Questions

1.  Should the validator be engine-specific (MySQL, MariaDB, SQL
    Server) or a single shared whitelist?
2.  Should `--timeout` have a default, and if so, what value?
3.  Should result sets be streamed or buffered entirely in memory?
4.  Should `llm-txt` include the result of a sample `execute-statement`
    run, or always remain static documentation?
