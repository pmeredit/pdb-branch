import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SQL_SCRIPTS = ["001_tables.sql", "002_package.sql"];
const MODULE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(MODULE_DIR, "../../..");

export class BranchClient {
  constructor(connection, options = {}) {
    this.connection = connection;
    this.autoCommit = options.autoCommit ?? false;
  }

  async ensureInstalled() {
    for (const scriptName of SQL_SCRIPTS) {
      const script = await readSqlScript(scriptName);
      for (const statement of splitSqlPlusScript(script)) {
        await this.connection.execute(statement, [], { autoCommit: this.autoCommit });
      }
    }

    if (!this.autoCommit && typeof this.connection.commit === "function") {
      await this.connection.commit();
    }
  }

  async createBranch(branchName, options = {}) {
    await this.call("pdb_branch.create_branch", [
      branchName,
      options.fromPdb ?? "GOLDEN_MASTER",
      yn(options.snapshotCopy ?? true),
      yn(options.openBranch ?? true),
      options.profileName ?? null,
      options.expiresAt ?? null,
      options.notes ?? null
    ]);
  }

  async createBranchWithResult(branchName, options = {}) {
    const snapshotCopyRequested = options.snapshotCopy ?? true;
    const lastEventId = snapshotCopyRequested ? await this.maxEventId(branchName) : null;

    await this.createBranch(branchName, options);

    const fallbackWarning = snapshotCopyRequested
      ? await this.snapshotFallbackWarning(branchName, lastEventId)
      : null;

    return {
      snapshotCopyRequested,
      snapshotCopyFellBack: fallbackWarning !== null,
      fallbackWarning
    };
  }

  async cloneBranchFromRemote(branchName, options = {}) {
    await this.call("pdb_branch.clone_branch_from_remote", [
      branchName,
      options.sourcePdb ?? "GOLDEN_MASTER",
      options.sourceDbLink ?? "PDB_BRANCH_SOURCE",
      normalizeCloneMode(options.cloneMode ?? "FULL"),
      yn(options.openBranch ?? true),
      options.profileName ?? null,
      options.expiresAt ?? null,
      options.notes ?? null,
      options.createFileDest ?? null
    ]);
  }

  async cloneBranchFromRemoteWithResult(branchName, options = {}) {
    const cloneMode = normalizeCloneMode(options.cloneMode ?? "FULL");
    const tracksFallback = cloneMode === "AUTO";
    const snapshotCopyRequested = tracksFallback || cloneMode === "SNAPSHOT";
    const lastEventId = tracksFallback ? await this.maxEventId(branchName) : null;

    await this.cloneBranchFromRemote(branchName, { ...options, cloneMode });

    const fallbackWarning = tracksFallback
      ? await this.remoteSnapshotFallbackWarning(branchName, lastEventId)
      : null;

    return {
      cloneMode,
      snapshotCopyRequested,
      snapshotCopyFellBack: fallbackWarning !== null,
      fallbackWarning
    };
  }

  async openBranch(branchName, options = {}) {
    await this.call("pdb_branch.open_branch", [branchName, options.profileName ?? null]);
  }

  async closeBranch(branchName, options = {}) {
    await this.call("pdb_branch.close_branch", [branchName, yn(options.immediate ?? true)]);
  }

  async dropBranch(branchName, options = {}) {
    await this.call("pdb_branch.drop_branch", [branchName, yn(options.includingDatafiles ?? true)]);
  }

  async setProfile(branchName, profileName, options = {}) {
    await this.call("pdb_branch.set_profile", [
      branchName,
      profileName,
      yn(options.reopen ?? true)
    ]);
  }

  async recordActivity(branchName, options = {}) {
    await this.call("pdb_branch.record_activity", [branchName, options.status ?? null]);
  }

  async recordScore(branchName, score, options = {}) {
    await this.call("pdb_branch.record_score", [branchName, score, options.notes ?? null]);
  }

  async promote(branchName, options = {}) {
    await this.call("pdb_branch.promote_branch", [branchName, options.notes ?? null]);
  }

  async cleanup(options = {}) {
    await this.call("pdb_branch.cleanup", [
      options.closeIdleAfterMinutes ?? 60,
      yn(options.dropExpired ?? true)
    ]);
  }

  async configureResourcePlan(options = {}) {
    await this.call("pdb_branch.configure_resource_plan", [
      options.planName ?? "PDB_BRANCH_PLAN",
      yn(options.activate ?? false)
    ]);
  }

  async call(name, args) {
    const placeholders = args.map((_, index) => `:${index + 1}`).join(", ");
    await this.connection.execute(`BEGIN ${name}(${placeholders}); END;`, args, {
      autoCommit: this.autoCommit
    });
  }

  async maxEventId(branchName) {
    const value = await this.queryOptionalValue(
      `
      SELECT TO_CHAR(MAX(event_id))
        FROM pdb_branch_events
       WHERE branch_name = UPPER(:1)
      `,
      [branchName]
    );

    return value === null ? null : Number.parseInt(String(value), 10);
  }

  async snapshotFallbackWarning(branchName, lastEventId) {
    return this.eventWarning(branchName, "SNAPSHOT_COPY_FALLBACK", lastEventId);
  }

  async remoteSnapshotFallbackWarning(branchName, lastEventId) {
    return this.eventWarning(branchName, "REMOTE_SNAPSHOT_COPY_FALLBACK", lastEventId);
  }

  async eventWarning(branchName, eventType, lastEventId) {
    const eventIdFilter = lastEventId === null ? "" : "AND event_id > :3";

    const value = await this.queryOptionalValue(
      `
      SELECT warning
        FROM (
                        SELECT DBMS_LOB.SUBSTR(details, 4000, 1) warning
                          FROM pdb_branch_events
                         WHERE branch_name = UPPER(:1)
                           AND event_type = :2
                           ${eventIdFilter}
                         ORDER BY event_id DESC
                       )
       WHERE ROWNUM = 1
      `,
      [branchName, eventType, ...(lastEventId === null ? [] : [lastEventId])]
    );

    return value === null ? null : String(value);
  }

  async queryOptionalValue(sql, binds = []) {
    const result = await this.connection.execute(sql, binds);
    const rows = result.rows ?? [];
    if (rows.length === 0) {
      return null;
    }

    const row = rows[0];
    if (Array.isArray(row)) {
      return row[0] ?? null;
    }

    const values = Object.values(row);
    return values[0] ?? null;
  }
}

export async function readSqlScript(scriptName) {
  return readFile(resolve(REPO_ROOT, "sql", scriptName), "utf8");
}

export function splitSqlPlusScript(script) {
  const statements = [];
  let current = [];

  for (const line of script.split(/\r?\n/u)) {
    if (line.trim() === "/") {
      const statement = current.join("\n").trim();
      if (statement) {
        statements.push(statement);
      }
      current = [];
    } else {
      current.push(line.trimEnd());
    }
  }

  const trailing = current.join("\n").trim();
  if (trailing) {
    statements.push(trailing);
  }

  return statements;
}

function yn(value) {
  return value ? "Y" : "N";
}

function normalizeCloneMode(value) {
  const normalized = String(value).trim().toUpperCase();
  if (!["FULL", "AUTO", "SNAPSHOT"].includes(normalized)) {
    throw new Error("cloneMode must be FULL, AUTO, or SNAPSHOT");
  }
  return normalized;
}
