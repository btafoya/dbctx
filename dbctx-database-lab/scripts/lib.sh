#!/usr/bin/env bash
# Shared helpers for dbctx Database Lab scripts.
# This file is sourced by the other scripts in this directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAB_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck disable=SC2034
COMPOSE="docker compose -f ${LAB_DIR}/docker-compose.yml"

# Load .env if present so scripts can reuse configured values.
if [[ -f "${LAB_DIR}/.env" ]]; then
    set -a
    # shellcheck source=/dev/null
    source "${LAB_DIR}/.env"
    set +a
fi

# ---------------------------------------------------------------------------
# Container names (must match docker-compose.yml)
# ---------------------------------------------------------------------------
POSTGRES_CONTAINER="dbctx-postgres"
MARIADB_CONTAINER="dbctx-mariadb"
MYSQL_CONTAINER="dbctx-mysql"
MSSQL_CONTAINER="dbctx-mssql"
SQLITE_CONTAINER="dbctx-sqlite"

# ---------------------------------------------------------------------------
# Database connection defaults
# ---------------------------------------------------------------------------
DB_DATABASE="${DB_DATABASE:-app}"
DB_USERNAME="${DB_USERNAME:-app}"
DB_PASSWORD="${DB_PASSWORD:-secret}"
DB_ROOT_PASSWORD="${DB_ROOT_PASSWORD:-secret}"
MSSQL_SA_PASSWORD="${MSSQL_SA_PASSWORD:-Str0ngP@ssw0rd!}"

# ---------------------------------------------------------------------------
# Engine detection
# ---------------------------------------------------------------------------
is_container_running() {
    local name="$1"
    docker ps --format '{{.Names}}' | grep -qx "${name}"
}

engine_is_available() {
    local engine="$1"
    local container
    container="$(engine_to_container "${engine}")"
    is_container_running "${container}"
}

engine_to_container() {
    local engine="$1"
    case "${engine}" in
        postgres)  echo "${POSTGRES_CONTAINER}" ;;
        mariadb)   echo "${MARIADB_CONTAINER}" ;;
        mysql)     echo "${MYSQL_CONTAINER}" ;;
        mssql)     echo "${MSSQL_CONTAINER}" ;;
        sqlite)    echo "${SQLITE_CONTAINER}" ;;
        *)         echo "ERROR: unknown engine '${engine}'" >&2; exit 1 ;;
    esac
}

list_engines() {
    echo "postgres mariadb mysql mssql sqlite"
}

# ---------------------------------------------------------------------------
# SQL execution helpers
# ---------------------------------------------------------------------------
postgres_exec() {
    local sql="$1"
    docker exec "${POSTGRES_CONTAINER}" \
        psql -U "${DB_USERNAME}" -d "${DB_DATABASE}" -tAqc "${sql}"
}

mariadb_exec() {
    local sql="$1"
    docker exec "${MARIADB_CONTAINER}" \
        mariadb -u "${DB_USERNAME}" -p"${DB_PASSWORD}" --skip-ssl "${DB_DATABASE}" -Ne "${sql}"
}

mysql_exec() {
    local sql="$1"
    docker exec "${MYSQL_CONTAINER}" \
        mysql --ssl-mode=DISABLED -u "${DB_USERNAME}" -p"${DB_PASSWORD}" "${DB_DATABASE}" -Ne "${sql}"
}

mssql_exec() {
    local sql="$1"
    # SET NOCOUNT ON suppresses the '(N rows affected)' messages that would
    # otherwise pollute programmatic output.
    local wrapped_sql="SET NOCOUNT ON; ${sql}"
    if docker exec "${MSSQL_CONTAINER}" test -x /opt/mssql-tools18/bin/sqlcmd; then
        docker exec "${MSSQL_CONTAINER}" \
            /opt/mssql-tools18/bin/sqlcmd -S localhost -C -U "${DB_USERNAME}" -P "${DB_PASSWORD}" -d "${DB_DATABASE}" -Q "${wrapped_sql}" -h -1 -W
    else
        docker exec "${MSSQL_CONTAINER}" \
            /opt/mssql-tools/bin/sqlcmd -S localhost -U "${DB_USERNAME}" -P "${DB_PASSWORD}" -d "${DB_DATABASE}" -Q "${wrapped_sql}" -h -1 -W
    fi
}

sqlite_exec() {
    local sql="$1"
    docker exec "${SQLITE_CONTAINER}" sqlite3 /data/app.db "${sql}"
}

engine_exec() {
    local engine="$1"
    local sql="$2"
    case "${engine}" in
        postgres)  postgres_exec "${sql}" ;;
        mariadb)   mariadb_exec "${sql}" ;;
        mysql)     mysql_exec "${sql}" ;;
        mssql)     mssql_exec "${sql}" ;;
        sqlite)    sqlite_exec "${sql}" ;;
        *)         echo "ERROR: unknown engine '${engine}'" >&2; exit 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# Health checks
# ---------------------------------------------------------------------------
wait_for_engine() {
    local engine="$1"
    local timeout="${2:-120}"
    local container
    container="$(engine_to_container "${engine}")"

    if ! is_container_running "${container}"; then
        echo "SKIP: ${engine} container is not running"
        return 1
    fi

    echo "Waiting for ${engine} to become healthy (timeout ${timeout}s)..."
    local elapsed=0
    while [[ ${elapsed} -lt ${timeout} ]]; do
        local status
        status="$(docker inspect --format='{{.State.Health.Status}}' "${container}" 2>/dev/null || echo "unknown")"
        if [[ "${status}" == "healthy" ]]; then
            echo "${engine} is healthy."
            return 0
        fi
        sleep 2
        elapsed=$((elapsed + 2))
    done

    echo "ERROR: ${engine} did not become healthy within ${timeout}s" >&2
    return 1
}

wait_for_all_engines() {
    local timeout="${1:-120}"
    local failed=()
    for engine in $(list_engines); do
        if ! wait_for_engine "${engine}" "${timeout}"; then
            if is_container_running "$(engine_to_container "${engine}")"; then
                failed+=("${engine}")
            fi
        fi
    done

    if [[ ${#failed[@]} -gt 0 ]]; then
        echo "ERROR: the following engines failed to become healthy: ${failed[*]}" >&2
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Logging helpers
# ---------------------------------------------------------------------------
section() {
    echo ""
    echo "== $1 =="
}

pass() {
    echo "  [PASS] $1"
}

fail() {
    echo "  [FAIL] $1" >&2
}
