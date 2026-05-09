# pdb-branch

`pdb-branch` is a small multi-language library over a shared PL/SQL package for
making Oracle PDB snapshot copies feel like cheap database branches for agentic
workflow experiments.

Language bindings install or upgrade the PL/SQL package at startup. After that,
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
PDB requirements. When `snapshot_copy=True`, the library requests `SNAPSHOT COPY`
where it is expected to work, but falls back to ordinary full PDB clones on
Oracle Free and when Oracle reports that storage snapshots are unsupported. Full
clones preserve correctness but do not have the cheap copy-on-write behavior this
project is designed around.

## Shape

- `PDB_BRANCH` PL/SQL package
- `PDB_BRANCH_BRANCHES`, `PDB_BRANCH_EVENTS`, and `PDB_BRANCH_PROFILES` control tables
- Language-specific `BranchClient` wrappers
- Optional Resource Manager profile setup for `PDB_BRANCH_ACTIVE`,
  `PDB_BRANCH_IDLE`, and `PDB_BRANCH_BACKGROUND`

## Repository Layout

- `sql/` - shared PL/SQL install scripts
- `scripts/` - local development and integration test helpers
- `bindings/python/` - Python binding
- `bindings/node/` - Node.js binding
- `bindings/rust/` - Rust binding; defaults to `oracle-rs` and can use the
  ODPI-C based `oracle` crate with `--no-default-features --features rust-oracle`
- `bindings/java/` - Java binding

## Prerequisites

Connect to `CDB$ROOT` as a user that can create, open, close, and drop PDBs.
Resource Manager setup additionally needs Resource Manager administration
privileges.

Snapshot copy behavior depends on the Oracle environment and storage
configuration. Branch creation uses Oracle Managed Files via `CREATE_FILE_DEST`,
preferring `DB_CREATE_FILE_DEST` when it is configured and otherwise deriving a
destination from the parent PDB datafile directory:

```sql
CREATE PLUGGABLE DATABASE branch_name FROM parent_pdb SNAPSHOT COPY CREATE_FILE_DEST = '/path'
```

When the library is connected to Oracle Free, `snapshot_copy=True` is treated as
a full clone because the default Oracle Free container data directory does not
support storage snapshots. On non-Free databases, `snapshot_copy=True` attempts
`SNAPSHOT COPY` first and retries as a full clone if Oracle reports `ORA-17525`
or `ORA-65169`. Every fallback records a `SNAPSHOT_COPY_FALLBACK` event in
`PDB_BRANCH_EVENTS`; the Python binding also emits
`SnapshotCopyFallbackWarning`.

## Python Binding Usage

Install from the Python binding directory:

```bash
cd bindings/python
python -m pip install -e '.[dev]'
python -m pytest
```

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

## Agent Workflow Usage

Use two separate connections:

- **Control-plane connection:** trusted orchestration code connects to
  `CDB$ROOT` and uses `BranchClient` to create, open, close, and drop PDB
  branches.
- **Workload connection:** the agent connects directly to the branch PDB with a
  normal application user and runs ordinary SQL against branch-local data.

The agent does not need `SYS`, `CDB$ROOT`, or branch-management privileges. It
only needs a DSN for the branch PDB and normal app credentials. Users, schemas,
tables, seed data, and privileges should already exist in the parent PDB so they
are copied into each branch.

```python
import oracledb
from pdb_branch import BranchClient

# Trusted orchestration code. Do not hand this connection to the agent.
root = oracledb.connect(
    user="sys",
    password="...",
    dsn="localhost:1521/FREE",
    mode=oracledb.AUTH_MODE_SYSDBA,
)

branches = BranchClient(root)
branches.create_branch("AGENT_RAG_042", from_pdb="GOLDEN_MASTER")

# This is the connection information the agent should receive.
branch_dsn = "localhost:1521/AGENT_RAG_042"

# Normal workload connection into the branch PDB.
branch_conn = oracledb.connect(
    user="app_user",
    password="app_password",
    dsn=branch_dsn,
)

with branch_conn.cursor() as cur:
    cur.execute(
        "INSERT INTO experiment_log(branch_name, event) VALUES (:1, :2)",
        ["AGENT_RAG_042", "agent started trial"],
    )

    cur.execute("SELECT COUNT(*) FROM documents")
    document_count = cur.fetchone()[0]

branch_conn.commit()

branches.record_score(
    "AGENT_RAG_042",
    0.91,
    notes=f"agent completed eval over {document_count} documents",
)
```

Once a branch PDB is open, there is no special "branch query" mode. The branch is
just an isolated Oracle PDB service; agents run the same SQL they would run
against any other Oracle database.

## Oracle Free Integration Tests

The Python integration harness starts Oracle Database Free in a container,
installs the Python binding, installs the shared PL/SQL package into `CDB$ROOT`,
prepares `FREEPDB1` as a parent PDB, creates a branch PDB, connects to that
branch as a normal app user, writes branch-local data, records a score, and
drops the branch.

```bash
scripts/run-oracle-free-integration.sh
```

By default the harness uses Podman and the full Oracle Free image:

```bash
ORACLE_FREE_IMAGE=container-registry.oracle.com/database/free:latest \
  scripts/run-oracle-free-integration.sh
```

Useful knobs:

- `CONTAINER_RUNTIME=docker` uses Docker instead of Podman.
- `ORACLE_FREE_IMAGE=container-registry.oracle.com/database/free:latest-lite`
  uses the smaller image for smoke tests.
- `ORACLE_PWD=...` sets the `SYS` password used when starting a new container.
- `ORACLE_FREE_PORT=1522` maps the listener to a non-default host port.
- `PDB_BRANCH_TEST_VENV=...` selects the Python virtualenv path. The default is
  `.venv-integration`.
- `PDB_BRANCH_RECREATE_ORACLE=1` removes the named container before starting.
- `PDB_BRANCH_REMOVE_ORACLE=1` removes the named container after tests finish.
- `PDB_BRANCH_TEST_SNAPSHOT_COPY=1` also runs the snapshot-copy request path.

The harness keeps and reuses the named Oracle Free container by default because
database startup is expensive. It creates a local Python virtualenv for the test
dependencies instead of installing into the system Python environment.

The default test uses `snapshot_copy=False`. The snapshot-copy request path is
opt-in; on Oracle Free it still creates a full clone through the library's
fallback behavior.

Run the Rust Oracle Free integration test with its own entry script:

```bash
scripts/run-rust-oracle-free-integration.sh
```

Oracle PDB lifecycle DDL requires an administrative connection, so the live Rust
integration script runs the existing ODPI-C based `rust-oracle` backend with
SYSDBA authentication. The pure-Rust `oracle-rs` backend is still covered by the
normal Rust unit tests, but it does not currently expose SYSDBA authentication.
The live Rust script therefore needs the Oracle Client runtime expected by the
`oracle` crate.

Rust-specific knobs:

- `PDB_BRANCH_SYS_USER=...` selects the administrative CDB user. The default is
  `sys`.
- `PDB_BRANCH_SYS_PASSWORD=...` sets that user's password. The default is
  `ORACLE_PWD`.

On Linux and WSL2, Podman or Docker can run the Oracle Free container directly.
On macOS, Podman first has to start a Linux VM (`podman machine`), and that VM
provider can be its own source of failure. The test harness itself does not rely
on macOS-specific behavior.

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
