# pdb-branch

`pdb-branch` is a small Python + PL/SQL library for making Oracle PDB snapshot
copies feel like cheap database branches for agentic workflow experiments.

The Python layer installs or upgrades the PL/SQL package at startup. After that,
branch lifecycle operations go through a stable database-side API.

## Shape

- `PDB_BRANCH` PL/SQL package
- `PDB_BRANCH_BRANCHES`, `PDB_BRANCH_EVENTS`, and `PDB_BRANCH_PROFILES` control tables
- Python `BranchClient` wrapper
- Optional Resource Manager profile setup for `PDB_BRANCH_ACTIVE`,
  `PDB_BRANCH_IDLE`, and `PDB_BRANCH_BACKGROUND`

## Prerequisites

Connect to `CDB$ROOT` as a user that can create, open, close, and drop PDBs.
Resource Manager setup additionally needs Resource Manager administration
privileges.

Snapshot copy behavior depends on the Oracle environment and storage
configuration. This library assumes the branch can be created with:

```sql
CREATE PLUGGABLE DATABASE branch_name FROM parent_pdb SNAPSHOT COPY
```

The first version intentionally avoids `FILE_NAME_CONVERT` and custom storage
clauses. Use Oracle Managed Files or a platform where the simple snapshot-copy
form is valid.

## Python Usage

```python
import oracledb
from pdb_branch import BranchClient

connection = oracledb.connect(
    user="sys",
    password="...",
    dsn="localhost/FREE",
    mode=oracledb.AUTH_MODE_SYSDBA,
)

client = BranchClient(connection)  # installs/upgrades PL/SQL on startup

client.create_branch(
    "AGENT_RAG_042",
    from_pdb="GOLDEN_MASTER",
    notes="try smaller chunk size and rerank before answer synthesis",
)

client.record_score("AGENT_RAG_042", 0.91, notes="eval: qa_regression_v3")
client.promote("AGENT_RAG_042", notes="winner for current retrieval policy")
```

## Optional Resource Profiles

The base branch lifecycle API does not require Resource Manager profiles. If the
target database supports CDB profile directives and `DB_PERFORMANCE_PROFILE`,
configure and activate the default plan explicitly:

```python
client.configure_resource_plan(activate=True)
client.set_profile("AGENT_RAG_042", "PDB_BRANCH_ACTIVE")
```

Use `PDB_BRANCH_IDLE` or `PDB_BRANCH_BACKGROUND` for lower-priority branches.

## Maintenance

Branches can have an expiration timestamp and can be closed after inactivity:

```python
client.cleanup(close_idle_after_minutes=60, drop_expired=True)
```

In production, call `PDB_BRANCH.CLEANUP` from `DBMS_SCHEDULER` or a small
control-plane service.

## Current Boundaries

- The Python installer is idempotent, but it does not migrate destructive schema
  changes yet.
- PL/SQL identifiers are intentionally restricted to simple unquoted Oracle
  names.
- Promotion is metadata-only in v1; scaling or export workflows should be added
  as deployment-specific adapters.
- Local tests do not create real PDBs. Run integration tests against a CDB.
