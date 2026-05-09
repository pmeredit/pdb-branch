package dev.pdbbranch;

import org.junit.jupiter.api.Test;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.time.Duration;
import java.util.Locale;
import java.util.Properties;
import java.util.Set;
import java.util.UUID;
import java.util.regex.Pattern;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.fail;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

final class OracleFreeIntegrationTest {
    private static final Pattern NAME_RE = Pattern.compile("^[A-Z][A-Z0-9_$#]{0,29}$");

    @Test
    void oracleFreeFullCloneBranchLifecycle() throws Exception {
        assumeTrue(
                "1".equals(System.getenv("PDB_BRANCH_INTEGRATION")),
                "set PDB_BRANCH_INTEGRATION=1 to run Oracle integration tests"
        );

        runBranchLifecycle(false);
    }

    @Test
    void oracleFreeSnapshotCopyFallbackBranchLifecycle() throws Exception {
        assumeTrue(
                "1".equals(System.getenv("PDB_BRANCH_INTEGRATION")),
                "set PDB_BRANCH_INTEGRATION=1 to run Oracle integration tests"
        );
        assumeTrue(
                "1".equals(System.getenv("PDB_BRANCH_TEST_SNAPSHOT_COPY")),
                "set PDB_BRANCH_TEST_SNAPSHOT_COPY=1 to exercise SNAPSHOT COPY"
        );

        runBranchLifecycle(true);
    }

    private static void runBranchLifecycle(boolean snapshotCopy) throws Exception {
        TestConfig config = TestConfig.fromEnv();
        String branchName = makeBranchName(snapshotCopy ? "JBISC" : "JBIFC");

        try (Connection root = connectRoot(config)) {
            BranchClient client = new BranchClient(new JdbcSqlExecutor(root));

            try {
                requireCdbRoot(root, config);
                client.ensureInstalled();
                prepareParentPdb(root, config);

                BranchCreateResult createResult;
                try {
                    createResult = client.createBranchWithResult(
                            branchName,
                            BranchOptions.defaults()
                                    .withFromPdb(config.parentPdb())
                                    .withSnapshotCopy(snapshotCopy)
                                    .withNotes("oracle free java integration test")
                    );
                } catch (SQLException exception) {
                    DatabaseFacts facts = collectDatabaseFacts(root, config);
                    fail("createBranch failed against Oracle database:\n"
                            + formatDatabaseFacts(facts)
                            + "\nerror: "
                            + exception);
                    return;
                }

                assertEquals(
                        snapshotCopy,
                        createResult.snapshotCopyRequested(),
                        "createBranch result should report whether snapshot copy was requested"
                );

                if (snapshotCopy) {
                    assertTrue(
                            createResult.snapshotCopyFellBack(),
                            "createBranch result should report snapshot-copy fallback"
                    );
                    assertNotNull(
                            createResult.fallbackWarning(),
                            "createBranch result should include the fallback warning text"
                    );
                    assertTrue(
                            createResult.fallbackWarning().contains("created with full clone"),
                            "createBranch result should include the fallback warning text"
                    );
                    System.err.println(createResult.fallbackWarning());
                } else {
                    assertTrue(
                            !createResult.snapshotCopyFellBack(),
                            "full-clone createBranch result should not report snapshot-copy fallback"
                    );
                }

                assertBranchMetadata(root, branchName, config);

                if (snapshotCopy) {
                    assertSnapshotFallbackEvent(root, branchName);
                    mutateParentAfterBranchCreate(root, config);
                }

                try (Connection branch = connectWorkload(config, branchName)) {
                    assertEquals(
                            1L,
                            scalarLong(branch, "SELECT COUNT(*) FROM pdb_branch_seed"),
                            "branch should preserve parent seed state at branch creation time"
                    );
                    execute(branch, "INSERT INTO experiment_log(event) VALUES (?)", "agent wrote to branch");
                    branch.commit();
                    assertEquals(
                            1L,
                            scalarLong(branch, "SELECT COUNT(*) FROM experiment_log"),
                            "branch should accept writes from the workload user"
                    );
                }

                client.recordScore(branchName, 0.99, "java integration test passed");
                assertEquals(
                        0.99,
                        scalarDouble(root, "SELECT score FROM pdb_branch_branches WHERE branch_name = ?", branchName),
                        0.0001,
                        "branch score should be recorded"
                );
            } finally {
                ignoreErrors(() -> client.dropBranch(branchName, true));
                ignoreErrors(() -> reopenParentReadWrite(root, config.parentPdb()));
            }
        }
    }

