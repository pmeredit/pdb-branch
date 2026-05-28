use async_trait::async_trait;
use std::fmt::Debug;
use thiserror::Error;

const SQL_SCRIPTS: &[(&str, &str)] = &[
    (
        "001_tables.sql",
        include_str!("../../../sql/001_tables.sql"),
    ),
    (
        "002_package.sql",
        include_str!("../../../sql/002_package.sql"),
    ),
];

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database execution failed: {0}")]
    Database(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindValue {
    Null,
    String(String),
    Number(f64),
}

impl From<&str> for BindValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for BindValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<f64> for BindValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<i64> for BindValue {
    fn from(value: i64) -> Self {
        Self::Number(value as f64)
    }
}

#[cfg(feature = "oracle-rs")]
pub struct OracleRsExecutor {
    connection: oracle_rs::Connection,
}

#[cfg(feature = "oracle-rs")]
impl OracleRsExecutor {
    pub fn new(connection: oracle_rs::Connection) -> Self {
        Self { connection }
    }

    pub fn connection(&self) -> &oracle_rs::Connection {
        &self.connection
    }
}

#[cfg(feature = "oracle-rs")]
#[async_trait]
impl SqlExecutor for OracleRsExecutor {
    async fn execute(&self, sql: &str, binds: &[BindValue]) -> Result<()> {
        let values = binds.iter().map(to_oracle_rs_value).collect::<Vec<_>>();
        self.connection
            .execute(sql, &values)
            .await
            .map(|_| ())
            .map_err(|err| Error::Database(err.to_string()))
    }

    async fn commit(&self) -> Result<()> {
        self.connection
            .commit()
            .await
            .map_err(|err| Error::Database(err.to_string()))
    }

    async fn query_optional_string(
        &self,
        sql: &str,
        binds: &[BindValue],
    ) -> Result<Option<String>> {
        let values = binds.iter().map(to_oracle_rs_value).collect::<Vec<_>>();
        let result = self
            .connection
            .query(sql, &values)
            .await
            .map_err(|err| Error::Database(err.to_string()))?;

        Ok(result
            .rows
            .first()
            .and_then(|row| row.get_string(0))
            .map(str::to_owned))
    }

    async fn query_rows(&self, sql: &str, binds: &[BindValue]) -> Result<Vec<Vec<Option<String>>>> {
        let values = binds.iter().map(to_oracle_rs_value).collect::<Vec<_>>();
        let result = self
            .connection
            .query(sql, &values)
            .await
            .map_err(|err| Error::Database(err.to_string()))?;

        Ok(result
            .rows
            .iter()
            .map(|row| {
                row.values()
                    .iter()
                    .map(|value| {
                        if value.is_null() {
                            None
                        } else {
                            Some(value.to_string())
                        }
                    })
                    .collect()
            })
            .collect())
    }
}

#[cfg(feature = "oracle-rs")]
fn to_oracle_rs_value(value: &BindValue) -> oracle_rs::Value {
    match value {
        BindValue::Null => oracle_rs::Value::Null,
        BindValue::String(value) => oracle_rs::Value::String(value.clone()),
        BindValue::Number(value) => oracle_rs::Value::Float(*value),
    }
}

#[cfg(feature = "rust-oracle")]
pub struct RustOracleExecutor {
    connection: oracle::Connection,
}

#[cfg(feature = "rust-oracle")]
impl RustOracleExecutor {
    pub fn new(connection: oracle::Connection) -> Self {
        Self { connection }
    }

    pub fn connection(&self) -> &oracle::Connection {
        &self.connection
    }
}

#[cfg(feature = "rust-oracle")]
#[async_trait]
impl SqlExecutor for RustOracleExecutor {
    async fn execute(&self, sql: &str, binds: &[BindValue]) -> Result<()> {
        use oracle::sql_type::ToSql;

        let values = binds.iter().map(RustOracleBind::from).collect::<Vec<_>>();
        let params = values
            .iter()
            .map(|value| match value {
                RustOracleBind::Null(value) => value as &dyn ToSql,
                RustOracleBind::String(value) => value as &dyn ToSql,
                RustOracleBind::Number(value) => value as &dyn ToSql,
            })
            .collect::<Vec<_>>();

        self.connection
            .execute(sql, &params)
            .map(|_| ())
            .map_err(|err| Error::Database(err.to_string()))
    }

    async fn commit(&self) -> Result<()> {
        self.connection
            .commit()
            .map_err(|err| Error::Database(err.to_string()))
    }

    async fn query_optional_string(
        &self,
        sql: &str,
        binds: &[BindValue],
    ) -> Result<Option<String>> {
        use oracle::sql_type::ToSql;

        let values = binds.iter().map(RustOracleBind::from).collect::<Vec<_>>();
        let params = values
            .iter()
            .map(|value| match value {
                RustOracleBind::Null(value) => value as &dyn ToSql,
                RustOracleBind::String(value) => value as &dyn ToSql,
                RustOracleBind::Number(value) => value as &dyn ToSql,
            })
            .collect::<Vec<_>>();

        let mut rows = self
            .connection
            .query(sql, &params)
            .map_err(|err| Error::Database(err.to_string()))?;
        let Some(row) = rows.next() else {
            return Ok(None);
        };
        let row = row.map_err(|err| Error::Database(err.to_string()))?;
        row.get::<_, Option<String>>(0)
            .map_err(|err| Error::Database(err.to_string()))
    }

    async fn query_rows(&self, sql: &str, binds: &[BindValue]) -> Result<Vec<Vec<Option<String>>>> {
        use oracle::sql_type::ToSql;

        let values = binds.iter().map(RustOracleBind::from).collect::<Vec<_>>();
        let params = values
            .iter()
            .map(|value| match value {
                RustOracleBind::Null(value) => value as &dyn ToSql,
                RustOracleBind::String(value) => value as &dyn ToSql,
                RustOracleBind::Number(value) => value as &dyn ToSql,
            })
            .collect::<Vec<_>>();

        let mut rows = self
            .connection
            .query(sql, &params)
            .map_err(|err| Error::Database(err.to_string()))?;
        let mut result = Vec::new();

        while let Some(row) = rows.next() {
            let row = row.map_err(|err| Error::Database(err.to_string()))?;
            let mut values = Vec::with_capacity(row.column_info().len());
            for index in 0..row.column_info().len() {
                values.push(
                    row.get::<_, Option<String>>(index)
                        .map_err(|err| Error::Database(err.to_string()))?,
                );
            }
            result.push(values);
        }

        Ok(result)
    }
}

#[cfg(feature = "rust-oracle")]
enum RustOracleBind {
    Null(Option<String>),
    String(String),
    Number(f64),
}

#[cfg(feature = "rust-oracle")]
impl From<&BindValue> for RustOracleBind {
    fn from(value: &BindValue) -> Self {
        match value {
            BindValue::Null => Self::Null(None),
            BindValue::String(value) => Self::String(value.clone()),
            BindValue::Number(value) => Self::Number(*value),
        }
    }
}

#[async_trait]
pub trait SqlExecutor {
    async fn execute(&self, sql: &str, binds: &[BindValue]) -> Result<()>;

    async fn query_optional_string(
        &self,
        _sql: &str,
        _binds: &[BindValue],
    ) -> Result<Option<String>> {
        Err(Error::Database(
            "executor does not support scalar string queries".to_owned(),
        ))
    }

    async fn query_rows(
        &self,
        _sql: &str,
        _binds: &[BindValue],
    ) -> Result<Vec<Vec<Option<String>>>> {
        Err(Error::Database(
            "executor does not support row queries".to_owned(),
        ))
    }

    async fn commit(&self) -> Result<()> {
        Ok(())
    }
}

pub struct BranchClient<E> {
    executor: E,
}

impl<E> BranchClient<E>
where
    E: SqlExecutor + Send + Sync,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn ensure_installed(&self) -> Result<()> {
        for (_, script) in SQL_SCRIPTS {
            for statement in split_sqlplus_script(script) {
                self.executor.execute(&statement, &[]).await?;
            }
        }
        self.executor.commit().await
    }

    pub async fn create_branch(&self, branch_name: &str, options: BranchOptions<'_>) -> Result<()> {
        self.call(
            "pdb_branch.create_branch",
            &[
                branch_name.into(),
                options.from_pdb.into(),
                yn(options.snapshot_copy).into(),
                yn(options.open_branch).into(),
                optional(options.profile_name),
                optional(options.expires_at),
                optional(options.notes),
            ],
        )
        .await
    }

    pub async fn create_branch_with_result(
        &self,
        branch_name: &str,
        options: BranchOptions<'_>,
    ) -> Result<CreateBranchResult> {
        let snapshot_copy_requested = options.snapshot_copy;
        let last_event_id = if snapshot_copy_requested {
            self.max_event_id(branch_name).await?
        } else {
            None
        };

        self.create_branch(branch_name, options).await?;

        let fallback_warning = if snapshot_copy_requested {
            self.snapshot_fallback_warning(branch_name, last_event_id)
                .await?
        } else {
            None
        };

        Ok(CreateBranchResult {
            snapshot_copy_requested,
            snapshot_copy_fell_back: fallback_warning.is_some(),
            fallback_warning,
        })
    }

    pub async fn clone_branch_from_remote(
        &self,
        branch_name: &str,
        options: RemoteCloneOptions<'_>,
    ) -> Result<()> {
        self.call(
            "pdb_branch.clone_branch_from_remote",
            &[
                branch_name.into(),
                options.source_pdb.into(),
                options.source_db_link.into(),
                options.clone_mode.into(),
                yn(options.open_branch).into(),
                optional(options.profile_name),
                optional(options.expires_at),
                optional(options.notes),
                optional(options.create_file_dest),
            ],
        )
        .await
    }

    pub async fn clone_branch_from_remote_with_result(
        &self,
        branch_name: &str,
        options: RemoteCloneOptions<'_>,
    ) -> Result<RemoteCloneResult> {
        let clone_mode = options.clone_mode.to_owned();
        let tracks_fallback = options.clone_mode.eq_ignore_ascii_case("AUTO");
        let snapshot_copy_requested =
            tracks_fallback || options.clone_mode.eq_ignore_ascii_case("SNAPSHOT");
        let last_event_id = if tracks_fallback {
            self.max_event_id(branch_name).await?
        } else {
            None
        };

        self.clone_branch_from_remote(branch_name, options).await?;

        let fallback_warning = if tracks_fallback {
            self.remote_snapshot_fallback_warning(branch_name, last_event_id)
                .await?
        } else {
            None
        };

        Ok(RemoteCloneResult {
            clone_mode,
            snapshot_copy_requested,
            snapshot_copy_fell_back: fallback_warning.is_some(),
            fallback_warning,
        })
    }

    pub async fn open_branch(&self, branch_name: &str, profile_name: Option<&str>) -> Result<()> {
        self.call(
            "pdb_branch.open_branch",
            &[branch_name.into(), optional(profile_name)],
        )
        .await
    }

    pub async fn close_branch(&self, branch_name: &str, immediate: bool) -> Result<()> {
        self.call(
            "pdb_branch.close_branch",
            &[branch_name.into(), yn(immediate).into()],
        )
        .await
    }

    pub async fn drop_branch(&self, branch_name: &str, including_datafiles: bool) -> Result<()> {
        self.call(
            "pdb_branch.drop_branch",
            &[branch_name.into(), yn(including_datafiles).into()],
        )
        .await
    }

    pub async fn set_profile(
        &self,
        branch_name: &str,
        profile_name: &str,
        reopen: bool,
    ) -> Result<()> {
        self.call(
            "pdb_branch.set_profile",
            &[branch_name.into(), profile_name.into(), yn(reopen).into()],
        )
        .await
    }

    pub async fn record_activity(&self, branch_name: &str, status: Option<&str>) -> Result<()> {
        self.call(
            "pdb_branch.record_activity",
            &[branch_name.into(), optional(status)],
        )
        .await
    }

    pub async fn record_score(
        &self,
        branch_name: &str,
        score: f64,
        notes: Option<&str>,
    ) -> Result<()> {
        self.call(
            "pdb_branch.record_score",
            &[branch_name.into(), score.into(), optional(notes)],
        )
        .await
    }

    pub async fn promote(&self, branch_name: &str, notes: Option<&str>) -> Result<()> {
        self.call(
            "pdb_branch.promote_branch",
            &[branch_name.into(), optional(notes)],
        )
        .await
    }

    pub async fn cleanup(&self, options: CleanupOptions) -> Result<()> {
        self.call(
            "pdb_branch.cleanup",
            &[
                (options.close_idle_after_minutes as i64).into(),
                yn(options.drop_expired).into(),
            ],
        )
        .await
    }

    pub async fn configure_resource_plan(&self, options: ResourcePlanOptions<'_>) -> Result<()> {
        self.call(
            "pdb_branch.configure_resource_plan",
            &[options.plan_name.into(), yn(options.activate).into()],
        )
        .await
    }

    pub async fn get_branch(&self, branch_name: &str) -> Result<Option<BranchInfo>> {
        let sql = branch_select_sql("WHERE branch_name = UPPER(:1)");
        let rows = self
            .executor
            .query_rows(&sql, &[branch_name.into()])
            .await?;

        rows.into_iter()
            .next()
            .map(BranchInfo::from_row)
            .transpose()
    }

    pub async fn list_branches(&self, include_dropped: bool) -> Result<Vec<BranchInfo>> {
        let where_clause = if include_dropped {
            ""
        } else {
            "WHERE status <> 'DROPPED'"
        };
        let sql = branch_select_sql(where_clause);
        let rows = self.executor.query_rows(&sql, &[]).await?;

        rows.into_iter().map(BranchInfo::from_row).collect()
    }

    async fn call(&self, name: &str, binds: &[BindValue]) -> Result<()> {
        let placeholders = (1..=binds.len())
            .map(|i| format!(":{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("BEGIN {name}({placeholders}); END;");
        self.executor.execute(&sql, binds).await
    }

    async fn max_event_id(&self, branch_name: &str) -> Result<Option<i64>> {
        self.executor
            .query_optional_string(
                "
                SELECT TO_CHAR(MAX(event_id))
                  FROM pdb_branch_events
                 WHERE branch_name = UPPER(:1)
                ",
                &[branch_name.into()],
            )
            .await?
            .map(|value| {
                value
                    .parse::<i64>()
                    .map_err(|err| Error::Database(format!("invalid event_id value: {err}")))
            })
            .transpose()
    }

    async fn snapshot_fallback_warning(
        &self,
        branch_name: &str,
        last_event_id: Option<i64>,
    ) -> Result<Option<String>> {
        self.executor
            .query_optional_string(
                "
                SELECT warning
                  FROM (
                        SELECT DBMS_LOB.SUBSTR(details, 4000, 1) warning
                          FROM pdb_branch_events
                         WHERE branch_name = UPPER(:1)
                           AND event_type = 'SNAPSHOT_COPY_FALLBACK'
                           AND (:2 IS NULL OR event_id > :2)
                         ORDER BY event_id DESC
                       )
                 WHERE ROWNUM = 1
                ",
                &[
                    branch_name.into(),
                    last_event_id.map_or(BindValue::Null, BindValue::from),
                ],
            )
            .await
    }

    async fn remote_snapshot_fallback_warning(
        &self,
        branch_name: &str,
        last_event_id: Option<i64>,
    ) -> Result<Option<String>> {
        self.executor
            .query_optional_string(
                "
                SELECT warning
                  FROM (
                        SELECT DBMS_LOB.SUBSTR(details, 4000, 1) warning
                          FROM pdb_branch_events
                         WHERE branch_name = UPPER(:1)
                           AND event_type = 'REMOTE_SNAPSHOT_COPY_FALLBACK'
                           AND (:2 IS NULL OR event_id > :2)
                         ORDER BY event_id DESC
                       )
                 WHERE ROWNUM = 1
                ",
                &[
                    branch_name.into(),
                    last_event_id.map_or(BindValue::Null, BindValue::from),
                ],
            )
            .await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateBranchResult {
    pub snapshot_copy_requested: bool,
    pub snapshot_copy_fell_back: bool,
    pub fallback_warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteCloneResult {
    pub clone_mode: String,
    pub snapshot_copy_requested: bool,
    pub snapshot_copy_fell_back: bool,
    pub fallback_warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchInfo {
    pub branch_name: String,
    pub parent_pdb: Option<String>,
    pub status: String,
    pub profile_name: Option<String>,
    pub created_at: Option<String>,
    pub opened_at: Option<String>,
    pub closed_at: Option<String>,
    pub dropped_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub expires_at: Option<String>,
    pub score: Option<f64>,
    pub notes: Option<String>,
}

impl BranchInfo {
    fn from_row(mut row: Vec<Option<String>>) -> Result<Self> {
        if row.len() != 12 {
            return Err(Error::Database(format!(
                "expected 12 branch columns, got {}",
                row.len()
            )));
        }

        let notes = row.pop().flatten();
        let score = row
            .pop()
            .flatten()
            .map(|value| {
                value
                    .parse::<f64>()
                    .map_err(|err| Error::Database(format!("invalid score value: {err}")))
            })
            .transpose()?;
        let expires_at = row.pop().flatten();
        let last_activity_at = row.pop().flatten();
        let dropped_at = row.pop().flatten();
        let closed_at = row.pop().flatten();
        let opened_at = row.pop().flatten();
        let created_at = row.pop().flatten();
        let profile_name = row.pop().flatten();
        let status = required_column(row.pop().flatten(), "status")?;
        let parent_pdb = row.pop().flatten();
        let branch_name = required_column(row.pop().flatten(), "branch_name")?;

        Ok(Self {
            branch_name,
            parent_pdb,
            status,
            profile_name,
            created_at,
            opened_at,
            closed_at,
            dropped_at,
            last_activity_at,
            expires_at,
            score,
            notes,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BranchOptions<'a> {
    pub from_pdb: &'a str,
    pub snapshot_copy: bool,
    pub open_branch: bool,
    pub profile_name: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub notes: Option<&'a str>,
}

impl Default for BranchOptions<'_> {
    fn default() -> Self {
        Self {
            from_pdb: "GOLDEN_MASTER",
            snapshot_copy: true,
            open_branch: true,
            profile_name: None,
            expires_at: None,
            notes: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteCloneOptions<'a> {
    pub source_pdb: &'a str,
    pub source_db_link: &'a str,
    pub clone_mode: &'a str,
    pub open_branch: bool,
    pub profile_name: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub create_file_dest: Option<&'a str>,
}

impl Default for RemoteCloneOptions<'_> {
    fn default() -> Self {
        Self {
            source_pdb: "GOLDEN_MASTER",
            source_db_link: "PDB_BRANCH_SOURCE",
            clone_mode: "FULL",
            open_branch: true,
            profile_name: None,
            expires_at: None,
            notes: None,
            create_file_dest: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CleanupOptions {
    pub close_idle_after_minutes: i64,
    pub drop_expired: bool,
}

impl Default for CleanupOptions {
    fn default() -> Self {
        Self {
            close_idle_after_minutes: 60,
            drop_expired: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResourcePlanOptions<'a> {
    pub plan_name: &'a str,
    pub activate: bool,
}

impl Default for ResourcePlanOptions<'_> {
    fn default() -> Self {
        Self {
            plan_name: "PDB_BRANCH_PLAN",
            activate: false,
        }
    }
}

pub fn split_sqlplus_script(script: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = Vec::new();

    for line in script.lines() {
        if line.trim() == "/" {
            let statement = current.join("\n").trim().to_owned();
            if !statement.is_empty() {
                statements.push(statement);
            }
            current.clear();
        } else {
            current.push(line.trim_end());
        }
    }

    let trailing = current.join("\n").trim().to_owned();
    if !trailing.is_empty() {
        statements.push(trailing);
    }

    statements
}

fn yn(value: bool) -> &'static str {
    if value {
        "Y"
    } else {
        "N"
    }
}

fn optional(value: Option<&str>) -> BindValue {
    value.map_or(BindValue::Null, BindValue::from)
}

fn branch_select_sql(where_clause: &str) -> String {
    format!(
        "
        SELECT branch_name,
               parent_pdb,
               status,
               profile_name,
               TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') created_at,
               TO_CHAR(opened_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') opened_at,
               TO_CHAR(closed_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') closed_at,
               TO_CHAR(dropped_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') dropped_at,
               TO_CHAR(last_activity_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') last_activity_at,
               TO_CHAR(expires_at, 'YYYY-MM-DD HH24:MI:SS TZH:TZM') expires_at,
               TO_CHAR(score) score,
               DBMS_LOB.SUBSTR(notes, 4000, 1) notes
          FROM pdb_branch_branches
          {where_clause}
         ORDER BY created_at DESC
        "
    )
}

fn required_column(value: Option<String>, column: &str) -> Result<String> {
    value.ok_or_else(|| Error::Database(format!("{column} column was NULL")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeExecutor {
        executions: Arc<Mutex<Vec<(String, Vec<BindValue>)>>>,
        queries: Arc<Mutex<Vec<(String, Vec<BindValue>)>>>,
        query_results: Arc<Mutex<Vec<Option<String>>>>,
        row_query_results: Arc<Mutex<Vec<Vec<Vec<Option<String>>>>>>,
        commits: Arc<Mutex<u32>>,
    }

    impl FakeExecutor {
        fn with_query_results(results: Vec<Option<&str>>) -> Self {
            Self {
                query_results: Arc::new(Mutex::new(
                    results
                        .into_iter()
                        .map(|value| value.map(str::to_owned))
                        .collect(),
                )),
                ..Self::default()
            }
        }

        fn with_row_query_results(results: Vec<Vec<Vec<Option<&str>>>>) -> Self {
            Self {
                row_query_results: Arc::new(Mutex::new(
                    results
                        .into_iter()
                        .map(|rows| {
                            rows.into_iter()
                                .map(|row| {
                                    row.into_iter()
                                        .map(|value| value.map(str::to_owned))
                                        .collect()
                                })
                                .collect()
                        })
                        .collect(),
                )),
                ..Self::default()
            }
        }
    }

    #[async_trait]
    impl SqlExecutor for FakeExecutor {
        async fn execute(&self, sql: &str, binds: &[BindValue]) -> Result<()> {
            self.executions
                .lock()
                .unwrap()
                .push((sql.to_owned(), binds.to_vec()));
            Ok(())
        }

        async fn query_optional_string(
            &self,
            sql: &str,
            binds: &[BindValue],
        ) -> Result<Option<String>> {
            self.queries
                .lock()
                .unwrap()
                .push((sql.to_owned(), binds.to_vec()));
            Ok(self.query_results.lock().unwrap().remove(0))
        }

        async fn query_rows(
            &self,
            sql: &str,
            binds: &[BindValue],
        ) -> Result<Vec<Vec<Option<String>>>> {
            self.queries
                .lock()
                .unwrap()
                .push((sql.to_owned(), binds.to_vec()));
            Ok(self.row_query_results.lock().unwrap().remove(0))
        }

        async fn commit(&self) -> Result<()> {
            *self.commits.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[cfg(feature = "oracle-rs")]
    #[test]
    fn oracle_rs_executor_satisfies_client_bounds() {
        fn assert_executor<T: SqlExecutor + Send + Sync>() {}
        assert_executor::<OracleRsExecutor>();
    }

    #[cfg(feature = "rust-oracle")]
    #[test]
    fn rust_oracle_executor_satisfies_client_bounds() {
        fn assert_executor<T: SqlExecutor + Send + Sync>() {}
        assert_executor::<RustOracleExecutor>();
    }

    #[test]
    fn splits_sqlplus_script_on_slash_terminators() {
        let script = "\nCREATE TABLE demo (id NUMBER)\n/\nBEGIN\n  NULL;\nEND;\n/\n";

        assert_eq!(
            split_sqlplus_script(script),
            vec!["CREATE TABLE demo (id NUMBER)", "BEGIN\n  NULL;\nEND;"]
        );
    }

    #[tokio::test]
    async fn create_branch_calls_plsql_package() {
        let executor = FakeExecutor::default();
        let client = BranchClient::new(executor.clone());

        client
            .create_branch(
                "AGENT_RAG_042",
                BranchOptions {
                    notes: Some("try chunking"),
                    ..BranchOptions::default()
                },
            )
            .await
            .unwrap();

        let executions = executor.executions.lock().unwrap();
        assert_eq!(executions.len(), 1);
        assert_eq!(
            executions[0].0,
            "BEGIN pdb_branch.create_branch(:1, :2, :3, :4, :5, :6, :7); END;"
        );
        assert_eq!(
            executions[0].1,
            vec![
                "AGENT_RAG_042".into(),
                "GOLDEN_MASTER".into(),
                "Y".into(),
                "Y".into(),
                BindValue::Null,
                BindValue::Null,
                "try chunking".into(),
            ]
        );
    }

    #[tokio::test]
    async fn clone_branch_from_remote_calls_plsql_package() {
        let executor = FakeExecutor::default();
        let client = BranchClient::new(executor.clone());

        client
            .clone_branch_from_remote(
                "AGENT_RAG_042",
                RemoteCloneOptions {
                    source_pdb: "SOURCE_BRANCH",
                    source_db_link: "PDB_BRANCH_SOURCE",
                    clone_mode: "AUTO",
                    notes: Some("push from origin"),
                    create_file_dest: Some("/opt/oracle/oradata/FREE"),
                    ..RemoteCloneOptions::default()
                },
            )
            .await
            .unwrap();

        let executions = executor.executions.lock().unwrap();
        assert_eq!(executions.len(), 1);
        assert_eq!(
            executions[0].0,
            "BEGIN pdb_branch.clone_branch_from_remote(:1, :2, :3, :4, :5, :6, :7, :8, :9); END;"
        );
        assert_eq!(
            executions[0].1,
            vec![
                "AGENT_RAG_042".into(),
                "SOURCE_BRANCH".into(),
                "PDB_BRANCH_SOURCE".into(),
                "AUTO".into(),
                "Y".into(),
                BindValue::Null,
                BindValue::Null,
                "push from origin".into(),
                "/opt/oracle/oradata/FREE".into(),
            ]
        );
    }

    #[tokio::test]
    async fn clone_branch_from_remote_with_result_reports_auto_fallback() {
        let executor = FakeExecutor::with_query_results(vec![
            Some("12"),
            Some("WARNING: remote SNAPSHOT COPY requested with clone mode AUTO; pushed with full clone"),
        ]);
        let client = BranchClient::new(executor.clone());

        let result = client
            .clone_branch_from_remote_with_result(
                "AGENT_RAG_042",
                RemoteCloneOptions {
                    source_pdb: "SOURCE_BRANCH",
                    source_db_link: "PDB_BRANCH_SOURCE",
                    clone_mode: "AUTO",
                    ..RemoteCloneOptions::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(
            result,
            RemoteCloneResult {
                clone_mode: "AUTO".to_owned(),
                snapshot_copy_requested: true,
                snapshot_copy_fell_back: true,
                fallback_warning: Some(
                    "WARNING: remote SNAPSHOT COPY requested with clone mode AUTO; pushed with full clone"
                        .to_owned()
                ),
            }
        );

        let queries = executor.queries.lock().unwrap();
        assert_eq!(queries.len(), 2);
        assert!(queries[0].0.contains("MAX(event_id)"));
        assert!(queries[1].0.contains("REMOTE_SNAPSHOT_COPY_FALLBACK"));
        assert_eq!(
            queries[1].1,
            vec!["AGENT_RAG_042".into(), BindValue::Number(12.0)]
        );
    }

    #[tokio::test]
    async fn create_branch_with_result_reports_snapshot_fallback() {
        let executor = FakeExecutor::with_query_results(vec![
            Some("10"),
            Some("WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone"),
        ]);
        let client = BranchClient::new(executor.clone());

        let result = client
            .create_branch_with_result(
                "AGENT_RAG_042",
                BranchOptions {
                    snapshot_copy: true,
                    ..BranchOptions::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(
            result,
            CreateBranchResult {
                snapshot_copy_requested: true,
                snapshot_copy_fell_back: true,
                fallback_warning: Some(
                    "WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone"
                        .to_owned()
                ),
            }
        );

        let queries = executor.queries.lock().unwrap();
        assert_eq!(queries.len(), 2);
        assert!(queries[0].0.contains("MAX(event_id)"));
        assert!(queries[1].0.contains("SNAPSHOT_COPY_FALLBACK"));
        assert_eq!(
            queries[1].1,
            vec!["AGENT_RAG_042".into(), BindValue::Number(10.0)]
        );
    }

    #[tokio::test]
    async fn ensure_installed_executes_shared_sql_and_commits() {
        let executor = FakeExecutor::default();
        let client = BranchClient::new(executor.clone());

        client.ensure_installed().await.unwrap();

        assert_eq!(executor.executions.lock().unwrap().len(), 6);
        assert_eq!(*executor.commits.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn list_branches_maps_rows() {
        let executor = FakeExecutor::with_row_query_results(vec![vec![vec![
            Some("AGENT_RAG_042"),
            Some("GOLDEN_MASTER"),
            Some("OPEN"),
            Some("PDB_BRANCH_ACTIVE"),
            Some("2026-05-09 10:00:00 +00:00"),
            Some("2026-05-09 10:01:00 +00:00"),
            None,
            None,
            Some("2026-05-09 10:02:00 +00:00"),
            None,
            Some("0.91"),
            Some("eval passed"),
        ]]]);
        let client = BranchClient::new(executor.clone());

        let branches = client.list_branches(false).await.unwrap();

        assert_eq!(
            branches,
            vec![BranchInfo {
                branch_name: "AGENT_RAG_042".to_owned(),
                parent_pdb: Some("GOLDEN_MASTER".to_owned()),
                status: "OPEN".to_owned(),
                profile_name: Some("PDB_BRANCH_ACTIVE".to_owned()),
                created_at: Some("2026-05-09 10:00:00 +00:00".to_owned()),
                opened_at: Some("2026-05-09 10:01:00 +00:00".to_owned()),
                closed_at: None,
                dropped_at: None,
                last_activity_at: Some("2026-05-09 10:02:00 +00:00".to_owned()),
                expires_at: None,
                score: Some(0.91),
                notes: Some("eval passed".to_owned()),
            }]
        );

        let queries = executor.queries.lock().unwrap();
        assert!(queries[0].0.contains("WHERE status <> 'DROPPED'"));
    }
}
