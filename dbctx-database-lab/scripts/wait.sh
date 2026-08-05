#!/usr/bin/env bash
# Wait until every running database service reports healthy.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

TIMEOUT="${1:-120}"

echo "Waiting for running database services to become healthy..."
wait_for_all_engines "${TIMEOUT}"
echo ""
echo "All running database services are healthy."
