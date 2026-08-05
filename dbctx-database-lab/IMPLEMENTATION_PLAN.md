# dbctx Database Lab Implementation Plan

## Stage 1: Docker Compose Foundation
**Goal**: Complete `docker-compose.yml`, `.env.example`, and `Makefile`.
**Success Criteria**:
- All database services (PostgreSQL, MariaDB, MySQL, SQL Server, SQLite) are defined with named containers, volumes, network, health checks, restart policies, logging, init, stop_grace_period, ulimits, security options, and read-only bind mounts where appropriate.
- Adminer, pgAdmin, and phpMyAdmin services exist with `ui` and `admin` profiles.
- `.env.example` exposes every configurable value.
- Makefile implements all required targets.

## Stage 2: Database Schemas and Seed Scripts
**Goal**: Replace placeholder init SQL with full, engine-specific schemas and deterministic seed data.
**Success Criteria**:
- Every engine creates users, companies, products, orders, order_items with primary keys, foreign keys, indexes, views, and stored procedures/functions where supported.
- Logical data is equivalent across engines.

## Stage 3: Lab Utility Scripts
**Goal**: Implement Bash scripts for `up`, `down`, `wait`, `verify`, `benchmark`, and `reset`.
**Success Criteria**:
- All scripts use `set -euo pipefail` and pass ShellCheck.
- `verify.sh` checks connectivity, auth, schema, CRUD, transactions, prepared statements, Unicode, binary/blob, timestamps, indexes, views, and stored procedures.
- `benchmark.sh` produces `benchmark.md` with latency and throughput metrics.

## Stage 4: Deterministic Datasets
**Goal**: Provide reusable small, medium, and large seed datasets.
**Success Criteria**:
- `datasets/{small,medium,large}/` contain CSV/JSON seed files and loading instructions.

## Stage 5: TLS Certificate Support
**Goal**: Enable optional TLS for all engines.
**Success Criteria**:
- Development certificates are auto-generated if absent.
- `ENABLE_TLS=true` configures every engine to use TLS.

## Stage 6: README Documentation
**Goal**: Write comprehensive README.md.
**Success Criteria**:
- README covers all sections required by CLAUDE.md.

## Stage 7: Validation and Verification
**Goal**: Run linting and live tests.
**Success Criteria**:
- ShellCheck passes.
- `docker compose --profile all up -d` succeeds and all services report healthy.
- `make verify` passes.
- `make benchmark` produces `benchmark.md`.
