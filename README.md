# dbctx

[![CI](https://github.com/btafoya/dbctx/actions/workflows/ci.yml/badge.svg)](https://github.com/btafoya/dbctx/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/dbctx.svg)](https://crates.io/crates/dbctx)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)
[![Status: Beta](https://img.shields.io/badge/status-beta-yellow.svg)](CHANGELOG.md)

> The standard way to generate accurate, structured database context for
> AI coding agents.

`dbctx` is an open-source Rust library and CLI that inspects relational
databases and produces deterministic, versioned context for developers,
CI pipelines, and AI coding tools.

**Status:** beta. The full command surface described below works against
MySQL, MariaDB, SQL Server, PostgreSQL, and SQLite. Published to
crates.io — see Installation below.

## Why dbctx?

Modern software projects increasingly rely on AI-assisted development,
yet databases remain difficult to understand automatically. Existing
tools focus on DDL exports or ER diagrams. `dbctx` focuses on producing
**trusted database context**.

**Core philosophy**

-   Facts first
-   AI is optional
-   Deterministic output
-   Human-readable artifacts
-   AI-provider agnostic
-   Stable, versioned formats

## Features

-   MySQL, MariaDB, SQL Server, PostgreSQL, and SQLite via catalog
    metadata (never by parsing SQL)
-   Docker Compose autodiscovery, `.dbctx.toml`, `.env`, and direct TCP
    connections
-   JSON, Markdown, and Mermaid ER diagram export
-   Schema validation, statistics, and diffing between two exported
    schemas
-   Optional `--analyze`: deterministic heuristics for junction tables,
    lookup tables, audit tables, and soft deletes
-   Optional `--llm`: labeled, deterministic AI-generated context
    summaries and relationship narratives — never overwrites facts
-   `dbctx llm-txt`: emits a static `LLM.md` self-documentation guide
-   `dbctx execute-statement`: read-only SQL execution, rejecting
    mutating or multi-statement queries before contacting the database
-   `dbctx mcp`: an MCP server exposing the schema to MCP clients over
    stdio or Streamable HTTP

## Installation

### Prerequisites

-   Rust 1.88 or newer (`rustup update`)

### From crates.io

```bash
cargo install dbctx
```

### From source

```bash
git clone https://github.com/btafoya/dbctx.git
cd dbctx
cargo install --path .
```

### Directly from GitHub

```bash
cargo install --git https://github.com/btafoya/dbctx.git
```

All three install the `dbctx` binary to `~/.cargo/bin`. Confirm it's on
your `PATH` with `dbctx --version`.

## Quick Start

```bash
dbctx init          # write .dbctx.toml
dbctx inspect        # write .ai/dbctx/*
dbctx inspect --analyze
dbctx inspect --llm
dbctx graph
dbctx validate
```

## Usage

### Connecting to a database

`dbctx` resolves connection settings in this order: CLI options, Docker
Compose autodiscovery, `.dbctx.toml`, `.env`, environment variables, then
an interactive prompt (TTY only). See `docs/CLI.md` for the full option list.

```bash
# Explicit connection
dbctx inspect --driver mysql --host 127.0.0.1 --port 3306 \
  --database shop --user root --password secret

# Docker Compose service
dbctx inspect --compose-service mariadb

# SQLite, with attached databases
dbctx inspect --driver sqlite --database main.db --database archive.db
```

`.dbctx.toml` (written by `dbctx init`):

```toml
[dbctx]
driver = "mysql"
host = "127.0.0.1"
port = 3306
database = "shop"
user = "root"
```

### Commands

| Command | Purpose |
|---|---|
| `dbctx init` | Write a `.dbctx.toml` connection file |
| `dbctx inspect` | Inspect a database and write JSON, Markdown, and Mermaid artifacts |
| `dbctx graph` | Generate a Mermaid ER diagram |
| `dbctx validate` | Run deterministic validation rules against the schema |
| `dbctx stats` | Print schema statistics (tables, columns, indexes, foreign keys) |
| `dbctx diff` | Compare two exported `schema.json` documents |
| `dbctx llm-txt` | Emit the static `LLM.md` self-documentation guide |
| `dbctx execute-statement` | Run a single read-only SQL statement and print JSON |
| `dbctx mcp` | Serve the schema to MCP clients over stdio or Streamable HTTP |

Full options, exit codes, and examples are documented in `docs/CLI.md`.

### MCP server

```bash
dbctx mcp                  # stdio transport
dbctx mcp --sse-port 8080  # Streamable HTTP transport
```

Exposes resources (`dbctx://schema`, `dbctx://metadata`,
`dbctx://graph`, `dbctx://relationships`, `dbctx://tables/<schema>.<table>`),
tools (`execute-statement`, `refresh-schema`), and prompts
(`summarize-schema`, `describe-table`, `explain-relationships`).

## Generated Output

```text
.ai/dbctx/
├── schema.json
├── schema.md
├── metadata.json
├── relationships.json
├── graph.mmd
└── tables/
```

## Design Principles

1.  Facts before inference
2.  Human-readable artifacts
3.  Reproducible output
4.  Versioned document formats
5.  Library-first architecture
6.  AI-provider neutrality

## Documentation

-   [VISION.md](docs/VISION.md) — mission and non-goals
-   [SPEC.md](docs/SPEC.md) — behavior contract
-   [ARCHITECTURE.md](docs/ARCHITECTURE.md) — layers and module layout
-   [FORMAT.md](docs/FORMAT.md) — output document formats
-   [CLI.md](docs/CLI.md) — full command reference
-   [ROADMAP.md](docs/ROADMAP.md) — release scope
-   [TESTING.md](docs/TESTING.md) — test strategy and CI gates
-   [CHANGELOG.md](CHANGELOG.md) — release history
-   [CONTRIBUTING.md](CONTRIBUTING.md) — PR workflow and review criteria

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Significant changes go through an
RFC before implementation.

## Releasing

Maintainers cut a release with `scripts/release.sh`:

```bash
scripts/release.sh 1.0.0
```

**Requirements:**

-   A clean working tree on `main`
-   A crates.io API token installed via `cargo login` (get one from
    <https://crates.io/settings/tokens>)

**What it does:**

1.  Bumps the version in `Cargo.toml`
2.  Cuts `CHANGELOG.md`: renames `## [Unreleased]` to `## [<version>] - <date>`
    and adds a fresh empty `## [Unreleased]` above it
3.  Runs the same gates as CI: `cargo fmt --check`, `cargo clippy --all-features
    -D warnings`, `cargo nextest run --all-features`, `cargo test --doc`
4.  Commits and tags `v<version>`
5.  Runs `cargo publish --dry-run`, then pauses for confirmation before the
    irreversible `cargo publish` and `git push`

Answering "no" at that last prompt leaves the commit and tag in place
locally without publishing or pushing anything; the script prints the
commands to undo them.

## License

Dual licensed under **MIT OR Apache-2.0**. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
