import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import test from "node:test";

import { BranchClient } from "../src/index.js";

const NAME_RE = /^[A-Z][A-Z0-9_$#]{0,29}$/u;

if (process.env.PDB_BRANCH_INTEGRATION !== "1") {
  test("oracle free branch lifecycle", { skip: "set PDB_BRANCH_INTEGRATION=1 to run Oracle integration tests" }, () => {});
} else {
  const oracleModule = await import("oracledb");
  const oracledb = oracleModule.default ?? oracleModule;

  test("oracle free branch lifecycle: full clone", async () => {
    await runBranchLifecycle(oracledb, false);
  });

  test(
    "oracle free branch lifecycle: snapshot copy fallback",
    {
      skip:
        process.env.PDB_BRANCH_TEST_SNAPSHOT_COPY === "1"
          ? false
          : "set PDB_BRANCH_TEST_SNAPSHOT_COPY=1 to exercise SNAPSHOT COPY"
    },
    async () => {
      await runBranchLifecycle(oracledb, true);
    }
  );
}

async function runBranchLifecycle(oracledb, snapshotCopy) {
  const config = configFromEnv();
  const root = await connectRoot(oracledb, config);
  const client = new BranchClient(root);
  const branchName = makeBranchName(snapshotCopy ? "NBISC" : "NBIFC");

  try {
    await requireCdbRoot(root, config);
    await client.ensureInstalled();
    await prepareParentPdb(oracledb, root, config);

    let createResult;
    try {
      createResult = await client.createBranchWithResult(branchName, {
        fromPdb: config.parentPdb,
        snapshotCopy,
        notes: "oracle free node integration test"
      });
    } catch (error) {
      const facts = await collectDatabaseFacts(root, config);
      assert.fail(
        `createBranch failed against Oracle database:\n${formatDatabaseFacts(facts)}\nerror: ${error}`
      );
    }

    assert.equal(
      createResult.snapshotCopyRequested,
      snapshotCopy,
      "createBranch result should report whether snapshot copy was requested"
    );

    if (snapshotCopy) {
      assert.equal(
        createResult.snapshotCopyFellBack,
        true,
        "createBranch result should report snapshot-copy fallback"
      );
      assert.match(
        createResult.fallbackWarning ?? "",
        /created with full clone/u,
        "createBranch result should include the fallback warning text"
      );
      console.warn(createResult.fallbackWarning);
    } else {
      assert.equal(
        createResult.snapshotCopyFellBack,
        false,
        "full-clone createBranch result should not report snapshot-copy fallback"
      );
    }

    await assertBranchMetadata(root, branchName, config);

    if (snapshotCopy) {
      await assertSnapshotFallbackEvent(root, branchName);
      await mutateParentAfterBranchCreate(oracledb, root, config);
    }

    const branch = await connectWorkload(oracledb, config, branchName);
    try {
      assert.equal(
        await scalar(branch, "SELECT COUNT(*) FROM pdb_branch_seed"),
        1,
        "branch should preserve parent seed state at branch creation time"
      );
      await execute(branch, "INSERT INTO experiment_log(event) VALUES (:1)", [
        "agent wrote to branch"
      ]);
      await branch.commit();
      assert.equal(
        await scalar(branch, "SELECT COUNT(*) FROM experiment_log"),
        1,
        "branch should accept writes from the workload user"
      );
    } finally {
      await branch.close();
    }

    await client.recordScore(branchName, 0.99, { notes: "node integration test passed" });
    assert.equal(
      Number(
        await scalar(root, "SELECT score FROM pdb_branch_branches WHERE branch_name = :1", [
          branchName
        ])
      ),
      0.99,
      "branch score should be recorded"
    );
  } finally {
    await ignoreErrors(() => client.dropBranch(branchName, { includingDatafiles: true }));
    await ignoreErrors(() => reopenParentReadWrite(root, config.parentPdb));
    await root.close();
  }
}

function configFromEnv() {
  return {
    rootDsn: process.env.PDB_BRANCH_ROOT_DSN ?? "localhost:1521/FREE",
    rootUser: process.env.PDB_BRANCH_SYS_USER ?? "sys",
    rootPassword:
      process.env.PDB_BRANCH_SYS_PASSWORD ?? process.env.ORACLE_PWD ?? "PdbBranch1_",
    branchDsnTemplate:
      process.env.PDB_BRANCH_BRANCH_DSN_TEMPLATE ?? "localhost:1521/{branch_name}",
    parentPdb: simpleName(process.env.PDB_BRANCH_PARENT_PDB ?? "FREEPDB1", "parent PDB"),
    appUser: simpleName(process.env.PDB_BRANCH_APP_USER ?? "PDB_BRANCH_APP", "app user"),
    appPassword: process.env.PDB_BRANCH_APP_PASSWORD ?? "PdbBranch1_",
    serviceTimeoutSeconds: Number.parseInt(
      process.env.PDB_BRANCH_SERVICE_TIMEOUT_SECONDS ?? "120",
      10
    )
  };
}

async function connectRoot(oracledb, config) {
  return oracledb.getConnection({
    user: config.rootUser,
    password: config.rootPassword,
    connectString: config.rootDsn,
    privilege: oracledb.SYSDBA
  });
}

async function connect(oracledb, dsn, user, password) {
  return oracledb.getConnection({ user, password, connectString: dsn });
}

async function connectWorkload(oracledb, config, pdbName) {
  const dsn = config.branchDsnTemplate.replace("{branch_name}", pdbName);
  const deadline = Date.now() + config.serviceTimeoutSeconds * 1000;
  let lastError = null;

  while (Date.now() < deadline) {
    try {
      return await connect(oracledb, dsn, config.appUser, config.appPassword);
    } catch (error) {
      lastError = error;
      await delay(5000);
    }
  }

  throw new Error(`timed out waiting for PDB service ${dsn}: ${lastError ?? "no attempt made"}`);
}

async function requireCdbRoot(root, config) {
  const facts = await collectDatabaseFacts(root, config);
  const problems = [];

  if (facts.cdb !== "YES") {
    problems.push("database is not a CDB");
  }
  if (facts.conName !== "CDB$ROOT") {
    problems.push(`connection is in ${facts.conName}, not CDB$ROOT`);
  }
  if (facts.parentOpenMode === null) {
    problems.push(`parent PDB ${config.parentPdb} was not found`);
  }

  if (problems.length > 0) {
    assert.fail(
      `Oracle integration tests require a CDB root connection: ${problems.join("; ")}\n${formatDatabaseFacts(facts)}`
    );
  }
}

async function collectDatabaseFacts(root, config) {
  const params = new Map(
    (
      await rows(
        root,
        `
        SELECT name, value
          FROM v$parameter
         WHERE name IN ('db_create_file_dest', 'pdb_file_name_convert')
        `
      )
    ).map((row) => [row[0], row[1]])
  );
  const parent = await rows(
    root,
    `
    SELECT open_mode, restricted
      FROM v$pdbs
     WHERE name = :1
    `,
    [config.parentPdb]
  );

  return {
    dsn: config.rootDsn,
    banner: await scalar(root, "SELECT banner FROM v$version WHERE ROWNUM = 1"),
    cdb: await scalar(root, "SELECT cdb FROM v$database"),
    conName: await scalar(root, "SELECT SYS_CONTEXT('USERENV', 'CON_NAME') FROM dual"),
    dbCreateFileDest: params.get("db_create_file_dest") ?? null,
    pdbFileNameConvert: params.get("pdb_file_name_convert") ?? null,
    parentPdb: config.parentPdb,
    parentOpenMode: parent[0]?.[0] ?? null,
    parentRestricted: parent[0]?.[1] ?? null
  };
}

function formatDatabaseFacts(facts) {
  return [
    `  dsn: ${facts.dsn}`,
    `  banner: ${facts.banner}`,
    `  cdb: ${facts.cdb}`,
    `  container: ${facts.conName}`,
    `  db_create_file_dest: ${facts.dbCreateFileDest ?? "(unset)"}`,
    `  pdb_file_name_convert: ${facts.pdbFileNameConvert ?? "(unset)"}`,
    `  parent_pdb: ${facts.parentPdb}`,
    `  parent_open_mode: ${facts.parentOpenMode ?? "(missing)"}`,
    `  parent_restricted: ${facts.parentRestricted ?? "(missing)"}`
  ].join("\n");
}

async function prepareParentPdb(oracledb, root, config) {
  await reopenParentReadWrite(root, config.parentPdb);
  await execute(root, `ALTER SESSION SET CONTAINER = ${config.parentPdb}`);

  try {
    await executeIgnore(root, `DROP USER ${config.appUser} CASCADE`, new Set([1918]));
    await execute(
      root,
      `CREATE USER ${config.appUser} IDENTIFIED BY "${escapeQuoted(config.appPassword)}"`
    );
    await execute(root, `ALTER USER ${config.appUser} QUOTA UNLIMITED ON USERS`);
    await execute(root, `GRANT CREATE SESSION, CREATE TABLE TO ${config.appUser}`);
  } finally {
    await execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT");
  }

  const parent = await connectWorkload(oracledb, config, config.parentPdb);
  try {
    await execute(
      parent,
      "CREATE TABLE pdb_branch_seed (id NUMBER PRIMARY KEY, label VARCHAR2(100) NOT NULL)"
    );
    await execute(
      parent,
      "CREATE TABLE experiment_log (event VARCHAR2(100) NOT NULL, created_at TIMESTAMP DEFAULT SYSTIMESTAMP NOT NULL)"
    );
    await execute(parent, "INSERT INTO pdb_branch_seed(id, label) VALUES (1, 'seed row')");
    await parent.commit();
  } finally {
    await parent.close();
  }

  await closePdb(root, config.parentPdb);
  await execute(root, `ALTER PLUGGABLE DATABASE ${config.parentPdb} OPEN READ ONLY`);
}

async function mutateParentAfterBranchCreate(oracledb, root, config) {
  await reopenParentReadWrite(root, config.parentPdb);
  const parent = await connectWorkload(oracledb, config, config.parentPdb);
  try {
    await execute(
      parent,
      "INSERT INTO pdb_branch_seed(id, label) VALUES (2, 'parent mutation')"
    );
    await parent.commit();
    assert.equal(await scalar(parent, "SELECT COUNT(*) FROM pdb_branch_seed"), 2);
  } finally {
    await parent.close();
  }
}

async function reopenParentReadWrite(root, parentPdb) {
  await execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT");
  await closePdb(root, parentPdb);
  await execute(root, `ALTER PLUGGABLE DATABASE ${parentPdb} OPEN READ WRITE`);
}

async function closePdb(root, pdbName) {
  await executeIgnore(root, `ALTER PLUGGABLE DATABASE ${pdbName} CLOSE IMMEDIATE`, new Set([65020]));
}

async function assertBranchMetadata(root, branchName, config) {
  const branch = await requiredRow(
    root,
    "SELECT branch_name, parent_pdb, status FROM pdb_branch_branches WHERE branch_name = :1",
    [branchName]
  );

  assert.equal(branch[0], branchName, "control table should record branch name");
  assert.equal(branch[1], config.parentPdb, "control table should record parent PDB");
  assert.equal(branch[2], "OPEN", "created branch should be open");
}

async function assertSnapshotFallbackEvent(root, branchName) {
  const details = await scalar(
    root,
    `
    SELECT warning
      FROM (
            SELECT DBMS_LOB.SUBSTR(details, 4000, 1) warning
              FROM pdb_branch_events
             WHERE branch_name = :1
               AND event_type = 'SNAPSHOT_COPY_FALLBACK'
             ORDER BY event_id DESC
           )
     WHERE ROWNUM = 1
    `,
    [branchName]
  );
  assert.match(
    String(details),
    /created with full clone/u,
    "snapshot fallback event should explain that a full clone was created"
  );
}

async function execute(connection, sql, parameters = []) {
  await connection.execute(sql, parameters);
}

async function executeIgnore(connection, sql, ignoredCodes) {
  try {
    await execute(connection, sql);
  } catch (error) {
    if (!ignoredCodes.has(errorCode(error))) {
      throw error;
    }
  }
}

async function scalar(connection, sql, parameters = []) {
  const row = await requiredRow(connection, sql, parameters);
  return row[0];
}

async function requiredRow(connection, sql, parameters = []) {
  const result = await rows(connection, sql, parameters);
  assert.ok(result.length > 0, `query returned no rows: ${sql}`);
  return result[0];
}

async function rows(connection, sql, parameters = []) {
  const result = await connection.execute(sql, parameters);
  return result.rows ?? [];
}

function errorCode(error) {
  if (Number.isInteger(error?.errorNum)) {
    return error.errorNum;
  }
  const match = /ORA-(\d+)/u.exec(String(error?.message ?? error));
  return match ? Number.parseInt(match[1], 10) : null;
}

function makeBranchName(prefix) {
  return simpleName(`${prefix}${randomUUID().replaceAll("-", "").slice(0, 8).toUpperCase()}`, "branch name");
}

function simpleName(value, label) {
  const name = value.trim().toUpperCase();
  if (!NAME_RE.test(name)) {
    throw new Error(`${label} must be an unquoted Oracle identifier of 30 chars or fewer`);
  }
  return name;
}

function escapeQuoted(value) {
  return value.replaceAll('"', '""');
}

async function ignoreErrors(callback) {
  try {
    await callback();
  } catch {
    // Cleanup should not mask the real test failure.
  }
}
