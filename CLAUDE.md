# dbctx Implementation Guide for AI Coding Agents

This document provides implementation guidance for AI coding agents
(Claude Code, Codex CLI, Cursor, Aider, and similar tools). It
complements `SPEC.md` by describing the recommended implementation
sequence and engineering constraints.

> **This document is implementation guidance, not the specification.**
> If guidance here conflicts with `SPEC.md`, the specification wins.

------------------------------------------------------------------------

## Repository Status

**Phase 3 is complete. Phase 4 -- Database Introspection is next.**

`src/model.rs` holds the canonical schema model with its deterministic
ordering, `src/cli.rs` the full command surface from `CLI.md`,
`src/config.rs` the connection settings and their precedence, and
`src/discovery.rs` the Docker and prompt sources. A connection resolves
completely, engine included; nothing opens one yet. Every command parses and
resolves its configuration; only `init` does its work, the rest exit 1 until
the phases behind them land.

-   Anything not yet implemented is defined only in the specification
    documents. Read those rather than inferring intent from `src/`.
-   CI runs fmt, clippy, docs, nextest and an MSRV check on every push. The
    Docker database matrix and JSON Schema validation join it at Phase 4 and
    Phase 5.

------------------------------------------------------------------------

## Document Map

Read in this order before writing code. Each document is authoritative
for its own subject:

| Document | Authoritative for |
|---|---|
| `VISION.md` | Why the project exists; what it refuses to become |
| `SPEC.md` | Behavior contract. Wins every conflict |
| `ARCHITECTURE.md` | Layers, module layout, dependency rules, invariants |
| `FORMAT.md` | Output document formats, fields, ordering, versioning |
| `CLI.md` | Command names, options, exit codes, stability policy |
| `TESTING.md` | Test strategy, Docker matrix, golden files, CI gates |
| `ROADMAP.md` | Release scope |
| `CONTRIBUTING.md` | PR workflow, review criteria, commit style |
| `ADR_README.md` / `RFC_README.md` | How to change any of the above |

Then: implement the smallest complete unit, and add tests before moving
to the next feature.

Do not skip ahead because later phases depend on earlier architectural
guarantees.

------------------------------------------------------------------------

## Commands

The gates required by `CONTRIBUTING.md` and `TESTING.md`. All must pass
before a commit, and CI runs the same set.

``` bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings   # warnings are denied
cargo nextest run
cargo test                                  # doc tests
cargo insta review                          # after intentional snapshot changes
```

Integration databases (see the matrix in `TESTING.md`):

``` text
mysql:8.0, mysql:8.4
mariadb:10.11, mariadb:11
mcr.microsoft.com/mssql/server:2019-latest, :2022-latest
```

------------------------------------------------------------------------

## Project Priorities

1.  Correctness
2.  Determinism
3.  Stable public APIs
4.  Stable output formats
5.  Testability
6.  Performance
7.  Developer experience

Never sacrifice correctness for convenience.

------------------------------------------------------------------------

## Core Principles

-   Facts before inference.
-   AI features are optional.
-   The canonical schema model is the single source of truth.
-   Exporters never query databases.
-   Validation never mutates schema data.
-   Analysis never mutates factual metadata.
-   AI never overwrites facts.

------------------------------------------------------------------------

## Recommended Implementation Order

### Phase 0 -- Repository Foundation

-   Initialize Cargo project
-   Configure rustfmt and clippy
-   Create CI workflow
-   Configure cargo-nextest
-   Add LICENSE-MIT and LICENSE-APACHE
-   Add repository documentation

**Done when:** CI passes on every commit.

### Phase 1 -- Core Data Model

Implement the canonical schema model.

One module, `src/model.rs`, holding every model type. The types are plain
data with a single impl block; splitting them across files buys re-exports
and nothing else.

Keep models free of database-specific logic.

**Done when:** model serializes with Serde; unit tests cover all model
types.

### Phase 2 -- Configuration

Implement:

-   CLI parsing
-   .env loading
-   environment variables
-   configuration merging

Configuration precedence:

1.  CLI
2.  Docker Compose discovery
3.  `.dbctx.toml`
4.  .env
5.  Environment variables

**Done when:** configuration is immutable after construction.

### Phase 3 -- Connection Discovery

Implement:

-   explicit TCP
-   Docker Compose autodiscovery
-   database detection

No schema inspection yet.

**Done when:** connection parameters resolve correctly.

### Phase 4 -- Database Introspection

Read catalog metadata. INFORMATION_SCHEMA first, native catalog views
(`sys.*` on SQL Server) only for facts it does not expose. See SPEC.md §7.

Never parse SQL.

Implement:

-   tables
-   columns
-   indexes
-   foreign keys
-   views

Populate only the canonical model.

**Done when:** integration tests pass against MySQL, MariaDB and SQL
Server.

### Phase 5 -- JSON Export

Implement first exporter. Everything else should be based on this
implementation.

