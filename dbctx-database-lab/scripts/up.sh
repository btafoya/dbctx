#!/usr/bin/env bash
# Start selected Docker Compose profiles for the dbctx database lab.
# Usage: up.sh [profile]
# If no profile is provided, the "all" profile is started.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/lib.sh"

PROFILE="${1:-all}"

echo "Starting dbctx database lab profile: ${PROFILE}"

# Generate TLS certificates if needed before bringing up services.
bash "${SCRIPT_DIR}/init-certs.sh"

${COMPOSE} --profile "${PROFILE}" up -d --remove-orphans
${COMPOSE} --profile "${PROFILE}" ps

echo ""
echo "Profile '${PROFILE}' is starting. Run 'make wait' to block on health checks."