    private static Connection connectRoot(TestConfig config) throws Exception {
        Class.forName("oracle.jdbc.OracleDriver");
        Properties properties = new Properties();
        properties.setProperty("user", config.rootUser());
        properties.setProperty("password", config.rootPassword());
        properties.setProperty("internal_logon", "sysdba");
        Connection connection = DriverManager.getConnection(jdbcUrl(config.rootDsn()), properties);
        connection.setAutoCommit(false);
        return connection;
    }

    private static Connection connectWorkload(TestConfig config, String pdbName) throws Exception {
        Class.forName("oracle.jdbc.OracleDriver");
        String dsn = config.pdbDsn(pdbName);
        long deadlineNanos = System.nanoTime() + config.serviceTimeout().toNanos();
        SQLException lastError = null;

        while (System.nanoTime() < deadlineNanos) {
            try {
                Properties properties = new Properties();
                properties.setProperty("user", config.appUser());
                properties.setProperty("password", config.appPassword());
                Connection connection = DriverManager.getConnection(jdbcUrl(dsn), properties);
                connection.setAutoCommit(false);
                return connection;
            } catch (SQLException exception) {
                lastError = exception;
                Thread.sleep(5000);
            }
        }

        throw new SQLException(
                "timed out waiting for PDB service " + dsn,
                lastError
        );
    }

    private static String jdbcUrl(String dsn) {
        if (dsn.startsWith("jdbc:")) {
            return dsn;
        }
        return "jdbc:oracle:thin:@" + dsn;
    }

    private static void requireCdbRoot(Connection root, TestConfig config) throws SQLException {
        DatabaseFacts facts = collectDatabaseFacts(root, config);
        StringBuilder problems = new StringBuilder();

        if (!"YES".equals(facts.cdb())) {
            problems.append("database is not a CDB");
        }
        if (!"CDB$ROOT".equals(facts.conName())) {
            appendProblem(problems, "connection is in " + facts.conName() + ", not CDB$ROOT");
        }
        if (facts.parentOpenMode() == null) {
            appendProblem(problems, "parent PDB " + config.parentPdb() + " was not found");
        }

        if (!problems.isEmpty()) {
            fail("Oracle integration tests require a CDB root connection: "
                    + problems
                    + "\n"
                    + formatDatabaseFacts(facts));
        }
    }

    private static DatabaseFacts collectDatabaseFacts(Connection root, TestConfig config) throws SQLException {
        String dbCreateFileDest = null;
        String pdbFileNameConvert = null;
        try (PreparedStatement statement = root.prepareStatement(
                """
                SELECT name, value
                  FROM v$parameter
                 WHERE name IN ('db_create_file_dest', 'pdb_file_name_convert')
                """
        );
             ResultSet resultSet = statement.executeQuery()) {
            while (resultSet.next()) {
                if ("db_create_file_dest".equals(resultSet.getString(1))) {
                    dbCreateFileDest = resultSet.getString(2);
                } else if ("pdb_file_name_convert".equals(resultSet.getString(1))) {
                    pdbFileNameConvert = resultSet.getString(2);
                }
            }
        }

        String parentOpenMode = null;
        String parentRestricted = null;
        try (PreparedStatement statement = root.prepareStatement(
                """
                SELECT open_mode, restricted
                  FROM v$pdbs
                 WHERE name = ?
                """
        )) {
            statement.setString(1, config.parentPdb());
            try (ResultSet resultSet = statement.executeQuery()) {
                if (resultSet.next()) {
                    parentOpenMode = resultSet.getString(1);
                    parentRestricted = resultSet.getString(2);
                }
            }
        }

        return new DatabaseFacts(
                config.rootDsn(),
                scalarString(root, "SELECT banner FROM v$version WHERE ROWNUM = 1"),
                scalarString(root, "SELECT cdb FROM v$database"),
                scalarString(root, "SELECT SYS_CONTEXT('USERENV', 'CON_NAME') FROM dual"),
                dbCreateFileDest,
                pdbFileNameConvert,
                config.parentPdb(),
                parentOpenMode,
                parentRestricted
        );
    }

