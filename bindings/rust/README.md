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

Run the Oracle Free integration test from the repository root:

```bash
PDB_BRANCH_TEST_RUST=1 scripts/run-oracle-free-integration.sh
```

The Rust integration path uses the default pure-Rust `oracle-rs` backend and the
shared harness prepares a common CDB control-plane user for it.
