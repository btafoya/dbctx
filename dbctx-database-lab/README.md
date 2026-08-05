# dbctx Database Lab

A standalone, production-quality database compatibility laboratory for the
[`dbctx`](../) project. It provides a Docker-based environment for
development, testing, benchmarking, and regression validation across multiple
database engines.

> This directory is **not** part of the core `dbctx` product. The main project
> continues to support only MySQL, MariaDB, and SQL Server per `VISION.md`.

------------------------------------------------------------------------

## Architecture Overview

The lab is a single Docker Compose v2 stack with one service per supported
database engine. Each engine runs in an isolated container, mounts its own
persistent named volume, and initializes itself from engine-specific SQL
scripts at startup.

Key design decisions:

- **No manual setup** beyond `cp .env.example .env`.
- **Deterministic schemas and seed data** across all engines.
- **Optional TLS** with automatically generated development certificates.
- **Profiles** for starting individual engines or groups.
- **Administration UIs** available through optional profiles.
- **Verification and benchmark scripts** run from the host against live
  containers.

------------------------------------------------------------------------

## Repository Layout

```
.
├── docker-compose.yml      # Compose v2 stack definition
├── .env.example            # Default environment variables
├── Makefile                # Convenience targets
├── README.md               # This file
├── scripts/                # Bash automation scripts
│   ├── up.sh
│   ├── down.sh
│   ├── wait.sh
│   ├── verify.sh
│   ├── benchmark.sh
│   ├── reset.sh
│   └── init-certs.sh
├── postgres/               # PostgreSQL init SQL, TLS, entrypoint
├── mariadb/                # MariaDB init SQL, TLS, entrypoint
├── mysql/                  # MySQL init SQL, TLS, entrypoint
├── mssql/                  # SQL Server init SQL, TLS, entrypoint
├── sqlite/                 # SQLite init SQL
├── datasets/               # Deterministic CSV seed datasets
│   ├── small/
│   ├── medium/
│   └── large/
└── docs/                   # Additional documentation
```

------------------------------------------------------------------------

## Supported Databases

| Engine   | Version                       | Image                                  |
|----------|-------------------------------|----------------------------------------|
| PostgreSQL | 17                            | `postgres:17-alpine`                   |
| MariaDB  | 12.2                          | `mariadb:12.2.2`                       |
| MySQL    | 8.4 LTS                       | `mysql:8.4`                            |
| SQL Server | 2025 Developer              | `mcr.microsoft.com/mssql/server:2025-latest` |
| SQLite   | 3 (via Alpine)                | `alpine:3.22`                          |

------------------------------------------------------------------------

## Quick Start

```bash
cp .env.example .env
make up
make wait
make verify
```

`make up` starts every database service. `make wait` blocks until all running
services report healthy. `make verify` runs the cross-engine verification suite.

------------------------------------------------------------------------

## Profiles

Use `--profile <name>` with `docker compose` or the Makefile targets below.

| Profile | Services Started                                   |
|---------|----------------------------------------------------|
| `all`   | PostgreSQL, MariaDB, MySQL, SQL Server, SQLite     |
| `postgres` | PostgreSQL only                                 |
| `mariadb`  | MariaDB only                                    |
| `mysql`    | MySQL only                                      |
| `mssql`    | SQL Server only                                 |
| `sqlite`   | SQLite only                                     |
| `ui`       | Adminer only                                    |
| `admin`    | Adminer, pgAdmin, phpMyAdmin                    |

Examples:

```bash
make up                 # profile all
make ui                 # profile ui
make admin              # profile admin
bash scripts/up.sh mssql # profile mssql
```

------------------------------------------------------------------------

## Ports

Default ports are defined in `.env.example` and can be changed in `.env`.

| Service      | Default Port | Container Port |
|--------------|--------------|----------------|
| PostgreSQL   | 5432         | 5432           |
| MariaDB      | 3306         | 3306           |
| MySQL        | 3307         | 3306           |
| SQL Server   | 1433         | 1433           |
| Adminer      | 8080         | 8080           |
| pgAdmin      | 5050         | 80             |
| phpMyAdmin   | 8081         | 80             |

------------------------------------------------------------------------

## Credentials

| Engine     | Username | Password             | Database |
|------------|----------|----------------------|----------|
| PostgreSQL | `app`    | `secret`             | `app`    |
| MariaDB    | `app`    | `secret`             | `app`    |
| MySQL      | `app`    | `secret`             | `app`    |
| SQL Server | `app`    | `secret`             | `app`    |
| SQLite     | N/A      | N/A                  | `/data/app.db` |

