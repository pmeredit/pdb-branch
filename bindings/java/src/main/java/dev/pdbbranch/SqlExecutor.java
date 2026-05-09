package dev.pdbbranch;

import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLException;
import java.util.List;
import java.util.Optional;

public interface SqlExecutor {
    void execute(String sql, List<Object> binds) throws SQLException;

    default Optional<String> queryOptionalString(String sql, List<Object> binds) throws SQLException {
        throw new SQLFeatureNotSupportedException("executor does not support scalar string queries");
    }

    default void commit() throws SQLException {
    }
}
