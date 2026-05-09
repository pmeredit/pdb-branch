#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/oracle-free-common.sh"

usage() {
    cat <<'USAGE'
Run pdb-branch Node.js integration tests against an Oracle Database Free container.

Environment variables:
USAGE
    oracle_free_usage_common
    oracle_free_usage_node
    cat <<'USAGE'
Any arguments are forwarded to node --test.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

oracle_free_check_runtime
if ! command -v "$NODE" >/dev/null 2>&1; then
    echo "error: $NODE is not installed or not on PATH" >&2
    exit 127
fi
if ! command -v "$NPM" >/dev/null 2>&1; then
    echo "error: $NPM is not installed or not on PATH" >&2
    exit 127
fi

oracle_free_register_cleanup
oracle_free_ensure_container

cd "$ROOT_DIR/bindings/node"
if ! "$NODE" --input-type=module -e 'await import("oracledb")' >/dev/null 2>&1; then
    "$NPM" install --no-save --package-lock=false oracledb
fi

PDB_BRANCH_INTEGRATION=1 \
PDB_BRANCH_ROOT_DSN="localhost:${PORT}/FREE" \
PDB_BRANCH_BRANCH_DSN_TEMPLATE="localhost:${PORT}/{branch_name}" \
PDB_BRANCH_SYS_PASSWORD="$ORACLE_PASSWORD" \
"$NODE" --test "$@" test/oracle-free.integration.test.js
