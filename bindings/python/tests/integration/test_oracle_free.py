from __future__ import annotations

import os
import re
import time
import uuid
from contextlib import suppress
from typing import Any

import pytest

from pdb_branch import BranchClient

pytestmark = pytest.mark.integration

if os.getenv("PDB_BRANCH_INTEGRATION") != "1":
    pytest.skip(
        "set PDB_BRANCH_INTEGRATION=1 to run Oracle integration tests",
        allow_module_level=True,
    )


oracledb = pytest.importorskip("oracledb")

NAME_RE = re.compile(r"^[A-Z][A-Z0-9_$#]{0,29}$")


@pytest.mark.parametrize(
    "snapshot_copy",
    [
        pytest.param(False, id="full-clone"),
        pytest.param(
            True,
            marks=pytest.mark.skipif(
                os.getenv("PDB_BRANCH_TEST_SNAPSHOT_COPY") != "1",
                reason="set PDB_BRANCH_TEST_SNAPSHOT_COPY=1 to exercise SNAPSHOT COPY",
            ),
            id="snapshot-copy",
        ),
    ],
)
def test_oracle_free_branch_lifecycle(snapshot_copy: bool) -> None:
    root = connect_root()
    branch_name = make_branch_name("PBISC" if snapshot_copy else "PBIFC")
    parent_pdb = simple_name(os.getenv("PDB_BRANCH_PARENT_PDB", "FREEPDB1"), "parent PDB")
    app_user = simple_name(os.getenv("PDB_BRANCH_APP_USER", "PDB_BRANCH_APP"), "app user")
    app_password = os.getenv("PDB_BRANCH_APP_PASSWORD", "PdbBranch1_")

    client = BranchClient(root)

    try:
        prepare_parent_pdb(root, parent_pdb, app_user, app_password)

        client.create_branch(
            branch_name,
            from_pdb=parent_pdb,
            snapshot_copy=snapshot_copy,
            notes="oracle free integration test",
        )

        branch = client.get_branch(branch_name)
        assert branch is not None
        assert branch.branch_name == branch_name
        assert branch.parent_pdb == parent_pdb
        assert branch.status == "OPEN"

        if snapshot_copy:
            mutate_parent_after_branch_create(root, parent_pdb, app_user)

        branch_conn = connect_workload(branch_name, app_user, app_password)
        try:
            assert (
                scalar(branch_conn, "SELECT COUNT(*) FROM pdb_branch_seed") == 1
            ), "branch should preserve parent seed state at branch creation time"
            execute(
                branch_conn,
                "INSERT INTO experiment_log(event) VALUES (:1)",
                ["agent wrote to branch"],
            )
            branch_conn.commit()
            assert scalar(branch_conn, "SELECT COUNT(*) FROM experiment_log") == 1
        finally:
            branch_conn.close()

        client.record_score(branch_name, 0.99, notes="integration test passed")
        scored = client.get_branch(branch_name)
        assert scored is not None
        assert scored.score == 0.99
    finally:
        with suppress(Exception):
            client.drop_branch(branch_name)
        with suppress(Exception):
            reopen_parent_read_write(root, parent_pdb)
        root.close()


def connect_root() -> Any:
    dsn = os.getenv("PDB_BRANCH_ROOT_DSN", "localhost:1521/FREE")
    user = os.getenv("PDB_BRANCH_SYS_USER", "sys")
    password = os.getenv("PDB_BRANCH_SYS_PASSWORD", os.getenv("ORACLE_PWD", "PdbBranch1_"))
    return oracledb.connect(
        user=user,
        password=password,
        dsn=dsn,
        mode=oracledb.AUTH_MODE_SYSDBA,
    )


def connect_workload(branch_name: str, user: str, password: str) -> Any:
    template = os.getenv("PDB_BRANCH_BRANCH_DSN_TEMPLATE", "localhost:1521/{branch_name}")
    dsn = template.format(branch_name=branch_name)
    timeout_seconds = int(os.getenv("PDB_BRANCH_SERVICE_TIMEOUT_SECONDS", "120"))
    deadline = time.monotonic() + timeout_seconds
    last_error: BaseException | None = None

    while time.monotonic() < deadline:
        try:
            return oracledb.connect(user=user, password=password, dsn=dsn)
        except oracledb.DatabaseError as exc:
            last_error = exc
            time.sleep(5)

    if last_error is not None:
        raise last_error
    raise RuntimeError(f"timed out waiting for branch service {dsn}")