SQL Server additionally requires the `SA` password, which is configurable via
`MSSQL_SA_PASSWORD`.

------------------------------------------------------------------------

## Environment Variables

All tunable values are exposed in `.env.example`. Copy it to `.env` and adjust
as needed.

Key variables:

- `DB_TIMEZONE` - timezone passed to every service
- `DB_DATABASE`, `DB_USERNAME`, `DB_PASSWORD` - application credentials
- `DB_ROOT_PASSWORD` - MariaDB/MySQL root password
- `MSSQL_SA_PASSWORD` - SQL Server `SA` password
- `*_PORT` - host port for each service
- `DB_CPU_LIMIT`, `DB_MEMORY_LIMIT`, etc. - resource limits
- `ENABLE_TLS` - set to `true` to enable TLS for supported engines
- `TLS_CN`, `TLS_DAYS` - development certificate settings
- `PGADMIN_EMAIL`, `PGADMIN_PASSWORD` - pgAdmin login

------------------------------------------------------------------------

## TLS

Development TLS certificates are generated automatically by `make certs` (also
invoked by `make up`). To enable TLS, set in `.env`:

```text
ENABLE_TLS=true
```

Then restart the stack:

```bash
make reset
```

The `scripts/init-certs.sh` script creates per-engine certificates under
`<engine>/tls/`:

- `ca.crt`
- `server.crt`
- `server.key`

Database-specific entrypoints copy these into writable locations and set
ownership so each engine can read its key.

> These are self-signed development certificates. Do not use them in
> production.

------------------------------------------------------------------------

## Loading Custom SQL

Place `.sql` files in the engine's `init/` directory and restart the service.
Existing data is preserved unless you reset the volume.

```bash
# PostgreSQL example
cp my-schema.sql postgres/init/002-my-schema.sql
make restart
```

For one-off statements, connect with the appropriate client inside the
container:

```bash
docker exec -it dbctx-postgres psql -U app -d app
```

------------------------------------------------------------------------

## Resetting Databases

To destroy all persistent data and recreate fresh containers:

```bash
make reset
```

This runs `docker compose down -v` and then `make up`.

------------------------------------------------------------------------

## Backups

Back up individual engines using the standard client tools:

```bash
# PostgreSQL
docker exec dbctx-postgres pg_dump -U app -d app > backup.sql

# MariaDB / MySQL
docker exec dbctx-mariadb mariadb-dump -u app -psecret app > backup.sql

# SQL Server
docker exec dbctx-mssql /opt/mssql-tools18/bin/sqlcmd -S localhost -C -U SA -P 'Str0ngP@ssw0rd!' -Q 'BACKUP DATABASE [app] TO DISK = "/var/opt/mssql/backup/app.bak"'
```

SQLite persists its database in the `sqlite_data` Docker volume. Copy it from
the container:

```bash
docker cp dbctx-sqlite:/data/app.db ./sqlite/app.db
```

------------------------------------------------------------------------

## Troubleshooting

### A service never becomes healthy

```bash
make status
make logs
```

Check resource limits if SQL Server fails to start; it requires at least 2 GB
of memory.

### TLS errors after enabling ENABLE_TLS

Ensure certificates were generated:

```bash
make certs
ls postgres/tls/
```

Then reset the stack so the custom entrypoints copy the new keys.

### Verification fails on one engine

Run the verification script for a single engine by starting only that profile:

```bash
make down
bash scripts/up.sh postgres
bash scripts/wait.sh
bash scripts/verify.sh
```

------------------------------------------------------------------------

## Benchmarking

Run the benchmark suite after the databases are healthy:

```bash
make benchmark
```

This produces `benchmark.md` with connection latency and approximate
insert/select/update/delete/transaction throughput for each running engine.
The default scale is 1,000 rows per operation. Change it with:

```bash
BENCHMARK_SCALE=10000 make benchmark
```

------------------------------------------------------------------------

## Verification

The verification suite checks every running engine for:

- connectivity
- authentication
- schema creation (tables, indexes, foreign keys, views)
- CRUD operations
- transactions (rollback and commit)
- prepared/parameterized statements
- Unicode support
- binary/blob round-trip
- timestamp defaults
- stored procedures and functions where supported

Run it with:

```bash
make verify
```

Exit code is non-zero if any check fails.

------------------------------------------------------------------------

## Development

All shell scripts use `set -euo pipefail` and are intended to be
ShellCheck-clean. Validate changes with:

```bash
shellcheck scripts/*.sh */docker-entrypoint.sh
```

------------------------------------------------------------------------

## License

This lab shares the license of the parent `dbctx` project.
