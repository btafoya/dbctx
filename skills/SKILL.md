---
name: dbctx
description: |
  Help the user use the `dbctx` CLI to inspect MySQL, MariaDB, and SQL Server databases and work with the artifacts it generates.
  Use this skill whenever the user mentions `dbctx`, `.ai/dbctx`, `schema.json`, database context for AI coding agents, schema inspection, ER diagrams from a database, database validation, or diffing two database schemas.
  Also use it when the user asks to inspect a MySQL/MariaDB/SQL Server database, generate a Mermaid ER diagram, validate foreign keys/indexes, compare two schema snapshots, run a safe read-only SQL query, or set up a `.dbctx.toml` / `.env` configuration.
  Trigger even if the user doesn't explicitly type the word `dbctx` but is clearly trying to turn a live database into JSON, Markdown, Mermaid, or validation output.
compatibility: |
  Requires the `dbctx` CLI to be installed (`cargo install dbctx` or built from source in this repository).
  Works with MySQL, MariaDB, and SQL Server databases that are reachable over TCP or through a Docker Compose service.
---

# dbctx Skill

This skill makes it easy to use the `dbctx` CLI to turn a relational database into deterministic, versioned context for humans and AI coding agents.

## What dbctx does

`dbctx` reads catalog metadata (not DDL parsing) from MySQL, MariaDB, and SQL Server and writes:

- `schema.json` — canonical machine-readable schema
- `schema.md` — human-readable documentation
- `graph.mmd` — Mermaid ER diagram (always call it a Mermaid ER diagram when the user asks about it)
- `metadata.json` — project-level statistics
- `relationships.json` — derived relationship list
- `tables/*.json` — per-table JSON documents

All output lands under `.ai/dbctx/` by default.

## When to use this skill

Use this skill when the user wants to:

- Inspect a database and generate artifacts (`dbctx inspect`)
- Validate schema health (`dbctx validate`)
- Generate an ER diagram (`dbctx graph`)
- Compare two exported schemas (`dbctx diff`)
- Show schema statistics (`dbctx stats`)
- Initialize a project (`dbctx init`)
- Run a safe read-only query (`dbctx execute-statement`)
- Emit the LLM self-documentation guide (`dbctx llm-txt`)
- Understand existing dbctx output files
- Configure `.dbctx.toml` or `.env` for dbctx

## Core principles to follow

1. **Facts first.** Default output is factual metadata. `--analyze` and `--llm` are opt-in.
2. **Read-only.** `dbctx` never modifies the database. `execute-statement` rejects any statement that could change data or schema before execution. Never list example mutating keywords such as `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, or `REPLACE` in the response unless the user specifically asks which statements are blocked; instead, say that any data-modifying or schema-changing statement is rejected.
3. **Deterministic.** The same schema produces the same artifacts (minus timestamps and generator version).
4. **Connection precedence.** CLI options > Docker Compose > `.dbctx.toml` > `.env` > environment variables > interactive prompt.

## What to do

### 1. Prefer running commands when asked

If the user asks you to run a dbctx command and the connection settings are available, run it directly with `Bash`. If connection settings are missing, ask for them or help resolve them first.

For write-like operations (`dbctx init`), confirm before overwriting an existing file unless the user explicitly asked to replace it.

### 2. Help find the connection

Look for connection sources in this order:

- CLI options (`--host`, `--port`, `--user`, `--password`, `--driver`, `--database`)
- Docker Compose (`--compose-service <service>` or `--docker-container <container>`)
- `.dbctx.toml` in the working directory
- `.env` in the working directory (`DB_CONNECTION`, `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME`, `DB_PASSWORD`)
- Environment variables
- Interactive prompt (TTY only)

When the user says something like "use the mariadb service", prefer `--compose-service mariadb`.

When the driver is omitted and a Docker container is discovered, dbctx detects it from the image. Otherwise `--driver` (or `DB_CONNECTION` / `.dbctx.toml` `driver`) is required.

### 3. Common command patterns

Inspect a local database:

```bash
dbctx inspect --host 127.0.0.1 --port 3306 --user reader --password secret --driver mysql --database shop
```

Inspect via Docker Compose:

```bash
dbctx inspect --compose-service mariadb --database shop
```

Inspect with analysis and optional AI context:

```bash
dbctx inspect --analyze
dbctx inspect --llm
```

Write to a custom directory:

```bash
dbctx inspect --output docs/database
```

Validate:

```bash
dbctx validate
```

Generate a Mermaid ER diagram:

```bash
dbctx graph --output graph.mmd
```

Compare schemas:

```bash
dbctx diff old/schema.json new/schema.json
```

Statistics:

```bash
dbctx stats
```

Run a safe read-only query:

```bash
dbctx execute-statement "SELECT COUNT(*) FROM users"
dbctx execute-statement --query "SELECT * FROM orders LIMIT 10" --timeout 10
```

Initialize a project:

```bash
dbctx init
dbctx init --force
```

Emit the LLM guide:

```bash
dbctx llm-txt
dbctx llm-txt --stdout
```

### 4. Interpret exit codes

| Code | Meaning |
|------|---------|
| 0    | Success |
| 1    | General error |
| 2    | Connection failed |
| 3    | Invalid configuration |
| 4    | Export failed |
| 5    | Validation failed |
| 6    | Unsupported database |
| 7    | Statement execution failed |
| 8    | Write operation rejected |
| 10   | Diff detected |
| 64   | Invalid CLI usage |

When a command exits non-zero, surface the meaning of the exit code and suggest the most likely fix based on the error text.

### 5. Work with generated artifacts

If the user shares or asks about `.ai/dbctx/schema.json`, `schema.md`, `graph.mmd`, `metadata.json`, or `relationships.json`, read the files with `Read` and summarize what matters for their question.

For `schema.json`, focus on tables, columns, indexes, foreign keys, and relationships. For `schema.md`, focus on the table list and relationships. For `graph.mmd`, describe the entity relationships.

### 6. Configuration guidance

Help the user create `.dbctx.toml` when they want committed, project-level connection settings. Remind them:

- `password` is not allowed in `.dbctx.toml`.
- Put secrets in `.env` (which is gitignored) or pass `--password`.
- `.dbctx.toml` ranks below CLI options and Docker Compose, but above `.env` and environment variables.

Example `.dbctx.toml`:

```toml
[dbctx]
driver = "mysql"
host = "127.0.0.1"
port = 3306
database = "shop"
user = "reader"
```

Example `.env`:

```text
DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_PORT=3306
DB_DATABASE=shop
DB_USERNAME=reader
DB_PASSWORD=secret
```

## Safety rules

- Never suggest running `execute-statement` with `INSERT`, `UPDATE`, `DELETE`, `DROP`, `CREATE`, `ALTER`, `TRUNCATE`, `MERGE`, `REPLACE`, `GRANT`, `REVOKE`, `EXEC`, or `sp_executesql`. dbctx will reject these anyway; warn the user before they try.
- Do not modify `schema.json` or other generated artifacts and claim they are canonical. They are derived from the database.
- Do not persist credentials in committed files. Remind the user to use `.env` or `--password`.

## Default behaviour

If the user just says something vague like "inspect this database" or "generate context for my database", run `dbctx inspect` with the best available connection source and report what was generated. If no connection source exists, ask for the minimum needed: host, port, user, password, driver, and database.
