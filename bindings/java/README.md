# pdb-branch Java Binding

Java/JDBC control-plane binding for `pdb-branch`.

`BranchClient` wraps a `SqlExecutor`; `JdbcSqlExecutor` adapts a JDBC
`Connection`.

```java
try (Connection root = DriverManager.getConnection(rootDsn, props)) {
    BranchClient branches = new BranchClient(new JdbcSqlExecutor(root));
    branches.ensureInstalled();
    branches.createBranch("AGENT_RAG_042", BranchOptions.defaults());
}
```

Run tests:

```bash
mvn test
```
