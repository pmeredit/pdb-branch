# pdb-branch

`pdb-branch` is a small multi-language control plane over a shared PL/SQL
package for treating Oracle PDBs more like Git working state. It supports cheap
local PDB branches with Snapshot Copy where Oracle storage allows it, and it now
also models CDBs as remotes so branches can be pushed across CDB boundaries.

The Git analogy is intentionally scoped to the pieces that map to PDB lifecycle
operations: branches are PDB clones, remotes are CDB root connections, and push
asks the target CDB to clone `SOURCE_PDB@DB_LINK`. The project does not transfer
Git objects or client-side database files; Oracle still performs the PDB clone,
with either a full clone or a Snapshot Copy clone depending on the selected
clone mode.

Language bindings install or upgrade the PL/SQL package at startup. After that,
branch lifecycle, remote clone, and push-like operations go through a stable
database-side API.

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
- `bin/` - checkout-local command wrappers
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

## CLI Usage

The checkout includes a `pdb` command wrapper at `bin/pdb`. It runs the Rust CLI
without requiring callers to know Cargo's `target/debug` or `target/release`
paths. Build it once for faster startup:

```bash
cargo build --manifest-path bindings/rust/Cargo.toml --release --features cli --bin pdb
```

Initialize a local TOML profile:

```bash
bin/pdb init \
  --remote origin \
  --dsn localhost:1521/FREE \
  --user sys \
  --password PdbBranch1_ \
  --from FREEPDB1
```

`.pdbprofile` keeps the daily commands short:

```toml
default_remote = "origin"

[remotes.origin]
dsn = "localhost:1521/FREE"
user = "sys"
password = "PdbBranch1_"
sysdba = true
install = true

[branch]
from = "FREEPDB1"
snapshot_copy = true
open = true
```

Each remote is a CDB root connection. Lifecycle commands run against the
selected remote, defaulting to `default_remote`; use `--remote NAME` for a
one-off selection.

Branch usage mirrors the common `git branch` flow:

```bash
bin/pdb branch
bin/pdb branch EXPERIMENT_042 --notes "try reranking"
bin/pdb branch -v
bin/pdb branch -d EXPERIMENT_042
```

Add another CDB remote when you want to copy branches across CDBs:

```bash
bin/pdb remote add qa \
  --dsn qa-host:1521/QA \
  --user sys \
  --password '...' \
  --source-db-link PDB_BRANCH_ORIGIN \
  --push-clone-mode full

bin/pdb push qa EXPERIMENT_042
bin/pdb push qa EXPERIMENT_042:QA_EXPERIMENT_042
```

`pdb push` connects to the target remote and creates the target PDB from
`SOURCE_PDB@DB_LINK`. The `source_db_link` must already exist in the target CDB
and point back to the source CDB that contains the branch.

Push clone mode controls how the target CDB creates the PDB:

```bash
bin/pdb push qa EXPERIMENT_042 --clone-mode full
bin/pdb push qa EXPERIMENT_042 --clone-mode auto
bin/pdb push qa EXPERIMENT_042 --clone-mode snapshot
```

- `full` is the default. It creates an independent full clone in the target CDB.
- `auto` tries `SNAPSHOT COPY` first, then falls back to a full clone when
  Oracle reports that storage snapshots are unsupported. The CLI prints the
  fallback warning.
- `snapshot` requires `SNAPSHOT COPY`; if Oracle cannot create a snapshot copy,
  the push fails.

Set a target remote default when a CDB is known to support cheap snapshots:

```toml
[remotes.qa]
dsn = "qa-host:1521/QA"
user = "sys"
password = "..."
sysdba = true
install = true
source_db_link = "PDB_BRANCH_ORIGIN"
push_clone_mode = "auto"
```

Snapshot pushes are cheaper but less independent than full pushes. They depend
on Oracle Snapshot Copy PDB support in the database and storage layer, and they
keep lifecycle coupling between the source PDB and snapshot-copy clones. Use
`snapshot` for environments where failure is preferable to a full copy, and use
`auto` when cheap copy-on-write is preferred but a full copy is acceptable.

The same target-side remote clone primitive is exposed by the bindings:
Rust `clone_branch_from_remote`, Python `clone_branch_from_remote`, Node.js
`cloneBranchFromRemote`, and Java `cloneBranchFromRemote`. Use the
`*_with_result` / `*WithResult` variants when callers need to detect whether
`AUTO` fell back to a full clone. These APIs do not create database links,
unplug PDBs, or transfer bytes client-side; they connect to the target CDB and
ask Oracle to clone `SOURCE_PDB@DB_LINK`.