    private static String formatDatabaseFacts(DatabaseFacts facts) {
        return String.join(
                "\n",
                "  dsn: " + facts.dsn(),
                "  banner: " + facts.banner(),
                "  cdb: " + facts.cdb(),
                "  container: " + facts.conName(),
                "  db_create_file_dest: " + nullText(facts.dbCreateFileDest(), "(unset)"),
                "  pdb_file_name_convert: " + nullText(facts.pdbFileNameConvert(), "(unset)"),
                "  parent_pdb: " + facts.parentPdb(),
                "  parent_open_mode: " + nullText(facts.parentOpenMode(), "(missing)"),
                "  parent_restricted: " + nullText(facts.parentRestricted(), "(missing)")
        );
    }

    private static void prepareParentPdb(Connection root, TestConfig config) throws Exception {
        reopenParentReadWrite(root, config.parentPdb());
        execute(root, "ALTER SESSION SET CONTAINER = " + config.parentPdb());

        try {
            executeIgnore(root, "DROP USER " + config.appUser() + " CASCADE", Set.of(1918));
            execute(
                    root,
                    "CREATE USER "
                            + config.appUser()
                            + " IDENTIFIED BY \""
                            + escapeQuoted(config.appPassword())
                            + "\""
            );
            execute(root, "ALTER USER " + config.appUser() + " QUOTA UNLIMITED ON USERS");
            execute(root, "GRANT CREATE SESSION, CREATE TABLE TO " + config.appUser());
        } finally {
            execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT");
        }

        try (Connection parent = connectWorkload(config, config.parentPdb())) {
            execute(
                    parent,
                    "CREATE TABLE pdb_branch_seed (id NUMBER PRIMARY KEY, label VARCHAR2(100) NOT NULL)"
            );
            execute(
                    parent,
                    "CREATE TABLE experiment_log (event VARCHAR2(100) NOT NULL, created_at TIMESTAMP DEFAULT SYSTIMESTAMP NOT NULL)"
            );
            execute(parent, "INSERT INTO pdb_branch_seed(id, label) VALUES (1, 'seed row')");
            parent.commit();
        }

        closePdb(root, config.parentPdb());
        execute(root, "ALTER PLUGGABLE DATABASE " + config.parentPdb() + " OPEN READ ONLY");
    }

    private static void mutateParentAfterBranchCreate(Connection root, TestConfig config) throws Exception {
        reopenParentReadWrite(root, config.parentPdb());
        try (Connection parent = connectWorkload(config, config.parentPdb())) {
            execute(parent, "INSERT INTO pdb_branch_seed(id, label) VALUES (2, 'parent mutation')");
            parent.commit();
            assertEquals(2L, scalarLong(parent, "SELECT COUNT(*) FROM pdb_branch_seed"));
        }
    }

    private static void reopenParentReadWrite(Connection root, String parentPdb) throws SQLException {
        execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT");
        closePdb(root, parentPdb);
        execute(root, "ALTER PLUGGABLE DATABASE " + parentPdb + " OPEN READ WRITE");
    }

    private static void closePdb(Connection root, String pdbName) throws SQLException {
        executeIgnore(root, "ALTER PLUGGABLE DATABASE " + pdbName + " CLOSE IMMEDIATE", Set.of(65020));
    }

    private static void assertBranchMetadata(Connection root, String branchName, TestConfig config) throws SQLException {
        try (PreparedStatement statement = root.prepareStatement(
                "SELECT branch_name, parent_pdb, status FROM pdb_branch_branches WHERE branch_name = ?"
        )) {
            statement.setString(1, branchName);
            try (ResultSet resultSet = statement.executeQuery()) {
                assertTrue(resultSet.next(), "control table should record branch");
                assertEquals(branchName, resultSet.getString(1), "control table should record branch name");
                assertEquals(config.parentPdb(), resultSet.getString(2), "control table should record parent PDB");
                assertEquals("OPEN", resultSet.getString(3), "created branch should be open");
            }
        }
    }

    private static void assertSnapshotFallbackEvent(Connection root, String branchName) throws SQLException {
        String details = scalarString(
                root,
                """
                SELECT warning
                  FROM (
                        SELECT DBMS_LOB.SUBSTR(details, 4000, 1) warning
                          FROM pdb_branch_events
                         WHERE branch_name = ?
                           AND event_type = 'SNAPSHOT_COPY_FALLBACK'
                         ORDER BY event_id DESC
                       )
                 WHERE ROWNUM = 1
                """,
                branchName
        );
        assertTrue(
                details.contains("created with full clone"),
                "snapshot fallback event should explain that a full clone was created"
        );
    }

