#!/usr/bin/env bash

ROOT_DIR="${ROOT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RUNTIME="${CONTAINER_RUNTIME:-podman}"
IMAGE="${ORACLE_FREE_IMAGE:-container-registry.oracle.com/database/free:latest}"
CONTAINER="${ORACLE_FREE_CONTAINER:-pdb-branch-oracle-free}"
PORT="${ORACLE_FREE_PORT:-1521}"
ORACLE_PASSWORD="${ORACLE_PWD:-PdbBranch1_}"
PYTHON="${PYTHON:-python3}"
VENV_DIR="${PDB_BRANCH_TEST_VENV:-${ROOT_DIR}/.venv-integration}"
STARTUP_TIMEOUT_SECONDS="${ORACLE_FREE_STARTUP_TIMEOUT_SECONDS:-1800}"
REMOVE_CONTAINER="${PDB_BRANCH_REMOVE_ORACLE:-0}"
RECREATE_CONTAINER="${PDB_BRANCH_RECREATE_ORACLE:-0}"

oracle_free_usage_common() {
    cat <<'USAGE'
  CONTAINER_RUNTIME                     podman or docker. Default: podman
  ORACLE_FREE_IMAGE                     Oracle Free image. Default: container-registry.oracle.com/database/free:latest
  ORACLE_FREE_CONTAINER                 Container name. Default: pdb-branch-oracle-free
  ORACLE_FREE_PORT                      Host listener port. Default: 1521
  ORACLE_PWD                            SYS password. Default: PdbBranch1_
  ORACLE_FREE_STARTUP_TIMEOUT_SECONDS   Startup wait timeout. Default: 1800
  PDB_BRANCH_REMOVE_ORACLE              Remove container after tests when set to 1.
  PDB_BRANCH_RECREATE_ORACLE            Remove existing container before tests when set to 1.
  PDB_BRANCH_TEST_SNAPSHOT_COPY         Also run the snapshot-copy case; Oracle Free falls back to full clones.
USAGE
}

oracle_free_usage_python() {
    cat <<'USAGE'
  PDB_BRANCH_TEST_VENV                  Python helper venv path. Default: .venv-integration
  PYTHON                                Python interpreter. Default: python3
USAGE
}

oracle_free_check_runtime() {
    if ! command -v "$RUNTIME" >/dev/null 2>&1; then
        echo "error: $RUNTIME is not installed or not on PATH" >&2
        exit 127
    fi
}

oracle_free_register_cleanup() {
    trap oracle_free_cleanup EXIT
}

oracle_free_cleanup() {
    if [[ "$REMOVE_CONTAINER" == "1" ]]; then
        "$RUNTIME" rm -f "$CONTAINER" >/dev/null 2>&1 || true
    fi
}

oracle_free_container_exists() {
    "$RUNTIME" inspect "$CONTAINER" >/dev/null 2>&1
}

oracle_free_container_running() {
    [[ "$("$RUNTIME" inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" == "true" ]]
}

oracle_free_ensure_container() {
    if [[ "$RECREATE_CONTAINER" == "1" ]]; then
        "$RUNTIME" rm -f "$CONTAINER" >/dev/null 2>&1 || true
    fi

    if oracle_free_container_exists; then
        if ! oracle_free_container_running; then
            printf 'starting existing Oracle Free container %s\n' "$CONTAINER"
            "$RUNTIME" start "$CONTAINER" >/dev/null
        else
            printf 'using existing Oracle Free container %s\n' "$CONTAINER"
        fi
    else
        printf 'creating Oracle Free container %s from %s\n' "$CONTAINER" "$IMAGE"
        "$RUNTIME" run \
            --detach \
            --name "$CONTAINER" \
            --publish "${PORT}:1521" \
            --env "ORACLE_PWD=${ORACLE_PASSWORD}" \
            "$IMAGE" >/dev/null
    fi

    oracle_free_wait_ready
}

oracle_free_wait_ready() {
    local deadline
    local health

    deadline=$((SECONDS + STARTUP_TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        health="$("$RUNTIME" inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$CONTAINER" 2>/dev/null || true)"
        if [[ "$health" == "healthy" ]]; then
            break
        fi

        if "$RUNTIME" logs "$CONTAINER" 2>&1 | grep -q "DATABASE IS READY TO USE"; then
            break
        fi

        printf 'waiting for Oracle Free container %s, current status: %s\n' "$CONTAINER" "${health:-unknown}"
        sleep 15
    done

    health="$("$RUNTIME" inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$CONTAINER" 2>/dev/null || true)"
    if [[ "$health" != "healthy" ]] && ! "$RUNTIME" logs "$CONTAINER" 2>&1 | grep -q "DATABASE IS READY TO USE"; then
        "$RUNTIME" logs --tail 200 "$CONTAINER" >&2 || true
        echo "error: Oracle Free container did not become ready within ${STARTUP_TIMEOUT_SECONDS}s" >&2
        exit 1
    fi
}

oracle_free_ensure_python_venv() {
    if [[ ! -x "$VENV_DIR/bin/python" ]]; then
        "$PYTHON" -m venv "$VENV_DIR"
    fi
}

oracle_free_upgrade_pip() {
    "$VENV_DIR/bin/python" -m pip install --upgrade pip
}

oracle_free_install_python_binding() {
    oracle_free_upgrade_pip
    "$VENV_DIR/bin/python" -m pip install -e "$ROOT_DIR/bindings/python[dev]"
}
