package dev.pdbbranch;

import java.io.IOException;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

public final class BranchClient {
    private final SqlExecutor executor;

    public BranchClient(SqlExecutor executor) {
        this.executor = executor;
    }

    public void ensureInstalled() throws IOException, SQLException {
        for (String scriptName : SqlScripts.scriptNames()) {
            String script = SqlScripts.readSharedScript(scriptName);
            for (String statement : SqlScripts.splitSqlPlusScript(script)) {
                executor.execute(statement, List.of());
            }
        }
        executor.commit();
    }

    public void createBranch(String branchName, BranchOptions options) throws SQLException {
        call("pdb_branch.create_branch", Arrays.asList(
                branchName,
                options.fromPdb(),
                yn(options.snapshotCopy()),
                yn(options.openBranch()),
                nullable(options.profileName()),
                nullable(options.expiresAt()),
                nullable(options.notes())
        ));
    }

    public void openBranch(String branchName, String profileName) throws SQLException {
        call("pdb_branch.open_branch", Arrays.asList(branchName, nullable(profileName)));
    }

    public void closeBranch(String branchName, boolean immediate) throws SQLException {
        call("pdb_branch.close_branch", List.of(branchName, yn(immediate)));
    }

    public void dropBranch(String branchName, boolean includingDatafiles) throws SQLException {
        call("pdb_branch.drop_branch", List.of(branchName, yn(includingDatafiles)));
    }

    public void setProfile(String branchName, String profileName, boolean reopen) throws SQLException {
        call("pdb_branch.set_profile", List.of(branchName, profileName, yn(reopen)));
    }

    public void recordActivity(String branchName, String status) throws SQLException {
        call("pdb_branch.record_activity", Arrays.asList(branchName, nullable(status)));
    }

    public void recordScore(String branchName, double score, String notes) throws SQLException {
        call("pdb_branch.record_score", Arrays.asList(branchName, score, nullable(notes)));
    }

    public void promote(String branchName, String notes) throws SQLException {
        call("pdb_branch.promote_branch", Arrays.asList(branchName, nullable(notes)));
    }

    public void cleanup(CleanupOptions options) throws SQLException {
        call("pdb_branch.cleanup", List.of(options.closeIdleAfterMinutes(), yn(options.dropExpired())));
    }

    public void configureResourcePlan(ResourcePlanOptions options) throws SQLException {
        call("pdb_branch.configure_resource_plan", List.of(options.planName(), yn(options.activate())));
    }

    private void call(String name, List<Object> binds) throws SQLException {
        List<String> placeholders = new ArrayList<>();
        for (int i = 1; i <= binds.size(); i++) {
            placeholders.add(":" + i);
        }
        String sql = "BEGIN " + name + "(" + String.join(", ", placeholders) + "); END;";
        executor.execute(sql, binds);
    }

    private static String yn(boolean value) {
        return value ? "Y" : "N";
    }

    private static Object nullable(Object value) {
        return value;
    }
}
