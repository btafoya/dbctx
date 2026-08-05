#!/usr/bin/env bash
# Destroy and recreate all persistent data in the dbctx database lab.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

echo "Resetting dbctx database lab persistent data..."
${COMPOSE} --profile all down -v --remove-orphans

# Remove any SQLite database that might have been copied into the workspace.
if [[ -f "${LAB_DIR}/sqlite/app.db" ]]; then
    rm -f "${LAB_DIR}/sqlite/app.db"
fi

echo "Persistent data removed. Re-creating services..."
bash "${SCRIPT_DIR}/up.sh" all
echo ""
echo "Reset complete. Run 'make wait' to block on health checks."
