#!/usr/bin/env bash
# MariaDB custom entrypoint for the dbctx database lab.
# Copies development TLS material into a writable location and delegates to the
# official MariaDB entrypoint with the correct SSL configuration.
set -euo pipefail

ENABLE_TLS="${ENABLE_TLS:-false}"
SRC_TLS_DIR="/etc/mysql/tls"
DST_TLS_DIR="/tmp/mariadb-tls"

configure_tls() {
    if [[ ! -f "${SRC_TLS_DIR}/server.crt" || ! -f "${SRC_TLS_DIR}/server.key" ]]; then
        return 0
    fi

    mkdir -p "${DST_TLS_DIR}"
    cp "${SRC_TLS_DIR}/server.crt" "${SRC_TLS_DIR}/server.key" "${SRC_TLS_DIR}/ca.crt" "${DST_TLS_DIR}/"
    chown -R mysql:mysql "${DST_TLS_DIR}"
    chmod 600 "${DST_TLS_DIR}/server.key"
}

configure_tls

if [[ "${ENABLE_TLS}" == "true" || "${ENABLE_TLS}" == "1" || "${ENABLE_TLS}" == "ON" ]]; then
    if [[ -f "${DST_TLS_DIR}/server.crt" ]]; then
        echo "MariaDB TLS enabled."
        exec /usr/local/bin/docker-entrypoint.sh \
            mariadbd \
            --character-set-server=utf8mb4 \
            --collation-server=utf8mb4_unicode_ci \
            --default-storage-engine=InnoDB \
            --innodb-default-row-format=DYNAMIC \
            --innodb-page-size="${MARIADB_INNODB_PAGE_SIZE:-16384}" \
            --ssl=ON \
            --ssl-ca="${DST_TLS_DIR}/ca.crt" \
            --ssl-cert="${DST_TLS_DIR}/server.crt" \
            --ssl-key="${DST_TLS_DIR}/server.key" \
            --require-secure-transport="${MARIADB_REQUIRE_TLS:-OFF}"
    fi
    echo "WARNING: ENABLE_TLS is enabled but certificates are missing in ${SRC_TLS_DIR}." >&2
fi

exec /usr/local/bin/docker-entrypoint.sh \
    mariadbd \
    --character-set-server=utf8mb4 \
    --collation-server=utf8mb4_unicode_ci \
    --default-storage-engine=InnoDB \
    --innodb-default-row-format=DYNAMIC \
    --innodb-page-size="${MARIADB_INNODB_PAGE_SIZE:-16384}" \
    --ssl=OFF
