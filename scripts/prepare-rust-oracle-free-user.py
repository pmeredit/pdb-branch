#!/usr/bin/env python3
from __future__ import annotations

import os
import re
from typing import Any

import oracledb


NAME_RE = re.compile(r"^[A-Z][A-Z0-9_$#]{0,29}$")


def main() -> None:
    dsn = os.getenv("PDB_BRANCH_ROOT_DSN", "localhost:1521/FREE")
    sys_user = os.getenv("PDB_BRANCH_SYS_USER", "sys")
    sys_password = os.getenv("PDB_BRANCH_SYS_PASSWORD", os.getenv("ORACLE_PWD", "PdbBranch1_"))
    admin_user = simple_name(
        os.getenv("PDB_BRANCH_RUST_ROOT_USER", "C##PDB_BRANCH_RUST"),
        "Rust integration user",
    )
    admin_password = os.getenv(
        "PDB_BRANCH_RUST_ROOT_PASSWORD",
        os.getenv("PDB_BRANCH_SYS_PASSWORD", os.getenv("ORACLE_PWD", "PdbBranch1_")),
    )

    connection = oracledb.connect(
        user=sys_user,
        password=sys_password,
        dsn=dsn,
        mode=oracledb.AUTH_MODE_SYSDBA,
    )
    try:
        if scalar(
            connection,
            "SELECT COUNT(*) FROM dba_users WHERE username = :1",
            [admin_user],
        ):
            execute(
                connection,
                f'ALTER USER {admin_user} IDENTIFIED BY "{escape_quoted(admin_password)}" '
                "ACCOUNT UNLOCK CONTAINER=ALL",
            )
        else:
            execute(
                connection,
                f'CREATE USER {admin_user} IDENTIFIED BY "{escape_quoted(admin_password)}" '
                "CONTAINER=ALL",
            )

        execute_ignore(
            connection,
            f"ALTER USER {admin_user} QUOTA UNLIMITED ON USERS",
            {959},
        )
        execute(
            connection,
            f"""
            GRANT CREATE SESSION,
                  SET CONTAINER,
                  CREATE TABLE,
                  CREATE SEQUENCE,
                  CREATE PROCEDURE,
                  CREATE USER,
                  ALTER USER,
                  DROP USER,
                  GRANT ANY PRIVILEGE,
                  CREATE PLUGGABLE DATABASE,
                  ALTER PLUGGABLE DATABASE,
                  DROP PLUGGABLE DATABASE,
                  SELECT ANY DICTIONARY,
                  SELECT ANY TABLE,
                  UNLIMITED TABLESPACE
              TO {admin_user}
              CONTAINER=ALL
            """,
        )
        execute(connection, f"GRANT DBA TO {admin_user} CONTAINER=ALL")

        for object_name in [
            "SYS.V_$DATABASE",
            "SYS.V_$PARAMETER",
            "SYS.V_$PDBS",
            "SYS.V_$VERSION",
            "SYS.CDB_DATA_FILES",
        ]:
            execute_ignore(
                connection,
                f"GRANT SELECT ON {object_name} TO {admin_user}",
                {942, 1917},
            )

        connection.commit()
        print(f"prepared Rust integration user {admin_user}")
    finally:
        connection.close()


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
    cursor = connection.cursor()
    try:
        cursor.execute(sql, parameters or [])
        row = cursor.fetchone()
        return row[0] if row else None
    finally:
        cursor.close()


def error_code(exc: BaseException) -> int | None:
    if not getattr(exc, "args", None):
        return None
    error = exc.args[0]
    return getattr(error, "code", None)


def simple_name(value: str, label: str) -> str:
    name = value.strip().upper()
    if not NAME_RE.match(name):
        raise ValueError(f"{label} must be an unquoted Oracle identifier of 30 chars or fewer")
    return name


def escape_quoted(value: str) -> str:
    return value.replace('"', '""')


if __name__ == "__main__":
    main()
