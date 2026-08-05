#!/usr/bin/env bash
# Stop all dbctx database lab services cleanly.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

echo "Stopping dbctx database lab services..."
${COMPOSE} --profile all down --remove-orphans
echo "All dbctx services stopped."