    private static void execute(Connection connection, String sql, Object... binds) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            for (int i = 0; i < binds.length; i++) {
                statement.setObject(i + 1, binds[i]);
            }
            statement.execute();
        }
    }

    private static void executeIgnore(Connection connection, String sql, Set<Integer> ignoredCodes) throws SQLException {
        try {
            execute(connection, sql);
        } catch (SQLException exception) {
            if (!ignoredCodes.contains(exception.getErrorCode())) {
                throw exception;
            }
        }
    }

    private static String scalarString(Connection connection, String sql, Object... binds) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            for (int i = 0; i < binds.length; i++) {
                statement.setObject(i + 1, binds[i]);
            }
            try (ResultSet resultSet = statement.executeQuery()) {
                assertTrue(resultSet.next(), "query returned no rows: " + sql);
                return resultSet.getString(1);
            }
        }
    }

    private static long scalarLong(Connection connection, String sql, Object... binds) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            for (int i = 0; i < binds.length; i++) {
                statement.setObject(i + 1, binds[i]);
            }
            try (ResultSet resultSet = statement.executeQuery()) {
                assertTrue(resultSet.next(), "query returned no rows: " + sql);
                return resultSet.getLong(1);
            }
        }
    }

    private static double scalarDouble(Connection connection, String sql, Object... binds) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            for (int i = 0; i < binds.length; i++) {
                statement.setObject(i + 1, binds[i]);
            }
            try (ResultSet resultSet = statement.executeQuery()) {
                assertTrue(resultSet.next(), "query returned no rows: " + sql);
                return resultSet.getDouble(1);
            }
        }
    }

    private static void appendProblem(StringBuilder builder, String problem) {
        if (!builder.isEmpty()) {
            builder.append("; ");
        }
        builder.append(problem);
    }

    private static String makeBranchName(String prefix) {
        String suffix = UUID.randomUUID()
                .toString()
                .replace("-", "")
                .substring(0, 8)
                .toUpperCase(Locale.ROOT);
        return simpleName(prefix + suffix, "branch name");
    }

    private static String simpleName(String value, String label) {
        String name = value.trim().toUpperCase(Locale.ROOT);
        if (!NAME_RE.matcher(name).matches()) {
            throw new IllegalArgumentException(label + " must be an unquoted Oracle identifier of 30 chars or fewer");
        }
        return name;
    }

    private static String escapeQuoted(String value) {
        return value.replace("\"", "\"\"");
    }

    private static String nullText(String value, String fallback) {
        return value == null ? fallback : value;
    }

    private static void ignoreErrors(SqlRunnable runnable) {
        try {
            runnable.run();
        } catch (Exception ignored) {
            // Cleanup should not mask the real test failure.
        }
    }

    private interface SqlRunnable {
        void run() throws Exception;
    }

    private record TestConfig(
            String rootDsn,
            String rootUser,
            String rootPassword,
            String branchDsnTemplate,
            String parentPdb,
            String appUser,
            String appPassword,
            Duration serviceTimeout
    ) {
        private static TestConfig fromEnv() {
            return new TestConfig(
                    envOr("PDB_BRANCH_ROOT_DSN", "localhost:1521/FREE"),
                    envOr("PDB_BRANCH_SYS_USER", "sys"),
                    envFirst("PDB_BRANCH_SYS_PASSWORD", "ORACLE_PWD", "PdbBranch1_"),
                    envOr("PDB_BRANCH_BRANCH_DSN_TEMPLATE", "localhost:1521/{branch_name}"),
                    simpleName(envOr("PDB_BRANCH_PARENT_PDB", "FREEPDB1"), "parent PDB"),
                    simpleName(envOr("PDB_BRANCH_APP_USER", "PDB_BRANCH_APP"), "app user"),
                    envOr("PDB_BRANCH_APP_PASSWORD", "PdbBranch1_"),
                    Duration.ofSeconds(Long.parseLong(envOr("PDB_BRANCH_SERVICE_TIMEOUT_SECONDS", "120")))
            );
        }

        private String pdbDsn(String pdbName) {
            return branchDsnTemplate.replace("{branch_name}", pdbName);
        }
    }

    private record DatabaseFacts(
            String dsn,
            String banner,
            String cdb,
            String conName,
            String dbCreateFileDest,
            String pdbFileNameConvert,
            String parentPdb,
            String parentOpenMode,
            String parentRestricted
    ) {
    }

    private static String envOr(String name, String fallback) {
        String value = System.getenv(name);
        return value == null ? fallback : value;
    }

    private static String envFirst(String first, String second, String fallback) {
        String value = System.getenv(first);
        if (value != null) {
            return value;
        }
        return envOr(second, fallback);
    }
}
