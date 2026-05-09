from __future__ import annotations

import os
import re
import time
import uuid
from contextlib import suppress
from dataclasses import dataclass
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
SNAPSHOT_COPY_UNSUPPORTED_CODES = {17525, 65169}


@dataclass(frozen=True)
class DatabaseFacts:
    dsn: str
    banner: str
    cdb: str
    con_name: str
    db_create_file_dest: str | None
    pdb_file_name_convert: str | None
    parent_pdb: str
    parent_open_mode: str | None
    parent_restricted: str | None


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
    client: BranchClient | None = None

    try:
        require_cdb_root(root, parent_pdb)
        client = BranchClient(root)
        prepare_parent_pdb(root, parent_pdb, app_user, app_password)

        try:
            client.create_branch(
                branch_name,
                from_pdb=parent_pdb,
                snapshot_copy=snapshot_copy,
                notes="oracle free integration test",
            )
        except oracledb.DatabaseError as exc:
            facts = collect_database_facts(root, parent_pdb)
            if snapshot_copy and snapshot_copy_unsupported(exc):
                pytest.skip(
                    "SNAPSHOT COPY is not supported by this Oracle storage backend:\n"
                    f"{format_database_facts(facts)}\n"
                    f"error: {exc}"
                )
            pytest.fail(
                "create_branch failed against Oracle database:\n"
                f"{format_database_facts(facts)}\n"
                f"error: {exc}",
                pytrace=False,
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
        if client is not None:
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


def require_cdb_root(root: Any, parent_pdb: str) -> DatabaseFacts:
    facts = collect_database_facts(root, parent_pdb)
    problems = []
    if facts.cdb != "YES":
        problems.append("database is not a CDB")
    if facts.con_name != "CDB$ROOT":
        problems.append(f"connection is in {facts.con_name}, not CDB$ROOT")
    if facts.parent_open_mode is None:
        problems.append(f"parent PDB {parent_pdb} was not found")

    if problems:
        pytest.fail(
            "Oracle integration tests require a CDB root connection: "
            f"{'; '.join(problems)}\n{format_database_facts(facts)}",
            pytrace=False,
        )

    return facts


def collect_database_facts(root: Any, parent_pdb: str) -> DatabaseFacts:
    params = {
        row[0]: row[1]
        for row in rows(
            root,
            """
            SELECT name, value
              FROM v$parameter
             WHERE name IN ('db_create_file_dest', 'pdb_file_name_convert')
            """,
        )
    }
    parent = rows(
        root,
        """
        SELECT open_mode, restricted
          FROM v$pdbs
         WHERE name = :1
        """,
        [parent_pdb],
    )
    parent_open_mode = parent[0][0] if parent else None
    parent_restricted = parent[0][1] if parent else None

    return DatabaseFacts(
        dsn=os.getenv("PDB_BRANCH_ROOT_DSN", "localhost:1521/FREE"),
        banner=scalar(root, "SELECT banner FROM v$version WHERE ROWNUM = 1"),
        cdb=scalar(root, "SELECT cdb FROM v$database"),
        con_name=scalar(root, "SELECT SYS_CONTEXT('USERENV', 'CON_NAME') FROM dual"),
        db_create_file_dest=params.get("db_create_file_dest"),
        pdb_file_name_convert=params.get("pdb_file_name_convert"),
        parent_pdb=parent_pdb,
        parent_open_mode=parent_open_mode,
        parent_restricted=parent_restricted,
    )


def format_database_facts(facts: DatabaseFacts) -> str:
    return "\n".join(
        [
            f"  dsn: {facts.dsn}",
            f"  banner: {facts.banner}",
            f"  cdb: {facts.cdb}",
            f"  container: {facts.con_name}",
            f"  db_create_file_dest: {facts.db_create_file_dest or '(unset)'}",
            f"  pdb_file_name_convert: {facts.pdb_file_name_convert or '(unset)'}",
            f"  parent_pdb: {facts.parent_pdb}",
            f"  parent_open_mode: {facts.parent_open_mode or '(missing)'}",
            f"  parent_restricted: {facts.parent_restricted or '(missing)'}",
        ]
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


def scalar(connection: Any, sql: str, parameters: list[Any] | None = None) -> Any:
    result = rows(connection, sql, parameters)
    assert result
    return result[0][0]


def rows(connection: Any, sql: str, parameters: list[Any] | None = None) -> list[Any]:
    cursor = connection.cursor()
    try:
        cursor.execute(sql, parameters or [])
        return cursor.fetchall()
    finally:
        cursor.close()


def error_code(exc: BaseException) -> int | None:
    if not getattr(exc, "args", None):
        return None
    error = exc.args[0]
    return getattr(error, "code", None)


def snapshot_copy_unsupported(exc: BaseException) -> bool:
    code = error_code(exc)
    if code in SNAPSHOT_COPY_UNSUPPORTED_CODES:
        return True

    message = str(exc)
    return any(f"ORA-{unsupported_code}" in message for unsupported_code in SNAPSHOT_COPY_UNSUPPORTED_CODES)


def make_branch_name(prefix: str) -> str:
    return simple_name(f"{prefix}{uuid.uuid4().hex[:8].upper()}", "branch name")


def simple_name(value: str, label: str) -> str:
    name = value.strip().upper()
    if not NAME_RE.match(name):
        raise ValueError(f"{label} must be an unquoted Oracle identifier of 30 chars or fewer")
    return name


def escape_quoted(value: str) -> str:
    return value.replace('"', '""')
