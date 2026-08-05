# CLAUDE.md

## Mission

Build a **production-quality database compatibility laboratory** for the
`dbctx` project.

This repository is the canonical development, testing, benchmarking, and
regression environment for `dbctx`.

The finished repository must require **zero manual setup** beyond:

``` bash
cp .env.example .env
make up
make verify
```

The project is complete only when every supported database can be
started, verified, benchmarked, and reset using the supplied tooling.

------------------------------------------------------------------------

# Supported Databases

Implement and maintain support for:

-   PostgreSQL 17
-   MariaDB 12.2
-   MySQL 8.4 LTS
-   Microsoft SQL Server 2025 Developer
-   SQLite

Do not add any other engines.

------------------------------------------------------------------------

# Optional Administration UIs

Implement profiles for:

-   Adminer
-   pgAdmin
-   phpMyAdmin

Requirements:

-   `ui` profile starts Adminer only.
-   `admin` profile starts Adminer + pgAdmin + phpMyAdmin.

------------------------------------------------------------------------

# Repository Layout

    .
    ├── docker-compose.yml
    ├── .env.example
    ├── Makefile
    ├── README.md
    ├── scripts/
    │   ├── up.sh
    │   ├── down.sh
    │   ├── wait.sh
    │   ├── verify.sh
    │   ├── benchmark.sh
    │   └── reset.sh
    ├── postgres/
    │   ├── init/
    │   ├── seed/
    │   ├── tls/
    │   ├── backup/
    │   └── data/
    ├── mysql/
    ├── mariadb/
    ├── mssql/
    ├── sqlite/
    │   └── app.db
    ├── datasets/
    │   ├── small/
    │   ├── medium/
    │   └── large/
    └── docs/

------------------------------------------------------------------------

# Docker Compose Requirements

The compose file should be production quality.

Use:

-   Docker Compose v2
-   YAML anchors
-   x-\* extension fields
-   Named containers
-   Named volumes
-   Named network
-   Profiles
-   Restart policies
-   Health checks
-   Logging configuration
-   init: true where appropriate
-   stop_grace_period
-   ulimits
-   Security options
-   Read-only bind mounts where appropriate

Implement services for:

-   PostgreSQL
-   MariaDB
-   MySQL
-   SQL Server
-   SQLite
-   Adminer
-   pgAdmin
-   phpMyAdmin

------------------------------------------------------------------------

# Profiles

Implement:

-   all
-   postgres
-   mysql
-   mariadb
-   mssql
-   sqlite
-   ui
-   admin

------------------------------------------------------------------------

# Resource Limits

Every service must define CPU and memory reservations and limits.

------------------------------------------------------------------------

# Health Checks

Every database must expose a real health check.

Examples:

-   PostgreSQL → pg_isready
-   MariaDB → mariadb-admin ping
-   MySQL → mysqladmin ping
-   SQL Server → sqlcmd SELECT 1
-   SQLite → sqlite3 app.db ".tables"

Compose should wait for healthy dependencies where appropriate.

------------------------------------------------------------------------

# Environment Variables

Everything configurable through .env.

Include:

-   ports
-   usernames
-   passwords
-   database names
-   timezone
-   TLS enable/disable
-   memory limits
-   CPU limits
-   admin UI enable flags

Provide a complete `.env.example`.

------------------------------------------------------------------------

# Initialization

Every engine must automatically create:

-   database
-   application user
-   schema
-   indexes
-   foreign keys
-   views
-   stored procedure/function where supported

No manual SQL should be required.

------------------------------------------------------------------------

# Seed Data

Provide deterministic datasets containing:

-   users
-   companies
-   products
-   orders
-   order_items

Provide:

-   small
-   medium
-   large

All engines should contain equivalent logical data.

------------------------------------------------------------------------

# SQLite

Persist:

    sqlite/app.db

Automatically initialize it.

------------------------------------------------------------------------

# TLS

Support optional TLS.

Each engine should contain:

    tls/
        ca.crt
        server.crt
        server.key

Generate development certificates automatically if absent.

Enable through:

    ENABLE_TLS=true

------------------------------------------------------------------------

# Engine Tuning

Implement reasonable development tuning.

PostgreSQL:

-   shared_buffers
-   work_mem
-   maintenance_work_mem
-   max_connections

MySQL:

-   innodb_buffer_pool_size
-   utf8mb4
-   utf8mb4_0900_ai_ci

MariaDB:

-   utf8mb4
-   dynamic row format
-   page size

SQL Server:

-   Developer edition
-   memory cap
-   startup validation

------------------------------------------------------------------------

# Makefile

Implement:

-   make up
-   make down
-   make restart
-   make reset
-   make clean
-   make logs
-   make status
-   make verify
-   make benchmark
-   make ui
-   make admin

------------------------------------------------------------------------

# Scripts

Use Bash.

Every script must use:

``` bash
set -euo pipefail
```

Implement:

## up.sh

Starts selected profiles.

## down.sh

Stops services cleanly.

## wait.sh

Waits until every enabled database is healthy.

## verify.sh

Automatically verifies:

-   connectivity
-   authentication
-   schema creation
-   CRUD
-   transactions
-   prepared statements
-   Unicode
-   binary/blob support
-   timestamps
-   indexes
-   views
-   stored procedures where supported

Exit non-zero on failure.

## benchmark.sh

Run identical SQL workloads.

Generate:

    benchmark.md

Include:

-   connection latency
-   insert throughput
-   select throughput
-   update throughput
-   delete throughput
-   transaction throughput

## reset.sh

Destroy and recreate all persistent data.

------------------------------------------------------------------------

# Documentation

README.md must include:

-   architecture overview
-   repository layout
-   quick start
-   supported databases
-   profiles
-   ports
-   credentials
-   environment variables
-   TLS
-   loading custom SQL
-   resetting databases
-   backups
-   troubleshooting
-   benchmarking
-   verification

------------------------------------------------------------------------

# Code Quality

Requirements:

-   Production-ready
-   Fully commented
-   ShellCheck clean
-   Consistent formatting
-   No duplicated logic
-   No TODOs
-   No placeholders
-   No incomplete implementations

------------------------------------------------------------------------

# Acceptance Criteria

The project is complete only when:

-   `docker compose --profile all up -d` succeeds.
-   Every service reports healthy.
-   `make verify` passes.
-   `make benchmark` produces benchmark.md.
-   Every database contains equivalent logical schema.
-   Every database contains deterministic seed data.
-   TLS can be enabled without code changes.
-   README is complete.
-   No manual intervention is required.
