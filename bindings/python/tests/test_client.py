from __future__ import annotations

from pdb_branch import BranchClient


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

    def __init__(self) -> None:
        self.calls: list[tuple[str, list[object]]] = []
        self.executions: list[tuple[str, object]] = []

    def callproc(self, name: str, args: list[object]) -> None:
        self.calls.append((name, args))

    def execute(self, sql: str, parameters: object = None) -> None:
        self.executions.append((sql, parameters))

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

    def close(self) -> None:
        pass


class FakeConnection:
    def __init__(self) -> None:
        self.cursor_obj = FakeCursor()

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


def test_get_branch_maps_row_to_dataclass() -> None:
    connection = FakeConnection()
    client = BranchClient(connection, install=False)

    branch = client.get_branch("agent_rag_042")

    assert branch is not None
    assert branch.branch_name == "AGENT_RAG_042"
    assert branch.parent_pdb == "GOLDEN_MASTER"
    assert branch.status == "OPEN"
    assert branch.score == 0.91
