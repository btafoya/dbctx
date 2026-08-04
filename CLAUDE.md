# CLAUDE.md

# dbctx Implementation Guide for AI Coding Agents

This document provides implementation guidance for AI coding agents
(Claude Code, Codex CLI, Cursor, Aider, and similar tools). It
complements `SPEC.md` by describing the recommended implementation
sequence and engineering constraints.

> **This document is implementation guidance, not the specification.**
> If guidance here conflicts with `SPEC.md`, the specification wins.

------------------------------------------------------------------------

# Project Priorities

1.  Correctness
2.  Determinism
3.  Stable public APIs
4.  Stable output formats
5.  Testability
6.  Performance
7.  Developer experience

Never sacrifice correctness for convenience.

------------------------------------------------------------------------

# Core Principles

-   Facts before inference.
-   AI features are optional.
-   The canonical schema model is the single source of truth.
-   Exporters never query databases.
-   Validation never mutates schema data.
-   Analysis never mutates factual metadata.
-   AI never overwrites facts.

------------------------------------------------------------------------

# Recommended Implementation Order

## Phase 0 -- Repository Foundation

-   Initialize Cargo project
-   Configure rustfmt and clippy
-   Create CI workflow
-   Configure cargo-nextest
-   Add LICENSE-MIT and LICENSE-APACHE
-   Add repository documentation

Definition of Done: - CI passes on every commit.

------------------------------------------------------------------------

## Phase 1 -- Core Data Model

Implement the canonical schema model.

Suggested modules:

``` text
src/model/
    database.rs
    table.rs
    column.rs
    index.rs
    foreign_key.rs
    metadata.rs
```

Keep models free of database-specific logic.

Definition of Done: - Model serializes with Serde. - Unit tests cover
all model types.

------------------------------------------------------------------------

## Phase 2 -- Configuration

Implement:

-   CLI parsing
-   .env loading
-   environment variables
-   configuration merging

Configuration precedence:

1.  CLI
2.  Docker Compose discovery
3.  .env
4.  Environment variables

Definition of Done: - Configuration is immutable after construction.

------------------------------------------------------------------------

## Phase 3 -- Connection Discovery

Implement:

-   explicit TCP
-   Docker Compose autodiscovery
-   database detection

No schema inspection yet.

Definition of Done: - Connection parameters resolve correctly.

------------------------------------------------------------------------

## Phase 4 -- Database Introspection

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

Definition of Done: - Integration tests against MySQL, MariaDB and SQL
Server.

------------------------------------------------------------------------

## Phase 5 -- JSON Export

Implement first exporter.

Everything else should be based on this implementation.

Definition of Done:

-   deterministic ordering
-   valid JSON
-   schema validation
-   snapshot tests

------------------------------------------------------------------------

## Phase 6 -- Markdown Export

Generate concise human-readable documentation.

No AI summaries.

Definition of Done: - Stable markdown snapshots.

------------------------------------------------------------------------

## Phase 7 -- Mermaid Export

Generate deterministic ER diagrams.

Definition of Done: - Valid Mermaid syntax. - Snapshot tests.

------------------------------------------------------------------------

## Phase 8 -- Validation Engine

Rules only.

No automatic fixes.

Each validation rule requires:

-   positive test
-   negative test
-   edge case

------------------------------------------------------------------------

## Phase 9 -- Statistics

Implement database metrics.

Must not re-query the database.

Everything derives from the canonical model.

------------------------------------------------------------------------

## Phase 10 -- Diff Engine

Compare two schema models.

Comparison occurs on exported artifacts rather than live databases.

------------------------------------------------------------------------

## Phase 11 -- Analysis

Deterministic heuristics only.

Examples:

-   junction tables
-   lookup tables
-   audit tables
-   soft deletes

No LLM usage.

------------------------------------------------------------------------

## Phase 12 -- LLM Mode

Adds optional:

-   summaries
-   narratives
-   context

Every generated section must be labeled.

Facts remain unchanged.

------------------------------------------------------------------------

# Module Dependency Rules

Allowed:

