from __future__ import annotations

from importlib import resources
from pathlib import Path
from typing import Any

from .sqlsplit import split_sqlplus_script


SQL_SCRIPTS = (
    "001_tables.sql",
    "002_package.sql",
)


def ensure_installed(connection: Any) -> None:
    """Install or upgrade the PDB_BRANCH database objects.

    This is intentionally idempotent: tables are created only if absent, while
    the package spec/body are replaced on every startup so the Python package and
    PL/SQL API stay in lockstep.
    """

    for name in SQL_SCRIPTS:
        run_script(connection, name)
    verify_installed(connection)


def run_script(connection: Any, script_name: str) -> None:
    script = read_script(script_name)
    cursor = connection.cursor()
    try:
        for statement in split_sqlplus_script(script):
            cursor.execute(statement)
    finally:
        cursor.close()
    connection.commit()


def verify_installed(connection: Any) -> None:
    """Fail fast if Oracle compiled the PL/SQL package with errors."""

    cursor = connection.cursor()
    try:
        cursor.execute(
            """
            SELECT object_type
              FROM user_objects
             WHERE object_name = 'PDB_BRANCH'
               AND object_type IN ('PACKAGE', 'PACKAGE BODY')
               AND status <> 'VALID'
             ORDER BY object_type
            """
        )
        invalid_objects = [row[0] for row in cursor.fetchall()]
        if not invalid_objects:
            return

        cursor.execute(
            """
            SELECT type, line, position, text
              FROM user_errors
             WHERE name = 'PDB_BRANCH'
             ORDER BY sequence
            """
        )
        errors = [
            f"{row[0]} line {row[1]}, position {row[2]}: {row[3]}"
            for row in cursor.fetchall()
        ]
    finally:
        cursor.close()

    details = "\n".join(errors) if errors else "no compiler diagnostics returned"
    raise RuntimeError(
        "PDB_BRANCH PL/SQL package failed to compile: "
        f"{', '.join(invalid_objects)}\n{details}"
    )


def read_script(script_name: str) -> str:
    try:
        return resources.files("pdb_branch.sql").joinpath(script_name).read_text(encoding="utf-8")
    except (FileNotFoundError, ModuleNotFoundError):
        repo_sql = Path(__file__).resolve().parents[4] / "sql" / script_name
        return repo_sql.read_text(encoding="utf-8")
