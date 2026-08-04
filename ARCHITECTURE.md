# ARCHITECTURE.md

# dbctx Architecture

**Status:** Draft (Phase 1)

## Purpose

This document defines the architectural boundaries, module
responsibilities, dependency rules, and data flow for `dbctx`.

The architecture is designed around a single principle:

> **Facts → Analysis → AI**

Each layer depends only on lower layers. Higher layers may enrich data
but must never modify factual metadata.

The `execute-statement` command (v1.0) is a narrow exception: it issues
a read-only SQL query through the connection layer and returns raw
results directly. It never writes to the database and never feeds its
output into the canonical schema model.

------------------------------------------------------------------------

# High-Level Architecture

``` text
                  +----------------------+
                  |      CLI (clap)      |
                  +----------+-----------+
                             |
                             v
                  +----------------------+
                  |    dbctx Library     |
                  +----------+-----------+
                             |
    +------------------------+------------------------+
    |                        |                        |
    v                        v                        v
Configuration         Connection Discovery     Export Pipeline
    |                        |                        |
    +-------------+----------+------------------------+
                  |
                  v
          Database Introspection
                  |
                  v
           Canonical Schema Model
                  |
      +-----------+-----------+
      |                       |
      v                       v
 Validation             Analysis (optional)
                              |
                              v
                         AI Context (optional)
                              |
                              v
                      Exporters (JSON/MD/Mermaid)
```

------------------------------------------------------------------------

# Architectural Layers

## Layer 1 -- Configuration

Responsibilities:

-   CLI parsing
-   `.env`
-   Environment variables
-   Config file (`.dbctx.toml`)

Produces immutable runtime configuration.

------------------------------------------------------------------------

## Layer 2 -- Connection Discovery

Determines how to connect.

Priority:

1.  CLI
2.  Docker Compose
3.  `.dbctx.toml`
4.  `.env`
5.  Environment
6.  Interactive
7.  Fail

No schema logic exists here.

------------------------------------------------------------------------

## Layer 2.5 -- Read-only Execution (v1.0)

A thin, isolated path used only by `execute-statement`.

-   Accepts a single SQL statement from the CLI.
-   Validates that the statement is read-only before it reaches the
    database.
-   Executes through the same connection layer as introspection.
-   Returns tabular results directly to the user or to an exporter.
-   **Must never mutate the canonical schema model.**

This layer is deliberately separate from introspection so that ad-hoc
queries cannot accidentally become part of the factual record.

------------------------------------------------------------------------

## Layer 3 -- Introspection

Reads catalog metadata. INFORMATION_SCHEMA is the primary source on every
engine; native catalog views (`sys.*` on SQL Server) supply facts
INFORMATION_SCHEMA does not expose. See SPEC.md §7.

Responsibilities:

-   tables
-   columns
-   indexes
-   foreign keys
-   views

Never performs analysis.

------------------------------------------------------------------------

## Layer 4 -- Canonical Schema Model

This is the heart of the project.

Every exporter consumes this model.

Every analysis consumes this model.

No exporter reads database catalogs directly.

------------------------------------------------------------------------

## Layer 5 -- Validation

Pure rule engine.

Examples:

-   missing PK
-   duplicate indexes
-   circular FK
-   invalid metadata

Produces findings.

Never changes the schema.

------------------------------------------------------------------------

## Layer 6 -- Analysis

Optional deterministic heuristics.

Examples:

-   junction tables
-   lookup tables
-   audit tables
-   soft deletes
-   timestamp conventions

Produces additional metadata.

------------------------------------------------------------------------

## Layer 7 -- AI

Optional.

Consumes:

-   schema
-   validation
-   analysis

Produces:

-   summaries
-   narratives
-   entry-point suggestions

Cannot alter facts.

------------------------------------------------------------------------

## Layer 8 -- Exporters

Phase 1:

-   JSON
-   Markdown
-   Mermaid
-   Metadata

All exporters consume only the canonical model.

------------------------------------------------------------------------

# Module Layout

``` text
src/
├── main.rs
├── lib.rs
├── cli.rs
├── config.rs
├── discovery/
├── database/
│   ├── mysql.rs
│   ├── mariadb.rs
│   ├── sqlserver.rs
│   └── queries.rs
├── model.rs
├── validation/
├── analysis/
├── ai/
├── exporters/
├── util/
└── error.rs
```

------------------------------------------------------------------------

# Public API

The public library should remain intentionally small.

Example:

``` rust
let config = Config::discover()?;
let schema = dbctx::inspect(config)?;
dbctx::export(schema)?;
```

Implementation details remain internal.

------------------------------------------------------------------------

# Dependency Rules

Allowed:

    CLI
     ↓
    Config
     ↓
    Discovery
     ↓
    Database
     ↓
    Model
     ↓
    Validation
     ↓
    Analysis
     ↓
    AI
     ↓
    Exporters

Forbidden:

-   Exporters querying databases
-   AI querying databases
-   Validation modifying schema
-   Analysis modifying facts
-   Circular module dependencies

------------------------------------------------------------------------

# Canonical Schema Model

``` text
Database
├── Metadata
├── Tables (schema-qualified)
│   ├── Columns
│   ├── Indexes
│   ├── Foreign Keys
│   └── Constraints
├── Views
└── Relationships
```

Every output format derives from this model.

------------------------------------------------------------------------

# Error Architecture

Library:

-   typed errors

CLI:

-   formatted diagnostics
-   exit codes

No panics for recoverable failures.

------------------------------------------------------------------------

# Concurrency

Phase 1 should support parallel metadata collection where beneficial
while preserving deterministic ordering in the final model.

Ordering must be stable regardless of execution order.

------------------------------------------------------------------------

# Testing Strategy

Each architectural layer has independent tests.

-   Configuration tests
-   Discovery tests
-   Introspection tests
-   Model tests
-   Validation tests
-   Analysis tests
-   Export snapshot tests

End-to-end Docker integration tests validate supported database
versions.

------------------------------------------------------------------------

# Extension Strategy

Future databases implement the same introspection interface and populate
the canonical model.

Future exporters consume the canonical model without requiring database
changes.

------------------------------------------------------------------------

# Architecture Decision Records

Significant architectural changes require an ADR or RFC before
implementation.

Examples:

-   new database backend
-   new document format
-   plugin system
-   breaking schema model changes

------------------------------------------------------------------------

# Invariants

These rules must never be violated:

1.  Facts are immutable.
2.  Analysis is deterministic.
3.  AI is optional.
4.  Exporters are read-only.
5.  Canonical model is the single source of truth.
6.  Output formats are versioned.
7.  Stable ordering is preserved.

These invariants are the foundation of dbctx's long-term
maintainability.
