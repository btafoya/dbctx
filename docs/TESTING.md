# TESTING.md

# dbctx Testing Strategy

**Version:** 1.0

## Purpose

This document defines the testing philosophy, quality gates, and
validation strategy for `dbctx`.

Testing is a first-class feature. Every public behavior described in
`SPEC.md`, `FORMAT.md`, and `CLI.md` must be verifiable through
automated tests.

------------------------------------------------------------------------

# Testing Principles

1.  Test behavior, not implementation.
2.  Prefer deterministic tests.
3.  Every bug fix requires a regression test.
4.  Snapshot changes must be reviewed.
5.  CI must reproduce local results.

------------------------------------------------------------------------

# Testing Pyramid

``` text
            End-to-End
         Integration Tests
            Snapshot Tests
              Unit Tests
```

Target distribution:

-   Unit: \~70%
-   Integration: \~20%
-   End-to-end: \~10%

------------------------------------------------------------------------

# Unit Tests

Location:

``` text
src/**/tests.rs
tests/unit/
```

Coverage includes:

-   Configuration parsing
-   Connection resolution
-   Schema model
-   Validation rules
-   Analysis rules
-   Export helpers
-   CLI argument parsing

No external services required.

------------------------------------------------------------------------

# Integration Tests

Location:

``` text
tests/integration/
```

Use disposable databases.

Verify:

-   Catalog metadata queries
-   Schema model generation
-   JSON export
-   Markdown export
-   Mermaid export
-   Error handling

## MCP Server

`dbctx mcp` was verified manually against a real SQLite database: a full
JSON-RPC session over stdio (`initialize`, `resources/list`,
`resources/read` for `dbctx://schema` and `dbctx://graph`, `tools/list`,
`tools/call execute-statement` returning real rows, `tools/call
refresh-schema`, a mutating statement correctly rejected as an
`isError: true` tool result, `prompts/get summarize-schema`), and a `curl`
request against the `--sse-port` HTTP transport. Automated `tests/mcp.rs`
coverage spawning `dbctx mcp` as a subprocess is tracked separately.

------------------------------------------------------------------------

# Docker Test Matrix

Every supported database version should be tested in CI.

Phase 1:

## MySQL

-   8.0
-   8.4 LTS

## MariaDB

-   10.11 LTS
-   11.x current

## SQL Server

-   2019
-   2022

## PostgreSQL

-   14
-   15
-   16
-   17

## SQLite

No Docker container: SQLite is a file, not a service. Integration tests
create temporary `.db` files (including attached databases) and run
against them directly, the same way unit tests do.

Future phases expand this matrix.

------------------------------------------------------------------------

# Golden Tests

Generated artifacts are compared against committed reference files.

``` text
testdata/
├── mysql-basic/
│   ├── schema.json
│   ├── schema.md
│   ├── graph.mmd
│   └── metadata.json
├── mariadb-commerce/
├── sqlserver-basic/
├── sqlserver-multischema/
└── ...
```

Unexpected differences fail CI.

`sqlserver-multischema` covers tables of the same name in two schemas,
proving file naming and relationship references stay unambiguous.

------------------------------------------------------------------------

# Snapshot Testing

Recommended crate:

-   insta

Snapshot:

-   JSON
-   Markdown
-   Mermaid
-   Validation output

Snapshot updates require explicit review.

------------------------------------------------------------------------

# CLI Tests

Verify:

-   Help output
-   Version output
-   Exit codes
-   Invalid arguments
-   Error formatting
-   Verbosity
-   `dbctx llm-txt` produces the LLM guide
-   `dbctx execute-statement` runs a `SELECT` and emits JSON
-   `dbctx execute-statement` rejects mutating SQL with the documented
    exit code

All documented examples should execute successfully.

------------------------------------------------------------------------

# Performance Tests

Track:

-   Startup time
-   Inspection duration
-   Export duration
-   Memory usage

Performance regressions should be reported in CI.

Target:

-   500-table schema under 5 seconds on commodity hardware.

------------------------------------------------------------------------

# Determinism Tests

Running the same command twice against an unchanged schema must produce
identical output except for explicitly permitted metadata (timestamps,
generator version).

Ordering must remain stable regardless of concurrency.

------------------------------------------------------------------------

# Validation Tests

Every validation rule requires:

-   Positive case
-   Negative case
-   Edge case

Rules must never modify schema metadata.

------------------------------------------------------------------------

# Analysis Tests

Analysis is deterministic.

Every heuristic must include tests proving:

-   Detection
-   Non-detection
-   Confidence calculation (if applicable)

No LLMs are involved.

------------------------------------------------------------------------

# LLM Tests

AI output is optional.

Verify:

-   Clearly labeled
-   Facts preserved
-   AI sections removable
-   Stable document structure

Tests should not depend on external AI services.

------------------------------------------------------------------------

# Compatibility Tests

Every supported output format validates against its published JSON
Schema.

Older format versions remain readable where compatibility is guaranteed.

------------------------------------------------------------------------

# Security Tests

Verify:

-   Read-only database access
-   `execute-statement` rejects `INSERT`, `UPDATE`, `DELETE`, `DROP`,
    `ALTER`, `CREATE`, `TRUNCATE`, and `MERGE` before execution
-   `execute-statement` output never modifies the canonical schema model
-   Credentials never written to output
-   No unexpected outbound network traffic
-   Sensitive values omitted from logs

------------------------------------------------------------------------

# Fuzz Testing

Future:

-   Malformed metadata
-   Invalid identifiers
-   Large schemas
-   Unicode names
-   Edge-case comments

------------------------------------------------------------------------

# Continuous Integration

Required checks:

-   cargo fmt --check
-   cargo clippy
-   cargo test
-   cargo nextest
-   Snapshot verification
-   Docker integration matrix
-   JSON Schema validation
-   Documentation build
-   MSRV build

No release may proceed with failing checks.

------------------------------------------------------------------------

# Code Coverage

Coverage is a quality signal, not the goal.

Target:

-   Core library: ≥90%
-   Exporters: ≥90%
-   Validation: 100%
-   Analysis: ≥95%

Critical paths must have exhaustive tests.

------------------------------------------------------------------------

# Regression Policy

Every resolved defect must include:

1.  A failing test demonstrating the bug.
2.  A fix.
3.  A passing regression test.

------------------------------------------------------------------------

# Release Criteria

Before release:

-   All CI green
-   No snapshot drift
-   Performance targets met
-   Documentation updated
-   Format compatibility verified
-   CHANGELOG completed

------------------------------------------------------------------------

# Testing Philosophy

The published specification defines the expected behavior.

Tests verify conformance to the specification.

The implementation may evolve, but observable behavior must remain
stable unless explicitly changed through the project's RFC and
versioning processes.