Other lifecycle operations are explicit commands:

```bash
bin/pdb open EXPERIMENT_042
bin/pdb close EXPERIMENT_042
bin/pdb score EXPERIMENT_042 0.91 --notes "eval passed"
bin/pdb promote EXPERIMENT_042
```

Command-line flags override environment variables, which override
`.pdbprofile`. Commands that connect to Oracle require a selected remote with a
DSN, either from the profile, environment, or command-line flags.

### Git compatibility notes

The CLI intentionally borrows Git vocabulary for the small subset of operations
that map well to PDB branching: `init`, `remote`, `branch`, and `push`. It is not
a byte-for-byte Git interface, because CDB connections, PDB lifecycle DDL, and
remote PDB cloning have database-specific requirements.

Known differences in the mirrored subset:

- `pdb init --remote origin --dsn ...` creates the first CDB remote while
  writing `.pdbprofile`; Git normally separates `git init` from
  `git remote add origin <url>`.
- `.pdbprofile` has `default_remote = "origin"`. Git usually derives defaults
  from branch upstream configuration, with `origin` only as a convention.
- `pdb remote add qa --dsn ... --user ... --password ...` stores a CDB root
  connection, not a single Git URL.
- `pdb remote add qa --push-clone-mode auto` stores the target CDB's default
  PDB clone strategy. Git remotes do not have storage clone modes.
- `pdb remote default NAME` is a convenience command with no direct Git
  equivalent.
- `pdb branch NAME --from PDB` is closest to `git branch NAME START_POINT`, but
  the source PDB is passed with `--from` because branch creation clones a PDB.
- `pdb branch --all` currently means include dropped branch records. Git
  `branch --all` means local plus remote-tracking branches.
- `pdb branch -D` currently follows the same drop path as `-d`; unlike Git, it
  is not a stronger force-delete mode yet.
- `pdb push qa EXPERIMENT_042` copies a PDB to the target CDB by asking the
  target to clone `SOURCE_PDB@DB_LINK`. Git push transfers refs and objects from
  the local repository.
- `pdb push --clone-mode snapshot` is intentionally not Git-like. It asks Oracle
  to create a storage-level snapshot-copy PDB, which can be much cheaper than a
  full copy but is tied to Oracle storage support and PDB lifecycle rules.
- `pdb push --source origin` names the source CDB remote that the target-side
  database link is expected to reach. Git has no source-remote flag because the
  source is the local repository.
- `SOURCE[:TARGET]` is only the simple Git refspec shape. The CLI does not
  implement force refspecs, delete refspecs, wildcards, tags, or multiple
  refspecs.

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

client.clone_branch_from_remote(
    "AGENT_RAG_042",
    source_pdb="AGENT_RAG_042",
    source_db_link="PDB_BRANCH_ORIGIN",
    clone_mode="AUTO",
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
scripts/run-python-oracle-free-integration.sh
```

By default the harness uses Podman and the full Oracle Free image:

```bash
ORACLE_FREE_IMAGE=container-registry.oracle.com/database/free:latest \
  scripts/run-python-oracle-free-integration.sh
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

Run the CLI Oracle Free integration test with its own entry script:

```bash
scripts/run-cli-oracle-free-integration.sh
```

This script uses the same Oracle Free harness, but drives lifecycle operations
through the `pdb` binary instead of calling the Rust library directly.

Run the Node.js and Java Oracle Free integration tests with their own entry
scripts:

```bash
scripts/run-node-oracle-free-integration.sh
scripts/run-java-oracle-free-integration.sh
```

The Node.js script installs `oracledb` into `bindings/node/node_modules` when it
is not already present. The Java script runs the `OracleFreeIntegrationTest`
Maven test class. When `PDB_BRANCH_TEST_SNAPSHOT_COPY=1` is set, both tests
assert the Oracle Free fallback event and print the fallback warning text.

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

## License

`pdb-branch` is dual-licensed under the [MIT License](LICENSE-MIT) and the
[Apache License, Version 2.0](LICENSE-APACHE). You may use this software under
the terms of either license.

## Current Boundaries

- The Python installer is idempotent, but it does not migrate destructive schema
  changes yet.
- PL/SQL identifiers are intentionally restricted to simple unquoted Oracle
  names.
- Promotion is metadata-only in v1; scaling or export workflows should be added
  as deployment-specific adapters.
- Local tests do not create real PDBs. Run integration tests against a CDB.
