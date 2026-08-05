# ROADMAP.md

# dbctx Roadmap

## v0.1 -- Foundation

-   Rust library + CLI
-   MySQL/MariaDB/SQL Server support
-   Catalog metadata introspection
-   JSON, Markdown, Mermaid exporters
-   Validation, stats, diff
-   Optional --analyze and --llm
-   Docker Compose autodiscovery
-   Stable format v1.0

## v0.2

-   Performance improvements
-   Better diagnostics
-   More validation rules
-   Configuration file

## v0.3

-   PostgreSQL
-   SQLite, including attached databases
-   `attributes` extension on the canonical model for engine-specific
    facts
-   MCP server (`dbctx mcp`), stdio and Streamable HTTP transports

## v1.0

-   Stable public API
-   Stable format 1.x
-   Production-ready documentation
-   LLM self-documentation command (`llm-txt`) and safe read-only SQL
    execution (`execute-statement`)

## Future

-   Template engine
-   Plugin architecture
-   IDE integrations
-   RAG exports
-   Migrating MySQL, MariaDB and SQL Server to `sqlx`

## Release Policy

-   Semantic Versioning
-   RFCs for major features
-   ADRs for architecture
-   Format versions evolve independently.
