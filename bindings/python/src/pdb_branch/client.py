from __future__ import annotations

import warnings
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime
from typing import Any, Iterator, Mapping, Optional, Sequence, Union

from .installer import ensure_installed


class SnapshotCopyFallbackWarning(RuntimeWarning):
    """Warning emitted when a requested snapshot copy is created as a full clone."""


@dataclass(frozen=True)
class BranchInfo:
    branch_name: str
    parent_pdb: Optional[str]
    status: str
    profile_name: Optional[str]
    created_at: Optional[datetime]
    opened_at: Optional[datetime]
    closed_at: Optional[datetime]
    dropped_at: Optional[datetime]
    last_activity_at: Optional[datetime]
    expires_at: Optional[datetime]
    score: Optional[float]
    notes: Optional[str]

    @classmethod
    def from_row(cls, row: Mapping[str, Any]) -> "BranchInfo":
        return cls(
            branch_name=row["BRANCH_NAME"],
            parent_pdb=row.get("PARENT_PDB"),
            status=row["STATUS"],
            profile_name=row.get("PROFILE_NAME"),
            created_at=row.get("CREATED_AT"),
            opened_at=row.get("OPENED_AT"),
            closed_at=row.get("CLOSED_AT"),
            dropped_at=row.get("DROPPED_AT"),
            last_activity_at=row.get("LAST_ACTIVITY_AT"),
            expires_at=row.get("EXPIRES_AT"),
            score=row.get("SCORE"),
            notes=row.get("NOTES"),
        )


