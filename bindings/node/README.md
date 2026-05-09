# pdb-branch Node.js Binding

Node.js control-plane binding for `pdb-branch`.

This package wraps a connection-like object from `node-oracledb`. It does not
own credentials or connection pools.

```js
import oracledb from "oracledb";
import { BranchClient } from "@pdb-branch/node";

const root = await oracledb.getConnection({
  user: "sys",
  password: "...",
  connectString: "localhost:1521/FREE",
  privilege: oracledb.SYSDBA
});

const branches = new BranchClient(root);
await branches.ensureInstalled();
await branches.createBranch("AGENT_RAG_042", { fromPdb: "GOLDEN_MASTER" });
```

Run tests:

```bash
npm test
```

Run the Oracle Free integration test from the repository root:

```bash
scripts/run-node-oracle-free-integration.sh
```
