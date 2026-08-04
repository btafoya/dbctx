# dbctx

> The standard way to generate accurate, structured database context for
> AI coding agents.

`dbctx` is an open-source Rust library and CLI that inspects relational
databases and produces deterministic, versioned context for developers,
CI pipelines, and AI coding tools.

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

## Phase 1 Features

-   MySQL, MariaDB & SQL Server via catalog metadata
-   Docker Compose autodiscovery
-   `.env` support
-   Direct TCP connections
-   JSON export
-   Markdown documentation
-   Mermaid ER diagrams
-   Schema validation
-   Schema diff
-   Database statistics
-   Optional `--analyze`
-   Optional `--llm`

## Quick Start

``` bash
cargo install dbctx

dbctx inspect
dbctx inspect --analyze
dbctx inspect --llm
dbctx graph
dbctx validate
```

## Generated Output

``` text
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
2.  Reproducible output
3.  Versioned document formats
4.  Library-first architecture
5.  AI-provider neutrality

## Documentation

-   VISION.md
-   SPEC.md
-   docs/ARCHITECTURE.md
-   docs/FORMAT.md
-   docs/CLI.md
-   docs/ROADMAP.md
-   docs/TESTING.md
-   CONTRIBUTING.md

## Roadmap

### Phase 1

Core inspection engine, CLI, JSON, Markdown, Mermaid, validation,
statistics, diffing, optional AI context.

### Phase 2

PostgreSQL, SQLite, additional exporters.

### Phase 3

Template system, plugin architecture, IDE integrations, MCP resources,
RAG-friendly exports.

## License

Dual licensed under **MIT OR Apache-2.0**.
