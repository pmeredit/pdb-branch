# pdb-branch Rust Binding

Rust control-plane binding for `pdb-branch`.

The crate is structured around a small async `SqlExecutor` trait. The default
feature is reserved for `oracle-rs`, which is the intended driver backend. The
older ODPI-C based `oracle` crate can be supported behind a separate
`rust-oracle` feature without changing the public `BranchClient` API.

```rust
use pdb_branch::{BranchClient, BranchOptions};

# async fn demo<E: pdb_branch::SqlExecutor>(executor: E) -> pdb_branch::Result<()> {
let client = BranchClient::new(executor);
client.create_branch("AGENT_RAG_042", BranchOptions::default()).await?;
# Ok(())
# }
```

Run tests:

```bash
cargo test
```
