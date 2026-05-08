package dev.pdbbranch;

import org.junit.jupiter.api.Test;

import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

final class BranchClientTest {
    @Test
    void splitSqlPlusScriptUsesSlashTerminators() {
        String script = """
                CREATE TABLE demo (id NUMBER)
                /
                BEGIN
                  NULL;
                END;
                /
                """;

        assertEquals(
                List.of("CREATE TABLE demo (id NUMBER)", "BEGIN\n  NULL;\nEND;"),
                SqlScripts.splitSqlPlusScript(script)
        );
    }

    @Test
    void readSharedScriptReadsRootSqlDirectory() throws Exception {
        String script = SqlScripts.readSharedScript("001_tables.sql");

        assertTrue(script.contains("CREATE TABLE pdb_branch_branches"));
    }

    @Test
    void createBranchCallsPlsqlPackage() throws Exception {
        FakeExecutor executor = new FakeExecutor();
        BranchClient client = new BranchClient(executor);

        client.createBranch(
                "AGENT_RAG_042",
                BranchOptions.defaults().withNotes("try chunking")
        );

        assertEquals(1, executor.executions.size());
        assertEquals(
                "BEGIN pdb_branch.create_branch(:1, :2, :3, :4, :5, :6, :7); END;",
                executor.executions.get(0).sql
        );
        assertEquals(
                Arrays.asList("AGENT_RAG_042", "GOLDEN_MASTER", "Y", "Y", null, null, "try chunking"),
                executor.executions.get(0).binds
        );
    }

    @Test
    void ensureInstalledExecutesSharedSqlAndCommits() throws Exception {
        FakeExecutor executor = new FakeExecutor();
        BranchClient client = new BranchClient(executor);

        client.ensureInstalled();

        assertEquals(6, executor.executions.size());
        assertEquals(1, executor.commits);
    }

    private static final class FakeExecutor implements SqlExecutor {
        private final List<Execution> executions = new ArrayList<>();
        private int commits;

        @Override
        public void execute(String sql, List<Object> binds) throws SQLException {
            executions.add(new Execution(sql, binds));
        }

        @Override
        public void commit() {
            commits++;
        }
    }

    private record Execution(String sql, List<Object> binds) {
    }
}
