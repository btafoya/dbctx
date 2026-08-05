#!/usr/bin/env bash
# Generate self-signed development TLS certificates for every database engine.
# Certificates are written to <engine>/tls/ and are reused if they already exist.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAB_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
CN="${TLS_CN:-dbctx.local}"
DAYS="${TLS_DAYS:-365}"

# Engines that need TLS material under their own directory.
ENGINES=("postgres" "mariadb" "mysql" "mssql")

generate_for_engine() {
    local engine="$1"
    local tls_dir="${LAB_DIR}/${engine}/tls"
    mkdir -p "${tls_dir}"

    if [[ -f "${tls_dir}/ca.crt" && -f "${tls_dir}/server.crt" && -f "${tls_dir}/server.key" ]]; then
        echo "TLS certificates already exist for ${engine}; skipping."
        return 0
    fi

    echo "Generating TLS certificates for ${engine}..."

    # Remove stale partial files.
    rm -f "${tls_dir}/ca.key" "${tls_dir}/ca.crt" "${tls_dir}/server.csr" "${tls_dir}/server.key" "${tls_dir}/server.crt"

    # Generate a CA private key and self-signed certificate.
    openssl genrsa -out "${tls_dir}/ca.key" 2048 2>/dev/null
    openssl req -new -x509 -days "${DAYS}" -key "${tls_dir}/ca.key" \
        -out "${tls_dir}/ca.crt" -subj "/CN=dbctx-${engine}-ca/O=dbctx" 2>/dev/null

    # Generate a server private key and certificate signing request.
    openssl genrsa -out "${tls_dir}/server.key" 2048 2>/dev/null
    openssl req -new -key "${tls_dir}/server.key" \
        -out "${tls_dir}/server.csr" -subj "/CN=${CN}/O=dbctx" 2>/dev/null

    # Sign the server certificate with the CA.
    openssl x509 -req -days "${DAYS}" -in "${tls_dir}/server.csr" \
        -CA "${tls_dir}/ca.crt" -CAkey "${tls_dir}/ca.key" \
        -CAcreateserial -out "${tls_dir}/server.crt" 2>/dev/null

    # Clean up intermediate files.
    rm -f "${tls_dir}/ca.key" "${tls_dir}/server.csr" "${tls_dir}/ca.srl"

    # Restrict private key permissions.
    chmod 600 "${tls_dir}/server.key"
    chmod 644 "${tls_dir}/server.crt" "${tls_dir}/ca.crt"

    echo "TLS certificates generated for ${engine}."
}

main() {
    if ! command -v openssl >/dev/null 2>&1; then
        echo "ERROR: openssl is required to generate TLS certificates." >&2
        exit 1
    fi

    for engine in "${ENGINES[@]}"; do
        generate_for_engine "${engine}"
    done
}

main "$@"
