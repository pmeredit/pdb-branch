#![cfg(feature = "oracle-rs")]

use oracle_rs::{Connection, Row, Value};
use pdb_branch::{BranchClient, BranchOptions, OracleRsExecutor};
use std::env;
use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::process;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
struct TestConfig {
    root_dsn: String,
    root_user: String,
    root_password: String,
    branch_dsn_template: String,
    parent_pdb: String,
    app_user: String,
    app_password: String,
    service_timeout: Duration,
}

#[derive(Debug)]
struct DatabaseFacts {
    dsn: String,
    banner: String,
    cdb: String,
    con_name: String,
    db_create_file_dest: Option<String>,
    pdb_file_name_convert: Option<String>,
    parent_pdb: String,
    parent_open_mode: Option<String>,
    parent_restricted: Option<String>,
}

#[tokio::test(flavor = "current_thread")]
async fn oracle_free_branch_lifecycle() -> TestResult {
    if env::var("PDB_BRANCH_INTEGRATION").ok().as_deref() != Some("1") {
        eprintln!("set PDB_BRANCH_INTEGRATION=1 to run Oracle Rust integration tests");
        return Ok(());
    }

    run_branch_lifecycle(false).await?;

    if env::var("PDB_BRANCH_TEST_SNAPSHOT_COPY").ok().as_deref() == Some("1") {
        run_branch_lifecycle(true).await?;
    }

    Ok(())
}

async fn run_branch_lifecycle(snapshot_copy: bool) -> TestResult {
    let config = TestConfig::from_env()?;
    let root = connect(&config.root_dsn, &config.root_user, &config.root_password).await?;
    let client_root = connect(&config.root_dsn, &config.root_user, &config.root_password).await?;
    let client = BranchClient::new(OracleRsExecutor::new(client_root));
    let branch_name = make_branch_name(if snapshot_copy { "RBISC" } else { "RBIFC" })?;

    let result =
        run_branch_lifecycle_inner(&config, &root, &client, &branch_name, snapshot_copy).await;

    let _ = client.drop_branch(&branch_name, true).await;
    let _ = reopen_parent_read_write(&root, &config.parent_pdb).await;

    result
}

async fn run_branch_lifecycle_inner(
    config: &TestConfig,
    root: &Connection,
    client: &BranchClient<OracleRsExecutor>,
    branch_name: &str,
    snapshot_copy: bool,
) -> TestResult {
    require_cdb_root(root, config).await?;
    client.ensure_installed().await?;
    prepare_parent_pdb(root, config).await?;

    if let Err(err) = client
        .create_branch(
            branch_name,
            BranchOptions {
                from_pdb: &config.parent_pdb,
                snapshot_copy,
                notes: Some("oracle free rust integration test"),
                ..BranchOptions::default()
            },
        )
        .await
    {
        let facts = collect_database_facts(root, config).await?;
        return Err(test_error(format!(
            "create_branch failed against Oracle database:\n{}\nerror: {}",
            format_database_facts(&facts),
            err
        )));
    }

    assert_branch_metadata(root, branch_name, config).await?;

    if snapshot_copy {
        assert_snapshot_fallback_event(root, branch_name).await?;
        mutate_parent_after_branch_create(root, config).await?;
    }

    let branch = connect_workload(config, branch_name).await?;
    ensure(
        scalar_i64(&branch, "SELECT COUNT(*) FROM pdb_branch_seed", &[]).await? == 1,
        "branch should preserve parent seed state at branch creation time",
    )?;
    execute(
        &branch,
        "INSERT INTO experiment_log(event) VALUES (:1)",
        &[Value::from("agent wrote to branch")],
    )
    .await?;
    branch.commit().await?;
    ensure(
        scalar_i64(&branch, "SELECT COUNT(*) FROM experiment_log", &[]).await? == 1,
        "branch should accept writes from the workload user",
    )?;

    client
        .record_score(branch_name, 0.99, Some("rust integration test passed"))
        .await?;
    ensure(
        (scalar_f64(
            root,
            "SELECT score FROM pdb_branch_branches WHERE branch_name = :1",
            &[Value::from(branch_name)],
        )
        .await?
            - 0.99)
            .abs()
            < 0.0001,
        "branch score should be recorded",
    )?;

    Ok(())
}

