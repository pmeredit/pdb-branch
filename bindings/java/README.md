# pdb-branch Java Binding

Java/JDBC control-plane binding for `pdb-branch`.

`BranchClient` wraps a `SqlExecutor`; `JdbcSqlExecutor` adapts a JDBC
`Connection`.

```java
try (Connection root = DriverManager.getConnection(rootDsn, props)) {
    BranchClient branches = new BranchClient(new JdbcSqlExecutor(root));
    branches.ensureInstalled();
    branches.createBranch("AGENT_RAG_042", BranchOptions.defaults());
    branches.cloneBranchFromRemote(
            "AGENT_RAG_042",
            RemoteCloneOptions.defaults()
                    .withSourcePdb("AGENT_RAG_042")
                    .withSourceDbLink("PDB_BRANCH_ORIGIN")
                    .withCloneMode("AUTO")
    );
}
```

Remote clone calls run in the target CDB. The database link must already exist
there and point back to the source CDB. Clone mode is `FULL`, `AUTO`, or
`SNAPSHOT`. Use `cloneBranchFromRemoteWithResult` when callers need to inspect
whether `AUTO` fell back to a full clone.

Run tests:

```bash
mvn test
```

Run the Oracle Free integration test from the repository root:

```bash
scripts/run-java-oracle-free-integration.sh
```