class BranchClient:
    """Thin Python wrapper over the installed ``PDB_BRANCH`` PL/SQL package.

    The connection must point at ``CDB$ROOT`` and use an account with privileges
    to create/drop/open/close PDBs. Resource Manager configuration additionally
    needs ``ADMINISTER_RESOURCE_MANAGER``.
    """

    def __init__(self, connection: Any, *, install: bool = True) -> None:
        self.connection = connection
        if install:
            ensure_installed(connection)

    @classmethod
    def connect(
        cls,
        *,
        user: str,
        password: str,
        dsn: str,
        install: bool = True,
        **kwargs: Any,
    ) -> "BranchClient":
        import oracledb

        return cls(oracledb.connect(user=user, password=password, dsn=dsn, **kwargs), install=install)

    @classmethod
    def from_pool(cls, pool: Any, *, install: bool = True) -> "BranchClient":
        return cls(pool.acquire(), install=install)

    def close(self) -> None:
        self.connection.close()

    @contextmanager
    def cursor(self) -> Iterator[Any]:
        cursor = self.connection.cursor()
        try:
            yield cursor
        finally:
            cursor.close()

    def create_branch(
        self,
        branch_name: str,
        *,
        from_pdb: str = "GOLDEN_MASTER",
        snapshot_copy: bool = True,
        open_branch: bool = True,
        profile_name: Optional[str] = None,
        expires_at: Optional[datetime] = None,
        notes: Optional[str] = None,
    ) -> None:
        last_event_id = self._max_event_id(branch_name) if snapshot_copy else None
        with self.cursor() as cur:
            cur.callproc(
                "pdb_branch.create_branch",
                [
                    branch_name,
                    from_pdb,
                    _yn(snapshot_copy),
                    _yn(open_branch),
                    profile_name,
                    expires_at,
                    notes,
                ],
            )
        if snapshot_copy:
            self._warn_if_snapshot_fell_back(branch_name, last_event_id)

    def open_branch(self, branch_name: str, *, profile_name: Optional[str] = None) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.open_branch", [branch_name, profile_name])

    def close_branch(self, branch_name: str, *, immediate: bool = True) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.close_branch", [branch_name, _yn(immediate)])

    def drop_branch(self, branch_name: str, *, including_datafiles: bool = True) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.drop_branch", [branch_name, _yn(including_datafiles)])

    def set_profile(self, branch_name: str, profile_name: str, *, reopen: bool = True) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.set_profile", [branch_name, profile_name, _yn(reopen)])

    def record_activity(self, branch_name: str, *, status: Optional[str] = None) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.record_activity", [branch_name, status])

    def record_score(self, branch_name: str, score: float, *, notes: Optional[str] = None) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.record_score", [branch_name, score, notes])

    def promote(self, branch_name: str, *, notes: Optional[str] = None) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.promote_branch", [branch_name, notes])

    def cleanup(
        self,
        *,
        close_idle_after_minutes: Optional[int] = 60,
        drop_expired: bool = True,
    ) -> None:
        with self.cursor() as cur:
            cur.callproc(
                "pdb_branch.cleanup",
                [close_idle_after_minutes, _yn(drop_expired)],
            )

    def configure_resource_plan(
        self,
        *,
        plan_name: str = "PDB_BRANCH_PLAN",
        activate: bool = False,
    ) -> None:
        with self.cursor() as cur:
            cur.callproc("pdb_branch.configure_resource_plan", [plan_name, _yn(activate)])

    def get_branch(self, branch_name: str) -> Optional[BranchInfo]:
        rows = self._query(
            """
            SELECT branch_name,
                   parent_pdb,
                   status,
                   profile_name,
                   created_at,
                   opened_at,
                   closed_at,
                   dropped_at,
                   last_activity_at,
                   expires_at,
                   score,
                   notes
              FROM pdb_branch_branches
             WHERE branch_name = UPPER(:branch_name)
            """,
            {"branch_name": branch_name},
        )
        return BranchInfo.from_row(rows[0]) if rows else None

    def list_branches(self, *, include_dropped: bool = False) -> list[BranchInfo]:
        where = "" if include_dropped else "WHERE status <> 'DROPPED'"
        rows = self._query(
            f"""
            SELECT branch_name,
                   parent_pdb,
                   status,
                   profile_name,
                   created_at,
                   opened_at,
                   closed_at,
                   dropped_at,
                   last_activity_at,
                   expires_at,
                   score,
                   notes
              FROM pdb_branch_branches
              {where}
             ORDER BY created_at DESC
            """
        )
        return [BranchInfo.from_row(row) for row in rows]

    def _query(
        self,
        sql: str,
        parameters: Optional[Union[Mapping[str, Any], Sequence[Any]]] = None,
    ) -> list[dict[str, Any]]:
        with self.cursor() as cur:
            cur.execute(sql, parameters or {})
            columns = [col[0] for col in cur.description]
            return [dict(zip(columns, row)) for row in cur.fetchall()]

    def _max_event_id(self, branch_name: str) -> Optional[int]:
        with self.cursor() as cur:
            cur.execute(
                """
                SELECT MAX(event_id)
                  FROM pdb_branch_events
                 WHERE branch_name = UPPER(:branch_name)
                """,
                {"branch_name": branch_name},
            )
            row = cur.fetchone()
        return row[0] if row and row[0] is not None else None

    def _warn_if_snapshot_fell_back(
        self,
        branch_name: str,
        last_event_id: Optional[int],
    ) -> None:
        with self.cursor() as cur:
            cur.execute(
                """
                SELECT details
                  FROM (
                        SELECT details
                          FROM pdb_branch_events
                         WHERE branch_name = UPPER(:branch_name)
                           AND event_type = 'SNAPSHOT_COPY_FALLBACK'
                           AND (:last_event_id IS NULL OR event_id > :last_event_id)
                         ORDER BY event_id DESC
                       )
                 WHERE ROWNUM = 1
                """,
                {"branch_name": branch_name, "last_event_id": last_event_id},
            )
            row = cur.fetchone()

        if row:
            warnings.warn(
                _read_lob(row[0]),
                SnapshotCopyFallbackWarning,
                stacklevel=3,
            )


def connect(*, install: bool = True, **kwargs: Any) -> BranchClient:
    return BranchClient.connect(install=install, **kwargs)


def _yn(value: bool) -> str:
    return "Y" if value else "N"


def _read_lob(value: Any) -> str:
    if hasattr(value, "read"):
        return str(value.read())
    return str(value)
