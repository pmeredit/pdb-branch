# pdb-branch Rust Binding

Rust control-plane binding for `pdb-branch`.

The crate is structured around a small async `SqlExecutor` trait. Driver support
is selectable with Cargo features:

- `oracle-rs` - default; pure Rust async Oracle driver with Oracle JSON/OSON
  type support.
- `rust-oracle` - optional support for the ODPI-C based `oracle` crate.

```rust
use pdb_branch::{BranchClient, BranchOptions};

# async fn demo<E: pdb_branch::SqlExecutor>(executor: E) -> pdb_branch::Result<()> {
let client = BranchClient::new(executor);
client.create_branch("AGENT_RAG_042", BranchOptions::default()).await?;
# Ok(())
# }
```

Use the default `oracle-rs` backend:

```rust
use oracle_rs::{Config, Connection};
use pdb_branch::{BranchClient, BranchOptions, OracleRsExecutor};

# async fn demo() -> pdb_branch::Result<()> {
let config = Config::new("localhost", 1521, "FREE", "pdb_branch_admin", "password");
let connection = Connection::connect_with_config(config).await
    .map_err(|err| pdb_branch::Error::Database(err.to_string()))?;
let client = BranchClient::new(OracleRsExecutor::new(connection));
client.create_branch("AGENT_RAG_042", BranchOptions::default()).await?;
# Ok(())
# }
```

Use `create_branch_with_result` when callers need to inspect whether a requested
Snapshot Copy fell back to a full clone:

```rust
let result = client.create_branch_with_result("AGENT_RAG_043", BranchOptions::default()).await?;
if result.snapshot_copy_fell_back {
    eprintln!("{}", result.fallback_warning.as_deref().unwrap_or("snapshot copy fell back"));
}
```

Use the `oracle` crate instead:

```bash
cargo add pdb-branch --no-default-features --features rust-oracle
```

```rust
use oracle::Connection;
use pdb_branch::{BranchClient, BranchOptions, RustOracleExecutor};

# async fn demo() -> pdb_branch::Result<()> {
let connection = Connection::connect("pdb_branch_admin", "password", "localhost:1521/FREE")
    .map_err(|err| pdb_branch::Error::Database(err.to_string()))?;
let client = BranchClient::new(RustOracleExecutor::new(connection));
client.create_branch("AGENT_RAG_042", BranchOptions::default()).await?;
# Ok(())
# }
```

Run tests:

```bash
cargo test
```

Build the `pdb` CLI:

```bash
cargo build --features cli --bin pdb
```

Create a local TOML profile:

```bash
target/debug/pdb init --dsn localhost:1521/FREE --user sys --password PdbBranch1_ --from FREEPDB1
```

The CLI reads `.pdbprofile` from the current directory by default:

```toml
[database]
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

Daily branch usage mirrors the common `git branch` flow:

```bash
target/debug/pdb branch
target/debug/pdb branch EXPERIMENT_042 --notes "try reranking"
target/debug/pdb branch -d EXPERIMENT_042
```

Other lifecycle operations are explicit commands:

```bash
target/debug/pdb open EXPERIMENT_042
target/debug/pdb close EXPERIMENT_042
target/debug/pdb score EXPERIMENT_042 0.91 --notes "eval passed"
target/debug/pdb promote EXPERIMENT_042
```

Run the Oracle Free integration test from the repository root:

```bash
scripts/run-rust-oracle-free-integration.sh
```

The entry script runs the live Oracle Free lifecycle test through direct
`BranchClient` API calls using the ODPI-C based `rust-oracle` backend because
PDB lifecycle DDL requires SYSDBA authentication. The pure-Rust `oracle-rs`
backend is still covered by the normal Rust unit tests, but it does not
currently expose SYSDBA authentication. The live Rust script therefore needs the
Oracle Client runtime expected by the `oracle` crate.

Run the CLI Oracle Free integration test from the repository root:

```bash
scripts/run-cli-oracle-free-integration.sh
```

This entry script starts or reuses Oracle Free, builds `pdb`, writes a temporary
`.pdbprofile`, and runs create/list/score/close/open/promote/drop through the
CLI. Set `PDB_BRANCH_TEST_SNAPSHOT_COPY=1` to also assert that the CLI surfaces
the Oracle Free snapshot-copy fallback warning.
