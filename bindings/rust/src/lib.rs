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

#[async_trait]
pub trait SqlExecutor {
    async fn execute(&self, sql: &str, binds: &[BindValue]) -> Result<()>;

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

    async fn call(&self, name: &str, binds: &[BindValue]) -> Result<()> {
        let placeholders = (1..=binds.len())
            .map(|i| format!(":{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("BEGIN {name}({placeholders}); END;");
        self.executor.execute(&sql, binds).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeExecutor {
        executions: Arc<Mutex<Vec<(String, Vec<BindValue>)>>>,
        commits: Arc<Mutex<u32>>,
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

        async fn commit(&self) -> Result<()> {
            *self.commits.lock().unwrap() += 1;
            Ok(())
        }
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
    async fn ensure_installed_executes_shared_sql_and_commits() {
        let executor = FakeExecutor::default();
        let client = BranchClient::new(executor.clone());

        client.ensure_installed().await.unwrap();

        assert_eq!(executor.executions.lock().unwrap().len(), 6);
        assert_eq!(*executor.commits.lock().unwrap(), 1);
    }
}
