import assert from "node:assert/strict";
import test from "node:test";

import { BranchClient, readSqlScript, splitSqlPlusScript } from "../src/index.js";

class FakeConnection {
  constructor(options = {}) {
    this.executions = [];
    this.queries = [];
    this.queryResults = [...(options.queryResults ?? [])];
    this.commits = 0;
  }

  async execute(sql, binds = [], options = {}) {
    if (/^\s*SELECT\b/iu.test(sql)) {
      this.queries.push({ sql, binds });
      const value = this.queryResults.shift();
      return value === undefined ? { rows: [] } : { rows: [[value]] };
    }

    this.executions.push({ sql, binds, options });
    return {};
  }

  async commit() {
    this.commits += 1;
  }
}

test("splitSqlPlusScript uses slash terminators", () => {
  const script = `
CREATE TABLE demo (id NUMBER)
/
BEGIN
  NULL;
END;
/
`;

  assert.deepEqual(splitSqlPlusScript(script), [
    "CREATE TABLE demo (id NUMBER)",
    "BEGIN\n  NULL;\nEND;"
  ]);
});

test("readSqlScript reads shared SQL scripts", async () => {
  const script = await readSqlScript("001_tables.sql");

  assert.match(script, /CREATE TABLE pdb_branch_branches/u);
});

test("createBranch calls PL/SQL package", async () => {
  const connection = new FakeConnection();
  const client = new BranchClient(connection);

  await client.createBranch("AGENT_RAG_042", {
    fromPdb: "GOLDEN_MASTER",
    notes: "try chunking"
  });

  assert.deepEqual(connection.executions, [
    {
      sql: "BEGIN pdb_branch.create_branch(:1, :2, :3, :4, :5, :6, :7); END;",
      binds: ["AGENT_RAG_042", "GOLDEN_MASTER", "Y", "Y", null, null, "try chunking"],
      options: { autoCommit: false }
    }
  ]);
});

test("createBranchWithResult reports snapshot fallback", async () => {
  const warning = "WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone";
  const connection = new FakeConnection({ queryResults: ["10", warning] });
  const client = new BranchClient(connection);

  const result = await client.createBranchWithResult("AGENT_RAG_042", {
    fromPdb: "GOLDEN_MASTER",
    snapshotCopy: true,
    notes: "try chunking"
  });

  assert.deepEqual(result, {
    snapshotCopyRequested: true,
    snapshotCopyFellBack: true,
    fallbackWarning: warning
  });
  assert.equal(connection.queries.length, 2);
  assert.match(connection.queries[0].sql, /MAX\(event_id\)/u);
  assert.match(connection.queries[1].sql, /event_type = :2/u);
  assert.deepEqual(connection.queries[1].binds, ["AGENT_RAG_042", "SNAPSHOT_COPY_FALLBACK", 10]);
});

test("cloneBranchFromRemote calls PL/SQL package", async () => {
  const connection = new FakeConnection();
  const client = new BranchClient(connection);

  await client.cloneBranchFromRemote("AGENT_RAG_042", {
    sourcePdb: "SOURCE_BRANCH",
    sourceDbLink: "PDB_BRANCH_ORIGIN",
    cloneMode: "snapshot",
    notes: "push to qa",
    createFileDest: "/opt/oracle/oradata/QA"
  });

  assert.deepEqual(connection.executions, [
    {
      sql: "BEGIN pdb_branch.clone_branch_from_remote(:1, :2, :3, :4, :5, :6, :7, :8, :9); END;",
      binds: [
        "AGENT_RAG_042",
        "SOURCE_BRANCH",
        "PDB_BRANCH_ORIGIN",
        "SNAPSHOT",
        "Y",
        null,
        null,
        "push to qa",
        "/opt/oracle/oradata/QA"
      ],
      options: { autoCommit: false }
    }
  ]);
});

test("cloneBranchFromRemoteWithResult reports auto snapshot fallback", async () => {
  const warning =
    "WARNING: remote SNAPSHOT COPY requested with clone mode AUTO; pushed with full clone";
  const connection = new FakeConnection({ queryResults: ["12", warning] });
  const client = new BranchClient(connection);

  const result = await client.cloneBranchFromRemoteWithResult("AGENT_RAG_042", {
    sourcePdb: "SOURCE_BRANCH",
    sourceDbLink: "PDB_BRANCH_ORIGIN",
    cloneMode: "auto"
  });

  assert.deepEqual(result, {
    cloneMode: "AUTO",
    snapshotCopyRequested: true,
    snapshotCopyFellBack: true,
    fallbackWarning: warning
  });
  assert.equal(connection.queries.length, 2);
  assert.match(connection.queries[0].sql, /MAX\(event_id\)/u);
  assert.match(connection.queries[1].sql, /event_type = :2/u);
  assert.deepEqual(connection.queries[1].binds, [
    "AGENT_RAG_042",
    "REMOTE_SNAPSHOT_COPY_FALLBACK",
    12
  ]);
});

test("ensureInstalled executes shared SQL statements and commits", async () => {
  const connection = new FakeConnection();
  const client = new BranchClient(connection);

  await client.ensureInstalled();

  assert.equal(connection.executions.length, 6);
  assert.equal(connection.commits, 1);
});