impl TestConfig {
    fn from_env() -> TestResult<Self> {
        let root_password = env_first(&[
            "PDB_BRANCH_RUST_ROOT_PASSWORD",
            "PDB_BRANCH_SYS_PASSWORD",
            "ORACLE_PWD",
        ])
        .unwrap_or_else(|| "PdbBranch1_".to_owned());

        Ok(Self {
            root_dsn: env_or("PDB_BRANCH_ROOT_DSN", "localhost:1521/FREE"),
            root_user: env_or("PDB_BRANCH_RUST_ROOT_USER", "system"),
            root_password,
            branch_dsn_template: env_or(
                "PDB_BRANCH_BRANCH_DSN_TEMPLATE",
                "localhost:1521/{branch_name}",
            ),
            parent_pdb: simple_name(&env_or("PDB_BRANCH_PARENT_PDB", "FREEPDB1"), "parent PDB")?,
            app_user: simple_name(&env_or("PDB_BRANCH_APP_USER", "PDB_BRANCH_APP"), "app user")?,
            app_password: env_or("PDB_BRANCH_APP_PASSWORD", "PdbBranch1_"),
            service_timeout: Duration::from_secs(
                env_or("PDB_BRANCH_SERVICE_TIMEOUT_SECONDS", "120")
                    .parse()
                    .map_err(|err| {
                        test_error(format!(
                            "PDB_BRANCH_SERVICE_TIMEOUT_SECONDS is invalid: {err}"
                        ))
                    })?,
            ),
        })
    }

    fn pdb_dsn(&self, pdb_name: &str) -> String {
        self.branch_dsn_template.replace("{branch_name}", pdb_name)
    }
}

async fn connect(dsn: &str, user: &str, password: &str) -> TestResult<Connection> {
    Ok(Connection::connect(dsn, user, password).await?)
}

