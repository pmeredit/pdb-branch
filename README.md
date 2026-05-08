# pdb-branch

`pdb-branch` is a small Python + PL/SQL library for making Oracle PDB snapshot
copies feel like cheap database branches for agentic workflow experiments.

The Python layer installs or upgrades the PL/SQL package at startup. After that,
branch lifecycle operations go through a stable database-side API.

## Oracle Version Support Disclaimer

This library is intended for Oracle Database environments where you can connect
to `CDB$ROOT` and run PDB lifecycle DDL such as `CREATE PLUGGABLE DATABASE`,
`ALTER PLUGGABLE DATABASE`, and `DROP PLUGGABLE DATABASE`.

Expected support:

- **Best target:** Oracle Database / Oracle AI Database **19c or newer** in a
  self-managed CDB where PDB cloning and Snapshot Copy PDBs are available.
- **Primary development target:** Oracle AI Database **23ai / 26ai** CDBs.
- **Oracle Free Docker image:** suitable for local development, smoke tests, and
  small demonstrations. The Free image creates a CDB service named `FREE` and a
  default PDB service named `FREEPDB1`; connect to `FREE` for library install and
  branch management. Oracle Free is resource-limited and unsupported, so it is
  not a realistic high-density branch platform.
- **Oracle Free Lite Docker image:** may work for the basic PL/SQL installer and
  simple PDB DDL, but use the **Full** Free image for compatibility testing.
- **Public Autonomous Database Serverless / Always Free:** not a v1 target. ADB
  application connections normally land in an existing PDB, not in a customer-
  managed `CDB$ROOT`, so they generally cannot run this library's PDB branch DDL.

Snapshot-copy support is not just a database-version question. The source PDB,
database parameters, and underlying storage must satisfy Oracle's Snapshot Copy
PDB requirements. If storage snapshots are unavailable, use ordinary PDB clones
by passing `snapshot_copy=False`, but those clones will not have the cheap
copy-on-write behavior this project is designed around.

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
