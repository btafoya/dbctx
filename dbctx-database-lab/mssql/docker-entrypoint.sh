#!/usr/bin/env bash
# SQL Server 2025 custom entrypoint for the dbctx database lab.
# Handles optional TLS configuration and schema initialization before
# starting sqlservr.
set -euo pipefail

ENABLE_TLS="${ENABLE_TLS:-false}"
TLS_DIR="/var/opt/mssql/tls"
MSSQL_CONF="/var/opt/mssql/mssql.conf"
INIT_DIR="/init"
SQLCMD="/opt/mssql-tools18/bin/sqlcmd"

# Configure TLS if requested and certificates are present.
configure_tls() {
    if [[ "${ENABLE_TLS}" != "true" && "${ENABLE_TLS}" != "1" && "${ENABLE_TLS}" != "ON" ]]; then
        return 0
    fi

    if [[ ! -f "${TLS_DIR}/server.crt" || ! -f "${TLS_DIR}/server.key" ]]; then
        echo "WARNING: ENABLE_TLS is enabled but certificates are missing in ${TLS_DIR}." >&2
        return 0
    fi

    # Ensure the certificate and key are readable by the mssql user (uid 10001).
    cp "${TLS_DIR}/server.crt" /var/opt/mssql/server.crt
    cp "${TLS_DIR}/server.key" /var/opt/mssql/server.key
    chmod 600 /var/opt/mssql/server.key
    chown 10001:10001 /var/opt/mssql/server.crt /var/opt/mssql/server.key || true

    cat > "${MSSQL_CONF}" <<EOF
[network]
tlscert = /var/opt/mssql/server.crt
tlskey = /var/opt/mssql/server.key
tlsprotocols = 1.2
forceencryption = 0
EOF
    echo "SQL Server TLS configured."
}

# Wait for SQL Server to finish startup and then run initialization scripts.
run_init_scripts() {
    local database="${DB_DATABASE:-app}"
    local user="${DB_USERNAME:-app}"
    local password="${DB_PASSWORD:-secret}"
    local sa_password="${MSSQL_SA_PASSWORD:-Str0ngP@ssw0rd!}"

    echo "Waiting for SQL Server to accept connections..."
    local ready=0
    for _ in $(seq 1 90); do
        if "${SQLCMD}" -S localhost -C -U SA -P "${sa_password}" -Q "SELECT 1" -b -o /dev/null 2>/dev/null; then
            ready=1
            break
        fi
        sleep 2
    done

    if [[ ${ready} -ne 1 ]]; then
        echo "ERROR: SQL Server did not become ready within 180s." >&2
        return 1
    fi

    echo "SQL Server is ready."
    echo "Creating database '${database}' and application user '${user}'..."

    if "${SQLCMD}" -S localhost -C -U SA -P "${sa_password}" -d master -Q "
        IF NOT EXISTS (SELECT name FROM sys.databases WHERE name = N'${database}')
            CREATE DATABASE [${database}];
    " -b; then
        echo "  database created/verified."
    else
        echo "ERROR: failed to create database." >&2
        return 1
    fi

    if "${SQLCMD}" -S localhost -C -U SA -P "${sa_password}" -d master -Q "
        IF NOT EXISTS (SELECT name FROM sys.sql_logins WHERE name = N'${user}')
            CREATE LOGIN [${user}] WITH PASSWORD = N'${password}', CHECK_POLICY = OFF;
    " -b; then
        echo "  login created/verified."
    else
        echo "ERROR: failed to create login." >&2
        return 1
    fi

    if "${SQLCMD}" -S localhost -C -U SA -P "${sa_password}" -d "${database}" -Q "
        IF NOT EXISTS (SELECT name FROM sys.database_principals WHERE name = N'${user}')
            CREATE USER [${user}] FOR LOGIN [${user}];
        ALTER ROLE db_owner ADD MEMBER [${user}];
    " -b; then
        echo "  user created/verified."
    else
        echo "ERROR: failed to create user." >&2
        return 1
    fi

    if [[ -d "${INIT_DIR}" ]]; then
        for script in "${INIT_DIR}"/*.sql; do
            [[ -f "${script}" ]] || continue
            echo "Running init script: ${script}"
            if "${SQLCMD}" -S localhost -C -U "${user}" -P "${password}" -d "${database}" -i "${script}" -b; then
                echo "  ${script} complete."
            else
                echo "ERROR: init script failed: ${script}" >&2
                return 1
            fi
        done
    fi

    echo "SQL Server initialization complete."
}

configure_tls

# Start SQL Server in the background, run initialization, then bring it to the foreground.
if [[ $# -eq 0 ]]; then
    set -- /opt/mssql/bin/sqlservr
fi

# Launch sqlservr in the background so we can initialize the schema.
"$@" &
SQL_PID=$!

run_init_scripts || {
    echo "WARNING: SQL Server schema initialization failed; continuing with sqlservr." >&2
}

# Wait for the SQL Server process to keep the container alive.
wait "${SQL_PID}"
