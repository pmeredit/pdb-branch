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
