package dev.pdbbranch;

import org.junit.jupiter.api.Test;

import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.List;
import java.util.Optional;

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
                "BEGIN pdb_branch.create_branch(?, ?, ?, ?, ?, ?, ?); END;",
                executor.executions.get(0).sql
        );
        assertEquals(
                Arrays.asList("AGENT_RAG_042", "GOLDEN_MASTER", "Y", "Y", null, null, "try chunking"),
                executor.executions.get(0).binds
        );
    }

    @Test
    void createBranchWithResultReportsSnapshotFallback() throws Exception {
        String warning = "WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone";
        FakeExecutor executor = new FakeExecutor(List.of(Optional.of("10"), Optional.of(warning)));
        BranchClient client = new BranchClient(executor);

        BranchCreateResult result = client.createBranchWithResult(
                "AGENT_RAG_042",
                BranchOptions.defaults().withNotes("try chunking")
        );

        assertEquals(new BranchCreateResult(true, true, warning), result);
        assertEquals(2, executor.queries.size());
        assertTrue(executor.queries.get(0).sql.contains("MAX(event_id)"));
        assertTrue(executor.queries.get(1).sql.contains("SNAPSHOT_COPY_FALLBACK"));
        assertEquals(Arrays.asList("AGENT_RAG_042", 10L), executor.queries.get(1).binds);
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
        private final List<Execution> queries = new ArrayList<>();
        private final Deque<Optional<String>> queryResults;
        private int commits;

        private FakeExecutor() {
            this(List.of());
        }

        private FakeExecutor(List<Optional<String>> queryResults) {
            this.queryResults = new ArrayDeque<>(queryResults);
        }

        @Override
        public void execute(String sql, List<Object> binds) throws SQLException {
            executions.add(new Execution(sql, binds));
        }

        @Override
        public Optional<String> queryOptionalString(String sql, List<Object> binds) {
            queries.add(new Execution(sql, binds));
            return queryResults.isEmpty() ? Optional.empty() : queryResults.removeFirst();
        }

        @Override
        public void commit() {
            commits++;
        }
    }

    private record Execution(String sql, List<Object> binds) {
    }
}