def prepare_parent_pdb(root: Any, parent_pdb: str, app_user: str, app_password: str) -> None:
    reopen_parent_read_write(root, parent_pdb)

    execute(root, f"ALTER SESSION SET CONTAINER = {parent_pdb}")
    try:
        execute_ignore(root, f"DROP USER {app_user} CASCADE", {1918})
        execute(root, f'CREATE USER {app_user} IDENTIFIED BY "{escape_quoted(app_password)}"')
        execute(root, f"ALTER USER {app_user} QUOTA UNLIMITED ON USERS")
        execute(root, f"GRANT CREATE SESSION, CREATE TABLE TO {app_user}")
        execute(
            root,
            f"""
            CREATE TABLE {app_user}.pdb_branch_seed (
                id NUMBER PRIMARY KEY,
                label VARCHAR2(100) NOT NULL
            )
            """,
        )
        execute(
            root,
            f"""
            CREATE TABLE {app_user}.experiment_log (
                event VARCHAR2(100) NOT NULL,
                created_at TIMESTAMP DEFAULT SYSTIMESTAMP NOT NULL
            )
            """,
        )
        execute(
            root,
            f"INSERT INTO {app_user}.pdb_branch_seed(id, label) VALUES (1, 'seed row')",
        )
        root.commit()
    finally:
        execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT")

    close_pdb(root, parent_pdb)
    execute(root, f"ALTER PLUGGABLE DATABASE {parent_pdb} OPEN READ ONLY")


def mutate_parent_after_branch_create(root: Any, parent_pdb: str, app_user: str) -> None:
    reopen_parent_read_write(root, parent_pdb)
    execute(root, f"ALTER SESSION SET CONTAINER = {parent_pdb}")
    try:
        execute(
            root,
            f"INSERT INTO {app_user}.pdb_branch_seed(id, label) VALUES (2, 'parent mutation')",
        )
        root.commit()
        assert scalar(root, f"SELECT COUNT(*) FROM {app_user}.pdb_branch_seed") == 2
    finally:
        execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT")


def reopen_parent_read_write(root: Any, parent_pdb: str) -> None:
    execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT")
    close_pdb(root, parent_pdb)
    execute(root, f"ALTER PLUGGABLE DATABASE {parent_pdb} OPEN READ WRITE")


def close_pdb(root: Any, pdb_name: str) -> None:
    execute_ignore(root, f"ALTER PLUGGABLE DATABASE {pdb_name} CLOSE IMMEDIATE", {65020})


def execute(connection: Any, sql: str, parameters: list[Any] | None = None) -> None:
    cursor = connection.cursor()
    try:
        cursor.execute(sql, parameters or [])
    finally:
        cursor.close()


def execute_ignore(connection: Any, sql: str, ignored_codes: set[int]) -> None:
    try:
        execute(connection, sql)
    except oracledb.DatabaseError as exc:
        if error_code(exc) not in ignored_codes:
            raise


def scalar(connection: Any, sql: str) -> Any:
    cursor = connection.cursor()
    try:
        cursor.execute(sql)
        row = cursor.fetchone()
        assert row is not None
        return row[0]
    finally:
        cursor.close()


def error_code(exc: BaseException) -> int | None:
    if not getattr(exc, "args", None):
        return None
    error = exc.args[0]
    return getattr(error, "code", None)


def make_branch_name(prefix: str) -> str:
    return simple_name(f"{prefix}{uuid.uuid4().hex[:8].upper()}", "branch name")


def simple_name(value: str, label: str) -> str:
    name = value.strip().upper()
    if not NAME_RE.match(name):
        raise ValueError(f"{label} must be an unquoted Oracle identifier of 30 chars or fewer")
    return name


def escape_quoted(value: str) -> str:
    return value.replace('"', '""')
