from typing import Optional

import pytest

from pdb_branch import BranchClient, SnapshotCopyFallbackWarning


class FakeCursor:
    description = [
        ("BRANCH_NAME",),
        ("PARENT_PDB",),
        ("STATUS",),
        ("PROFILE_NAME",),
        ("CREATED_AT",),
        ("OPENED_AT",),
        ("CLOSED_AT",),
        ("DROPPED_AT",),
        ("LAST_ACTIVITY_AT",),
        ("EXPIRES_AT",),
        ("SCORE",),
        ("NOTES",),
    ]

    def __init__(self, fallback_details: Optional[str] = None) -> None:
        self.calls: list[tuple[str, list[object]]] = []
        self.executions: list[tuple[str, object]] = []
        self.fallback_details = fallback_details
        self.last_sql = ""

    def callproc(self, name: str, args: list[object]) -> None:
        self.calls.append((name, args))

    def execute(self, sql: str, parameters: object = None) -> None:
        self.executions.append((sql, parameters))
        self.last_sql = sql

    def fetchall(self) -> list[tuple[object, ...]]:
        return [
            (
                "AGENT_RAG_042",
                "GOLDEN_MASTER",
                "OPEN",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                0.91,
                "winner",
            )
        ]

    def fetchone(self) -> Optional[tuple[object, ...]]:
        if "MAX(event_id)" in self.last_sql:
            return (41,)
        if "SNAPSHOT_COPY_FALLBACK" in self.last_sql and self.fallback_details is not None:
            return (self.fallback_details,)
        return None

    def close(self) -> None:
        pass


class FakeConnection:
    def __init__(self, fallback_details: Optional[str] = None) -> None:
        self.cursor_obj = FakeCursor(fallback_details)

    def cursor(self) -> FakeCursor:
        return self.cursor_obj

    def close(self) -> None:
        pass


def test_create_branch_calls_plsql_api() -> None:
    connection = FakeConnection()
    client = BranchClient(connection, install=False)

    client.create_branch("agent_rag_042", from_pdb="golden_master", notes="try chunking")

    assert connection.cursor_obj.calls == [
        (
            "pdb_branch.create_branch",
            [
                "agent_rag_042",
                "golden_master",
                "Y",
                "Y",
                None,
                None,
                "try chunking",
            ],
        )
    ]
    assert len(connection.cursor_obj.executions) == 2


def test_create_branch_warns_when_snapshot_falls_back() -> None:
    connection = FakeConnection(
        "WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone"
    )
    client = BranchClient(connection, install=False)

    with pytest.warns(SnapshotCopyFallbackWarning, match="created with full clone"):
        client.create_branch("agent_rag_042")


def test_get_branch_maps_row_to_dataclass() -> None:
    connection = FakeConnection()
    client = BranchClient(connection, install=False)

    branch = client.get_branch("agent_rag_042")

    assert branch is not None
    assert branch.branch_name == "AGENT_RAG_042"
    assert branch.parent_pdb == "GOLDEN_MASTER"
    assert branch.status == "OPEN"
    assert branch.score == 0.91
