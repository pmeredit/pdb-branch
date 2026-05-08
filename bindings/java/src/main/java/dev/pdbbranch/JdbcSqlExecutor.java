package dev.pdbbranch;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.util.List;

public final class JdbcSqlExecutor implements SqlExecutor {
    private final Connection connection;

    public JdbcSqlExecutor(Connection connection) {
        this.connection = connection;
    }

    @Override
    public void execute(String sql, List<Object> binds) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(sql)) {
            for (int i = 0; i < binds.size(); i++) {
                statement.setObject(i + 1, binds.get(i));
            }
            statement.execute();
        }
    }

    @Override
    public void commit() throws SQLException {
        connection.commit();
    }
}
