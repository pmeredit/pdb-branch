from __future__ import annotations

from importlib import resources
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


def run_script(connection: Any, script_name: str) -> None:
    script = resources.files("pdb_branch.sql").joinpath(script_name).read_text(encoding="utf-8")
    cursor = connection.cursor()
    try:
        for statement in split_sqlplus_script(script):
            cursor.execute(statement)
    finally:
        cursor.close()
    connection.commit()