CLI → Config → Discovery → Database → Model → Validation → Analysis → AI
→ Exporters

Forbidden:

-   exporter → database
-   AI → database
-   validation → exporter
-   analysis → exporter
-   circular dependencies

------------------------------------------------------------------------

# Coding Standards

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

------------------------------------------------------------------------

# Error Handling

Library:

-   thiserror

CLI:

-   anyhow

Never panic for recoverable failures.

Every user-facing error should explain:

-   what failed
-   why
-   suggested resolution

------------------------------------------------------------------------

# Performance Targets

-   Startup \<250 ms
-   500-table schema \<5 s
-   Stable ordering
-   Minimal allocations where practical

Optimize only after correctness.

------------------------------------------------------------------------

# Testing Expectations

Every feature requires:

-   unit tests
-   integration tests
-   snapshot tests (when applicable)

Bug fixes require regression tests.

------------------------------------------------------------------------

# Definition of Done

A feature is complete only when:

-   implemented
-   tested
-   documented
-   snapshot updated
-   CI passes
-   specification updated (if needed)

------------------------------------------------------------------------

# AI Agent Guidance

When implementing code:

1.  Read VISION.md
2.  Read SPEC.md
3.  Read ARCHITECTURE.md
4.  Read FORMAT.md
5.  Read CLI.md
6.  Implement the smallest complete unit.
7.  Add tests before moving to the next feature.

Do not skip ahead because later phases depend on earlier architectural
guarantees.

------------------------------------------------------------------------

# Non-Goals

Do not implement:

-   plugin systems
-   template engines
-   PostgreSQL
-   SQLite
-   network services

unless specifically requested by an accepted RFC or updated
specification.

------------------------------------------------------------------------


## Rules

- Always fully complete the task.
- Never create stubs.
- Always build for production use.
- Always follow the `Implementation Loop` below.
- Apply the `ponytail` skill: prefer deletion over addition, reuse existing code,
  prefer stdlib/native/installed dependencies, and question whether speculative
  features need to exist at all.

## Claude Code Behaviour Guidelines

- Avoid ownership-dodging behaviour: if you encounter an issue, take responsibility for it and work towards a solution instead of passing it on to someone else. Don't say things like "not caused by my changes" or say that it's "a pre-existing issue". Instead, acknowledge the problem and take initiative to fix it. Also, don't give up with excuses like "known limitation" and don't mark it for "future work".
- Avoid premature stopping: if you encounter a problem, don't stop at the first obstacle. Instead, keep pushing forward and find a way to overcome it. Don't say things like "good stopping point" or "natural checkpoint". Instead, keep going until you have a complete solution.
- Avoid permission-seeking behaviour: if you have the knowledge and capability to solve a problem, push through. Don't say things like "should I continue?" or "want me to keep going?". Instead, take initiative and act towards the solution.
- Do plan multi-step approaches before acting (plan which files to read and in what order, which tools to use, etc).
- Do recall and apply project-specific conventions from CLAUDE.md files.
- Do catch your own mistakes by applying reasoning loops and self-checks, and fix them before committing or asking for help.

### Use of tools

Adhere to the following guidelines when using tools:

- Always use a **Research-First approach**: Before using any tool, conduct thorough research to understand the context and requirements. This ensures that you use the most appropriate tool for the task at hand. Never use an Edit-First approach. You should prefer making surgical edits to the codebase instead of rewriting whole files or doing large, sweeping changes.
- Use **Reasoning Loops** very frequently. Don't be lazy and skip them. Reasoning loops are essential for ensuring the quality and accuracy of your work.

## CodeGraph and MCP Tooling

Use the [CodeGraph MCP server](https://colbymchenry.github.io/codegraph/getting-started/introduction/)
for structural questions. Prefer `codegraph_explore` over `grep` or chained `Read`
calls; trust its AST-parsed results. Use other configured MCP servers when they
provide a dedicated tool for the task.


# Final Rule

Favor long-term maintainability over short-term convenience.

Every architectural decision should preserve the project's core promise:

**Generate accurate, deterministic database context that developers and
AI coding agents can trust.**
