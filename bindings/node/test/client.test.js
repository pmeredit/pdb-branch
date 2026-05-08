import assert from "node:assert/strict";
import test from "node:test";

import { BranchClient, readSqlScript, splitSqlPlusScript } from "../src/index.js";

class FakeConnection {
  constructor() {
    this.executions = [];
    this.commits = 0;
  }

  async execute(sql, binds = [], options = {}) {
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

test("ensureInstalled executes shared SQL statements and commits", async () => {
  const connection = new FakeConnection();
  const client = new BranchClient(connection);

  await client.ensureInstalled();

  assert.equal(connection.executions.length, 6);
  assert.equal(connection.commits, 1);
});
