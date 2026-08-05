#!/usr/bin/env bash
# PostgreSQL custom entrypoint for the dbctx database lab.
# Copies development TLS material into a writable location and delegates to the
# official PostgreSQL entrypoint with the correct SSL configuration.
set -euo pipefail

ENABLE_TLS="${ENABLE_TLS:-false}"
SRC_TLS_DIR="/var/lib/postgresql/tls"
DST_TLS_DIR="/tmp/postgres-tls"

configure_tls() {
    if [[ ! -f "${SRC_TLS_DIR}/server.crt" || ! -f "${SRC_TLS_DIR}/server.key" ]]; then
        return 0
    fi

    mkdir -p "${DST_TLS_DIR}"
    cp "${SRC_TLS_DIR}/server.crt" "${SRC_TLS_DIR}/server.key" "${SRC_TLS_DIR}/ca.crt" "${DST_TLS_DIR}/"
    chown -R postgres:postgres "${DST_TLS_DIR}"
    chmod 600 "${DST_TLS_DIR}/server.key"
}

configure_tls

# Build the postgres command line dynamically so SSL options are only present
# when TLS is enabled.
SSL_FLAG="${ENABLE_TLS:-false}"

if [[ "${SSL_FLAG}" == "true" || "${SSL_FLAG}" == "1" || "${SSL_FLAG}" == "ON" ]]; then
    if [[ -f "${DST_TLS_DIR}/server.crt" ]]; then
        echo "PostgreSQL TLS enabled."
        exec /usr/local/bin/docker-entrypoint.sh \
            postgres \
            -c "shared_buffers=${POSTGRES_SHARED_BUFFERS:-128MB}" \
            -c "work_mem=${POSTGRES_WORK_MEM:-4MB}" \
            -c "maintenance_work_mem=${POSTGRES_MAINTENANCE_WORK_MEM:-64MB}" \
            -c "max_connections=${POSTGRES_MAX_CONNECTIONS:-200}" \
            -c "ssl=on" \
            -c "ssl_cert_file=${DST_TLS_DIR}/server.crt" \
            -c "ssl_key_file=${DST_TLS_DIR}/server.key" \
            -c "ssl_ca_file=${DST_TLS_DIR}/ca.crt"
    fi
    echo "WARNING: ENABLE_TLS is enabled but certificates are missing in ${SRC_TLS_DIR}." >&2
fi

exec /usr/local/bin/docker-entrypoint.sh \
    postgres \
    -c "shared_buffers=${POSTGRES_SHARED_BUFFERS:-128MB}" \
    -c "work_mem=${POSTGRES_WORK_MEM:-4MB}" \
    -c "maintenance_work_mem=${POSTGRES_MAINTENANCE_WORK_MEM:-64MB}" \
    -c "max_connections=${POSTGRES_MAX_CONNECTIONS:-200}" \
    -c "ssl=off"