async fn connect_workload(config: &TestConfig, pdb_name: &str) -> TestResult<Connection> {
    let dsn = config.pdb_dsn(pdb_name);
    let deadline = Instant::now() + config.service_timeout;
    let mut last_error = None;

    while Instant::now() < deadline {
        match Connection::connect(&dsn, &config.app_user, &config.app_password).await {
            Ok(connection) => return Ok(connection),
            Err(err) => {
                last_error = Some(err.to_string());
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    Err(test_error(format!(
        "timed out waiting for PDB service {dsn}: {}",
        last_error.unwrap_or_else(|| "no connection attempt was made".to_owned())
    )))
}

async fn require_cdb_root(root: &Connection, config: &TestConfig) -> TestResult {
    let facts = collect_database_facts(root, config).await?;
    let mut problems = Vec::new();

    if facts.cdb != "YES" {
        problems.push("database is not a CDB".to_owned());
    }
    if facts.con_name != "CDB$ROOT" {
        problems.push(format!("connection is in {}, not CDB$ROOT", facts.con_name));
    }
    if facts.parent_open_mode.is_none() {
        problems.push(format!("parent PDB {} was not found", config.parent_pdb));
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(test_error(format!(
            "Oracle integration tests require a CDB root connection: {}\n{}",
            problems.join("; "),
            format_database_facts(&facts)
        )))
    }
}

async fn collect_database_facts(
    root: &Connection,
    config: &TestConfig,
) -> TestResult<DatabaseFacts> {
    let mut db_create_file_dest = None;
    let mut pdb_file_name_convert = None;
    for row in rows(
        root,
        "SELECT name, value FROM v$parameter WHERE name IN ('db_create_file_dest', 'pdb_file_name_convert')",
        &[],
    )
    .await?
    {
        match row.get_string(0) {
            Some("db_create_file_dest") => {
                db_create_file_dest = row.get_string(1).map(str::to_owned);
            }
            Some("pdb_file_name_convert") => {
                pdb_file_name_convert = row.get_string(1).map(str::to_owned);
            }
            _ => {}
        }
    }

    let parent = rows(
        root,
        "SELECT open_mode, restricted FROM v$pdbs WHERE name = :1",
        &[Value::from(config.parent_pdb.as_str())],
    )
    .await?;

    Ok(DatabaseFacts {
        dsn: config.root_dsn.clone(),
        banner: scalar_string(root, "SELECT banner FROM v$version WHERE ROWNUM = 1", &[]).await?,
        cdb: scalar_string(root, "SELECT cdb FROM v$database", &[]).await?,
        con_name: scalar_string(
            root,
            "SELECT SYS_CONTEXT('USERENV', 'CON_NAME') FROM dual",
            &[],
        )
        .await?,
        db_create_file_dest,
        pdb_file_name_convert,
        parent_pdb: config.parent_pdb.clone(),
        parent_open_mode: parent
            .first()
            .and_then(|row| row.get_string(0))
            .map(str::to_owned),
        parent_restricted: parent
            .first()
            .and_then(|row| row.get_string(1))
            .map(str::to_owned),
    })
}

fn format_database_facts(facts: &DatabaseFacts) -> String {
    [
        format!("  dsn: {}", facts.dsn),
        format!("  banner: {}", facts.banner),
        format!("  cdb: {}", facts.cdb),
        format!("  container: {}", facts.con_name),
        format!(
            "  db_create_file_dest: {}",
            facts.db_create_file_dest.as_deref().unwrap_or("(unset)")
        ),
        format!(
            "  pdb_file_name_convert: {}",
            facts.pdb_file_name_convert.as_deref().unwrap_or("(unset)")
        ),
        format!("  parent_pdb: {}", facts.parent_pdb),
        format!(
            "  parent_open_mode: {}",
            facts.parent_open_mode.as_deref().unwrap_or("(missing)")
        ),
        format!(
            "  parent_restricted: {}",
            facts.parent_restricted.as_deref().unwrap_or("(missing)")
        ),
    ]
    .join("\n")
}

async fn prepare_parent_pdb(root: &Connection, config: &TestConfig) -> TestResult {
    reopen_parent_read_write(root, &config.parent_pdb).await?;
    execute(
        root,
        &format!("ALTER SESSION SET CONTAINER = {}", config.parent_pdb),
        &[],
    )
    .await?;

    let setup_result = async {
        execute_ignore(
            root,
            &format!("DROP USER {} CASCADE", config.app_user),
            &[1918],
        )
        .await?;
        execute(
            root,
            &format!(
                "CREATE USER {} IDENTIFIED BY \"{}\"",
                config.app_user,
                escape_quoted(&config.app_password)
            ),
            &[],
        )
        .await?;
        execute(
            root,
            &format!("ALTER USER {} QUOTA UNLIMITED ON USERS", config.app_user),
            &[],
        )
        .await?;
        execute(
            root,
            &format!("GRANT CREATE SESSION, CREATE TABLE TO {}", config.app_user),
            &[],
        )
        .await
    }
    .await;

    let reset_result = execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT", &[]).await;
    setup_result?;
    reset_result?;

    let parent = connect(
        &config.pdb_dsn(&config.parent_pdb),
        &config.app_user,
        &config.app_password,
    )
    .await?;
    execute(
        &parent,
        "CREATE TABLE pdb_branch_seed (id NUMBER PRIMARY KEY, label VARCHAR2(100) NOT NULL)",
        &[],
    )
    .await?;
    execute(
        &parent,
        "CREATE TABLE experiment_log (event VARCHAR2(100) NOT NULL, created_at TIMESTAMP DEFAULT SYSTIMESTAMP NOT NULL)",
        &[],
    )
    .await?;
    execute(
        &parent,
        "INSERT INTO pdb_branch_seed(id, label) VALUES (1, 'seed row')",
        &[],
    )
    .await?;
    parent.commit().await?;

    close_pdb(root, &config.parent_pdb).await?;
    execute(
        root,
        &format!(
            "ALTER PLUGGABLE DATABASE {} OPEN READ ONLY",
            config.parent_pdb
        ),
        &[],
    )
    .await
}

async fn mutate_parent_after_branch_create(root: &Connection, config: &TestConfig) -> TestResult {
    reopen_parent_read_write(root, &config.parent_pdb).await?;
    let parent = connect(
        &config.pdb_dsn(&config.parent_pdb),
        &config.app_user,
        &config.app_password,
    )
    .await?;
    execute(
        &parent,
        "INSERT INTO pdb_branch_seed(id, label) VALUES (2, 'parent mutation')",
        &[],
    )
    .await?;
    parent.commit().await?;
    ensure(
        scalar_i64(&parent, "SELECT COUNT(*) FROM pdb_branch_seed", &[]).await? == 2,
        "parent mutation should be visible in the parent PDB",
    )
}

async fn reopen_parent_read_write(root: &Connection, parent_pdb: &str) -> TestResult {
    execute(root, "ALTER SESSION SET CONTAINER = CDB$ROOT", &[]).await?;
    close_pdb(root, parent_pdb).await?;
    execute(
        root,
        &format!("ALTER PLUGGABLE DATABASE {parent_pdb} OPEN READ WRITE"),
        &[],
    )
    .await
}

async fn close_pdb(root: &Connection, pdb_name: &str) -> TestResult {
    execute_ignore(
        root,
        &format!("ALTER PLUGGABLE DATABASE {pdb_name} CLOSE IMMEDIATE"),
        &[65020],
    )
    .await
}

async fn assert_branch_metadata(
    root: &Connection,
    branch_name: &str,
    config: &TestConfig,
) -> TestResult {
    let branch = required_row(
        root,
        "SELECT branch_name, parent_pdb, status FROM pdb_branch_branches WHERE branch_name = :1",
        &[Value::from(branch_name)],
    )
    .await?;

    ensure(
        branch.get_string(0) == Some(branch_name),
        "control table should record branch name",
    )?;
    ensure(
        branch.get_string(1) == Some(config.parent_pdb.as_str()),
        "control table should record parent PDB",
    )?;
    ensure(
        branch.get_string(2) == Some("OPEN"),
        "created branch should be open",
    )
}

async fn assert_snapshot_fallback_event(root: &Connection, branch_name: &str) -> TestResult {
    let details = scalar_string(
        root,
        "
        SELECT details
          FROM (
                SELECT details
                  FROM pdb_branch_events
                 WHERE branch_name = :1
                   AND event_type = 'SNAPSHOT_COPY_FALLBACK'
                 ORDER BY event_id DESC
               )
         WHERE ROWNUM = 1
        ",
        &[Value::from(branch_name)],
    )
    .await?;
    ensure(
        details.contains("created with full clone"),
        "snapshot fallback event should explain that a full clone was created",
    )
}

async fn execute(connection: &Connection, sql: &str, parameters: &[Value]) -> TestResult {
    connection.execute(sql, parameters).await?;
    Ok(())
}

async fn execute_ignore(connection: &Connection, sql: &str, ignored_codes: &[u32]) -> TestResult {
    match connection.execute(sql, &[]).await {
        Ok(_) => Ok(()),
        Err(err) if error_code(&err).map_or(false, |code| ignored_codes.contains(&code)) => Ok(()),
        Err(err) => Err(Box::new(err)),
    }
}

async fn scalar_string(
    connection: &Connection,
    sql: &str,
    parameters: &[Value],
) -> TestResult<String> {
    let row = required_row(connection, sql, parameters).await?;
    row.get_string(0)
        .map(str::to_owned)
        .ok_or_else(|| test_error(format!("query returned NULL or non-string value: {sql}")))
}

async fn scalar_i64(connection: &Connection, sql: &str, parameters: &[Value]) -> TestResult<i64> {
    let row = required_row(connection, sql, parameters).await?;
    row.get_i64(0)
        .ok_or_else(|| test_error(format!("query returned NULL or non-integer value: {sql}")))
}

async fn scalar_f64(connection: &Connection, sql: &str, parameters: &[Value]) -> TestResult<f64> {
    let row = required_row(connection, sql, parameters).await?;
    row.get_f64(0)
        .ok_or_else(|| test_error(format!("query returned NULL or non-number value: {sql}")))
}

async fn required_row(connection: &Connection, sql: &str, parameters: &[Value]) -> TestResult<Row> {
    rows(connection, sql, parameters)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| test_error(format!("query returned no rows: {sql}")))
}

async fn rows(connection: &Connection, sql: &str, parameters: &[Value]) -> TestResult<Vec<Row>> {
    Ok(connection.query(sql, parameters).await?.rows)
}

fn error_code(err: &oracle_rs::Error) -> Option<u32> {
    match err {
        oracle_rs::Error::OracleError { code, .. } => Some(*code),
        oracle_rs::Error::ServerError { code, .. } => Some(*code),
        _ => None,
    }
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn env_first(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env::var(name).ok())
}

fn make_branch_name(prefix: &str) -> TestResult<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| test_error(format!("system clock is before UNIX_EPOCH: {err}")))?
        .as_nanos();
    let unique = format!("{:X}{:X}", process::id(), now);
    let suffix = &unique[unique.len().saturating_sub(8)..];
    simple_name(&format!("{prefix}{suffix}"), "branch name")
}

fn simple_name(value: &str, label: &str) -> TestResult<String> {
    let name = value.trim().to_ascii_uppercase();
    let mut chars = name.chars();
    let first = chars.next();

    if name.len() > 30
        || !matches!(first, Some('A'..='Z'))
        || !chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || "_$#".contains(ch))
    {
        return Err(test_error(format!(
            "{label} must be an unquoted Oracle identifier of 30 chars or fewer"
        )));
    }

    Ok(name)
}

fn escape_quoted(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn ensure(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(test_error(message))
    }
}

fn test_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(IoError::new(ErrorKind::Other, message.into()))
}
