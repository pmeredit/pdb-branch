#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/oracle-free-common.sh"

usage() {
    cat <<'USAGE'
Run pdb CLI integration tests against an Oracle Database Free container.

Environment variables:
USAGE
    oracle_free_usage_common
    cat <<'USAGE'
  PDB_BRANCH_SYS_USER                   SYSDBA username. Default: sys
  PDB_BRANCH_SYS_PASSWORD               SYSDBA password. Default: ORACLE_PWD
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

oracle_free_check_runtime
if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is not installed or not on PATH" >&2
    exit 127
fi

oracle_free_register_cleanup
oracle_free_ensure_container

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pdb-cli-oracle-free.XXXXXX")"
PDB_BIN=""
PDB_ARGS=()
CREATED_BRANCHES=()
cleanup_cli() {
    if [[ -n "${PDB_BIN:-}" && ${#CREATED_BRANCHES[@]} -gt 0 ]]; then
        local branch
        for branch in "${CREATED_BRANCHES[@]}"; do
            "$PDB_BIN" "${PDB_ARGS[@]}" branch --delete "$branch" >/dev/null 2>&1 || true
        done
    fi
    rm -rf "$WORK_DIR"
}
trap 'cleanup_cli; oracle_free_cleanup' EXIT

cd "$ROOT_DIR/bindings/rust"
cargo build --no-default-features --features cli --bin pdb

PDB_BIN="$ROOT_DIR/bindings/rust/target/debug/pdb"
SYS_USER="${PDB_BRANCH_SYS_USER:-sys}"
SYS_PASSWORD="${PDB_BRANCH_SYS_PASSWORD:-$ORACLE_PASSWORD}"
PDB_ARGS=(-C "$WORK_DIR" --no-install)

"$PDB_BIN" \
    -C "$WORK_DIR" \
    --no-install \
    --dsn "localhost:${PORT}/FREE" \
    --user "$SYS_USER" \
    --password "$SYS_PASSWORD" \
    init \
    --force \
    --from FREEPDB1 \
    --no-snapshot

"$PDB_BIN" -C "$WORK_DIR" install

make_branch_name() {
    local prefix="$1"
    printf '%s%04d%04d\n' "$prefix" "$RANDOM" "$RANDOM"
}

run_pdb() {
    "$PDB_BIN" "${PDB_ARGS[@]}" "$@"
}

assert_contains() {
    local haystack="$1"
    local needle="$2"
    local message="$3"

    if [[ "$haystack" != *"$needle"* ]]; then
        printf 'error: %s\n' "$message" >&2
        printf 'expected to find: %s\n' "$needle" >&2
        printf 'output was:\n%s\n' "$haystack" >&2
        exit 1
    fi
}

assert_not_contains() {
    local haystack="$1"
    local needle="$2"
    local message="$3"

    if [[ "$haystack" == *"$needle"* ]]; then
        printf 'error: %s\n' "$message" >&2
        printf 'did not expect to find: %s\n' "$needle" >&2
        printf 'output was:\n%s\n' "$haystack" >&2
        exit 1
    fi
}

branch_line() {
    local output="$1"
    local branch_name="$2"
    local line

    while IFS= read -r line; do
        if [[ "$line" == *"$branch_name"* ]]; then
            printf '%s\n' "$line"
            return 0
        fi
    done <<< "$output"

    return 1
}

require_branch_line() {
    local output="$1"
    local branch_name="$2"

    if ! branch_line "$output" "$branch_name"; then
        printf 'error: branch %s should be listed\n' "$branch_name" >&2
        printf 'output was:\n%s\n' "$output" >&2
        exit 1
    fi
}

run_lifecycle() {
    local branch_name="$1"
    local snapshot_copy="$2"
    local create_output
    local list_output

    if [[ "$snapshot_copy" == "1" ]]; then
        create_output="$(run_pdb branch "$branch_name" --snapshot --notes "pdb CLI Oracle Free snapshot test" 2>&1)"
        printf '%s\n' "$create_output"
        assert_contains \
            "$create_output" \
            "created with full clone" \
            "snapshot-copy fallback warning should be surfaced by the CLI"
    else
        create_output="$(run_pdb branch "$branch_name" --no-snapshot --notes "pdb CLI Oracle Free full clone test" 2>&1)"
        printf '%s\n' "$create_output"
        assert_not_contains \
            "$create_output" \
            "SNAPSHOT COPY" \
            "full-clone create should not print snapshot-copy warning"
    fi
    CREATED_BRANCHES+=("$branch_name")

    list_output="$(run_pdb branch --list --verbose)"
    printf '%s\n' "$list_output"
    local line
    line="$(require_branch_line "$list_output" "$branch_name")"
    assert_contains "$line" "OPEN" "created branch should be open"

    run_pdb score "$branch_name" 0.99 --notes "pdb CLI Oracle Free integration passed"

    list_output="$(run_pdb branch --list --verbose)"
    printf '%s\n' "$list_output"
    line="$(require_branch_line "$list_output" "$branch_name")"
    assert_contains "$line" "0.99" "branch score should be listed"

    run_pdb close "$branch_name"
    list_output="$(run_pdb branch --list --verbose)"
    printf '%s\n' "$list_output"
    line="$(require_branch_line "$list_output" "$branch_name")"
    assert_contains "$line" "CLOSED" "closed branch should be listed as closed"

    run_pdb open "$branch_name"
    list_output="$(run_pdb branch --list --verbose)"
    printf '%s\n' "$list_output"
    line="$(require_branch_line "$list_output" "$branch_name")"
    assert_contains "$line" "OPEN" "reopened branch should be listed as open"

    run_pdb promote "$branch_name" --notes "pdb CLI Oracle Free integration winner"
    list_output="$(run_pdb branch --list --verbose)"
    printf '%s\n' "$list_output"
    line="$(require_branch_line "$list_output" "$branch_name")"
    assert_contains "$line" "PROMOTED" "promoted branch should be listed as promoted"

    run_pdb branch --delete "$branch_name"
    list_output="$(run_pdb branch --list)"
    printf '%s\n' "$list_output"
    assert_not_contains "$list_output" "$branch_name" "dropped branch should be hidden by default"
}

run_lifecycle "$(make_branch_name CBIFC)" 0

if [[ "${PDB_BRANCH_TEST_SNAPSHOT_COPY:-0}" == "1" ]]; then
    run_lifecycle "$(make_branch_name CBISC)" 1
fi