**Done when:**

-   deterministic ordering
-   valid JSON
-   schema validation
-   snapshot tests

### Phase 6 -- Markdown Export

Generate concise human-readable documentation.

No AI summaries.

**Done when:** markdown snapshots are stable.

### Phase 7 -- Mermaid Export

Generate deterministic ER diagrams.

**Done when:** Mermaid syntax is valid and snapshot tests pass.

### Phase 8 -- Validation Engine

Rules only.

No automatic fixes.

Each validation rule requires:

-   positive test
-   negative test
-   edge case

### Phase 9 -- Statistics

Implement database metrics.

Must not re-query the database.

Everything derives from the canonical model.

### Phase 10 -- Diff Engine

Compare two schema models.

Comparison occurs on exported artifacts rather than live databases.

### Phase 11 -- Analysis

Deterministic heuristics only.

Examples:

-   junction tables
-   lookup tables
-   audit tables
-   soft deletes

No LLM usage.

### Phase 12 -- LLM Mode

Adds optional:

-   summaries
-   narratives
-   context

Every generated section must be labeled.

Facts remain unchanged.

------------------------------------------------------------------------

## Architecture Rules

### Module Dependencies

Allowed:

CLI → Config → Discovery → Database → Model → Validation → Analysis → AI
→ Exporters

Forbidden:

-   exporter → database
-   AI → database
-   validation → exporter
-   analysis → exporter
-   circular dependencies

### Coding Standards

Prefer:

-   explicit types
-   small modules
-   descriptive names
-   exhaustive enums
-   immutable data

Avoid:

-   global state
-   hidden side effects
-   unnecessary traits
-   premature abstraction

### Error Handling

Library:

-   thiserror

CLI:

-   anyhow

Never panic for recoverable failures.

Every user-facing error should explain:

-   what failed
-   why
-   suggested resolution

### Performance Targets

-   Startup \<250 ms
-   500-table schema \<5 s
-   Stable ordering
-   Minimal allocations where practical

Optimize only after correctness.

------------------------------------------------------------------------

## Testing and Definition of Done

Every feature requires:

-   unit tests
-   integration tests
-   snapshot tests (when applicable)

Bug fixes require regression tests.

A feature is complete only when:

-   implemented
-   tested
-   documented
-   snapshot updated
-   CI passes
-   specification updated (if needed)

------------------------------------------------------------------------

## Working Agreement

### Rules

-   Always fully complete the task.
-   Never create stubs.
-   Always build for production use.
-   Apply the `ponytail` skill: prefer deletion over addition, reuse
    existing code, prefer stdlib/native/installed dependencies, and
    question whether speculative features need to exist at all.

### Behaviour

-   Avoid ownership-dodging behaviour: if you encounter an issue, take
    responsibility for it and work towards a solution instead of passing
    it on to someone else. Don't say things like "not caused by my
    changes" or say that it's "a pre-existing issue". Instead,
    acknowledge the problem and take initiative to fix it. Also, don't
    give up with excuses like "known limitation" and don't mark it for
    "future work".
-   Avoid premature stopping: if you encounter a problem, don't stop at
    the first obstacle. Instead, keep pushing forward and find a way to
    overcome it. Don't say things like "good stopping point" or "natural
    checkpoint". Instead, keep going until you have a complete solution.
-   Avoid permission-seeking behaviour: if you have the knowledge and
    capability to solve a problem, push through. Don't say things like
    "should I continue?" or "want me to keep going?". Instead, take
    initiative and act towards the solution.
-   Do plan multi-step approaches before acting (plan which files to read
    and in what order, which tools to use, etc).
-   Do recall and apply project-specific conventions from CLAUDE.md
    files.
-   Do catch your own mistakes by applying reasoning loops and
    self-checks, and fix them before committing or asking for help.

### Use of Tools

Adhere to the following guidelines when using tools:

-   Always use a **Research-First approach**: before using any tool,
    conduct thorough research to understand the context and
    requirements. This ensures that you use the most appropriate tool
    for the task at hand. Never use an Edit-First approach. You should
    prefer making surgical edits to the codebase instead of rewriting
    whole files or doing large, sweeping changes.
-   Use the [CodeGraph MCP server](https://colbymchenry.github.io/codegraph/getting-started/introduction/)
    for structural questions once `src/` exists. Prefer
    `codegraph_explore` over `grep` or chained `Read` calls; trust its
    AST-parsed results. Use other configured MCP servers when they
    provide a dedicated tool for the task.

------------------------------------------------------------------------

## Non-Goals

Do not implement:

-   plugin systems
-   template engines
-   PostgreSQL
-   SQLite
-   network services

unless specifically requested by an accepted RFC or updated
specification.

------------------------------------------------------------------------

## Final Rule

Favor long-term maintainability over short-term convenience.

Every architectural decision should preserve the project's core promise:

**Generate accurate, deterministic database context that developers and
AI coding agents can trust.**
