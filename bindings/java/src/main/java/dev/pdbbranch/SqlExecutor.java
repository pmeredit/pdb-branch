package dev.pdbbranch;

import java.sql.SQLException;
import java.util.List;

public interface SqlExecutor {
    void execute(String sql, List<Object> binds) throws SQLException;

    default void commit() throws SQLException {
    }
}
