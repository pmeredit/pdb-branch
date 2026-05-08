package dev.pdbbranch;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

public final class SqlScripts {
    private static final List<String> SCRIPT_NAMES = List.of("001_tables.sql", "002_package.sql");

    private SqlScripts() {
    }

    public static List<String> scriptNames() {
        return SCRIPT_NAMES;
    }

    public static String readSharedScript(String scriptName) throws IOException {
        Path path = Path.of("..", "..", "sql", scriptName).normalize();
        if (!Files.exists(path)) {
            path = Path.of("sql", scriptName).normalize();
        }
        return Files.readString(path, StandardCharsets.UTF_8);
    }

    public static List<String> splitSqlPlusScript(String script) {
        List<String> statements = new ArrayList<>();
        List<String> current = new ArrayList<>();

        for (String line : script.split("\\R", -1)) {
            if (line.trim().equals("/")) {
                String statement = String.join("\n", current).trim();
                if (!statement.isEmpty()) {
                    statements.add(statement);
                }
                current.clear();
            } else {
                current.add(stripTrailingWhitespace(line));
            }
        }

        String trailing = String.join("\n", current).trim();
        if (!trailing.isEmpty()) {
            statements.add(trailing);
        }

        return statements;
    }

    private static String stripTrailingWhitespace(String value) {
        return value.replaceFirst("\\s+$", "");
    }
}
