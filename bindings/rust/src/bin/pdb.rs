use clap::{Args, Parser, Subcommand};
use futures::executor::block_on;
use oracle::{Connection, Connector, Privilege};
use pdb_branch::{
    BranchClient, BranchInfo, BranchOptions, CleanupOptions, ResourcePlanOptions,
    RustOracleExecutor,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

type CliResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug, Parser)]
#[command(name = "pdb", version, about = "Manage Oracle PDB branches")]
struct Cli {
    #[arg(short = 'C', value_name = "PATH", global = true)]
    chdir: Option<PathBuf>,

    #[arg(short = 'c', value_name = "KEY=VALUE", global = true)]
    config: Vec<String>,

    #[arg(long, value_name = "FILE", global = true)]
    profile: Option<PathBuf>,

    #[arg(long, value_name = "DSN", global = true)]
    dsn: Option<String>,

    #[arg(long, value_name = "USER", global = true)]
    user: Option<String>,

    #[arg(long, value_name = "PASSWORD", global = true)]
    password: Option<String>,

    #[arg(long, global = true)]
    sysdba: bool,

    #[arg(long = "no-sysdba", global = true)]
    no_sysdba: bool,

    #[arg(long, global = true)]
    install: bool,

    #[arg(long = "no-install", global = true)]
    no_install: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init(InitArgs),
    Install,
    Branch(BranchArgs),
    Open(OpenArgs),
    Close(CloseArgs),
    Score(ScoreArgs),
    Promote(NotesArgs),
    Cleanup(CleanupArgs),
    ResourcePlan(ResourcePlanArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(short, long)]
    force: bool,

    #[arg(long = "from", value_name = "PDB")]
    from_pdb: Option<String>,

    #[arg(long)]
    snapshot: bool,

    #[arg(long = "no-snapshot")]
    no_snapshot: bool,

    #[arg(long)]
    open: bool,

    #[arg(long = "no-open")]
    no_open: bool,

    #[arg(long = "profile-name", value_name = "PROFILE")]
    profile_name: Option<String>,
}

#[derive(Debug, Args)]
struct BranchArgs {
    #[arg(value_name = "BRANCH")]
    branch: Option<String>,

    #[arg(short = 'd', long = "delete", value_name = "BRANCH", num_args = 1..)]
    delete: Vec<String>,

    #[arg(short = 'D', long = "delete-force", value_name = "BRANCH", num_args = 1..)]
    delete_force: Vec<String>,

    #[arg(short = 'a', long = "all")]
    all: bool,

    #[arg(long)]
    list: bool,

    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    #[arg(long = "from", value_name = "PDB")]
    from_pdb: Option<String>,

    #[arg(long)]
    snapshot: bool,

    #[arg(long = "no-snapshot")]
    no_snapshot: bool,

    #[arg(long)]
    open: bool,

    #[arg(long = "no-open")]
    no_open: bool,

    #[arg(long = "profile-name", value_name = "PROFILE")]
    profile_name: Option<String>,

    #[arg(long, value_name = "TIMESTAMP")]
    expires_at: Option<String>,

    #[arg(long, value_name = "TEXT")]
    notes: Option<String>,

    #[arg(long = "keep-datafiles")]
    keep_datafiles: bool,
}

#[derive(Debug, Args)]
struct OpenArgs {
    branch: String,

    #[arg(long = "profile-name", value_name = "PROFILE")]
    profile_name: Option<String>,
}

#[derive(Debug, Args)]
struct CloseArgs {
    branch: String,

    #[arg(long = "no-immediate")]
    no_immediate: bool,
}

#[derive(Debug, Args)]
struct ScoreArgs {
    branch: String,
    score: f64,

    #[arg(long, value_name = "TEXT")]
    notes: Option<String>,
}

#[derive(Debug, Args)]
struct NotesArgs {
    branch: String,

    #[arg(long, value_name = "TEXT")]
    notes: Option<String>,
}

#[derive(Debug, Args)]
struct CleanupArgs {
    #[arg(long, value_name = "MINUTES")]
    close_idle_after_minutes: Option<i64>,

    #[arg(long = "drop-expired")]
    drop_expired: bool,

    #[arg(long = "no-drop-expired")]
    no_drop_expired: bool,
}

#[derive(Debug, Args)]
struct ResourcePlanArgs {
    #[arg(long = "plan-name", value_name = "PLAN")]
    plan_name: Option<String>,

    #[arg(long)]
    activate: bool,
}

#[derive(Clone, Debug)]
struct ResolvedConfig {
    database: DatabaseResolved,
    branch: BranchResolved,
}

#[derive(Clone, Debug)]
struct DatabaseResolved {
    dsn: String,
    user: String,
    password: String,
    sysdba: bool,
    install: bool,
}

#[derive(Clone, Debug)]
struct BranchResolved {
    from_pdb: String,
    snapshot_copy: bool,
    open: bool,
    profile_name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct ProfileFile {
    database: DatabaseProfile,
    branch: BranchProfile,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct DatabaseProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    dsn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sysdba: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct BranchProfile {
    #[serde(rename = "from", skip_serializing_if = "Option::is_none")]
    from_pdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_copy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_name: Option<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("pdb: {err}");
        process::exit(1);
    }
}

fn run() -> CliResult {
    let cli = Cli::parse();
    if let Some(path) = &cli.chdir {
        env::set_current_dir(path)?;
    }

    let profile_path = cli
        .profile
        .clone()
        .unwrap_or_else(|| PathBuf::from(".pdbprofile"));
    let mut profile = read_profile(&profile_path)?;
    for value in &cli.config {
        apply_config_override(&mut profile, value)?;
    }
    let config = resolve_config(&cli, &profile)?;

    match &cli.command {
        Command::Init(args) => init_profile(&profile_path, &config, args),
        Command::Install => {
            let client = connect(&config.database)?;
            block_on(client.ensure_installed())?;
            println!("Installed pdb_branch package");
            Ok(())
        }
        Command::Branch(args) => {
            let client = ready_client(&config)?;
            run_branch(&client, &config.branch, args)
        }
        Command::Open(args) => {
            let client = ready_client(&config)?;
            block_on(client.open_branch(&args.branch, args.profile_name.as_deref()))?;
            println!("Opened branch {}", args.branch);
            Ok(())
        }
        Command::Close(args) => {
            let client = ready_client(&config)?;
            block_on(client.close_branch(&args.branch, !args.no_immediate))?;
            println!("Closed branch {}", args.branch);
            Ok(())
        }
        Command::Score(args) => {
            let client = ready_client(&config)?;
            block_on(client.record_score(&args.branch, args.score, args.notes.as_deref()))?;
            println!("Recorded score {} for {}", args.score, args.branch);
            Ok(())
        }
        Command::Promote(args) => {
            let client = ready_client(&config)?;
            block_on(client.promote(&args.branch, args.notes.as_deref()))?;
            println!("Promoted branch {}", args.branch);
            Ok(())
        }
        Command::Cleanup(args) => {
            let client = ready_client(&config)?;
            let defaults = CleanupOptions::default();
            let drop_expired = choose_bool(
                args.drop_expired,
                args.no_drop_expired,
                defaults.drop_expired,
                "drop-expired",
            )?;
            block_on(
                client.cleanup(CleanupOptions {
                    close_idle_after_minutes: args
                        .close_idle_after_minutes
                        .unwrap_or(defaults.close_idle_after_minutes),
                    drop_expired,
                }),
            )?;
            println!("Cleaned up branches");
            Ok(())
        }
        Command::ResourcePlan(args) => {
            let client = ready_client(&config)?;
            let plan_name = args.plan_name.as_deref().unwrap_or("PDB_BRANCH_PLAN");
            block_on(client.configure_resource_plan(ResourcePlanOptions {
                plan_name,
                activate: args.activate,
            }))?;
            println!("Configured resource plan {plan_name}");
            Ok(())
        }
    }
}

fn run_branch(
    client: &BranchClient<RustOracleExecutor>,
    defaults: &BranchResolved,
    args: &BranchArgs,
) -> CliResult {
    let mut deletes = args.delete.clone();
    deletes.extend(args.delete_force.iter().cloned());

    if !deletes.is_empty() {
        if args.branch.is_some() {
            return Err("branch name cannot be combined with --delete".into());
        }
        if args.list {
            return Err("--list cannot be combined with --delete".into());
        }
        for branch in deletes {
            block_on(client.drop_branch(&branch, !args.keep_datafiles))?;
            println!("Deleted branch {branch}");
        }
        return Ok(());
    }

    if let Some(branch) = &args.branch {
        if args.list {
            return Err("--list does not take branch names yet".into());
        }

        let from_pdb = args.from_pdb.as_deref().unwrap_or(&defaults.from_pdb);
        let snapshot_copy = choose_bool(
            args.snapshot,
            args.no_snapshot,
            defaults.snapshot_copy,
            "snapshot",
        )?;
        let open = choose_bool(args.open, args.no_open, defaults.open, "open")?;
        let profile_name = args
            .profile_name
            .as_deref()
            .or(defaults.profile_name.as_deref());

        let result = block_on(client.create_branch_with_result(
            branch,
            BranchOptions {
                from_pdb,
                snapshot_copy,
                open_branch: open,
                profile_name,
                expires_at: args.expires_at.as_deref(),
                notes: args.notes.as_deref(),
            },
        ))?;
        if let Some(warning) = result.fallback_warning {
            eprintln!("{warning}");
        }
        println!("Created branch {branch}");
        return Ok(());
    }

    let branches = block_on(client.list_branches(args.all))?;
    print_branches(&branches, args.verbose);
    Ok(())
}

fn ready_client(config: &ResolvedConfig) -> CliResult<BranchClient<RustOracleExecutor>> {
    let client = connect(&config.database)?;
    if config.database.install {
        block_on(client.ensure_installed())?;
    }
    Ok(client)
}

fn connect(config: &DatabaseResolved) -> CliResult<BranchClient<RustOracleExecutor>> {
    let connection = if config.sysdba {
        Connector::new(&config.user, &config.password, &config.dsn)
            .privilege(Privilege::Sysdba)
            .connect()?
    } else {
        Connection::connect(&config.user, &config.password, &config.dsn)?
    };
    Ok(BranchClient::new(RustOracleExecutor::new(connection)))
}

fn print_branches(branches: &[BranchInfo], verbose: bool) {
    if verbose {
        println!(
            "{:<32} {:<10} {:<20} {:<20} {:<8} CREATED",
            "NAME", "STATUS", "PARENT", "PROFILE", "SCORE"
        );
        for branch in branches {
            println!(
                "{:<32} {:<10} {:<20} {:<20} {:<8} {}",
                branch.branch_name,
                branch.status,
                branch.parent_pdb.as_deref().unwrap_or("-"),
                branch.profile_name.as_deref().unwrap_or("-"),
                branch
                    .score
                    .map(|score| score.to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                branch.created_at.as_deref().unwrap_or("-"),
            );
        }
    } else {
        for branch in branches {
            println!(
                "{:<32} {:<10} {}",
                branch.branch_name,
                branch.status,
                branch.parent_pdb.as_deref().unwrap_or("-")
            );
        }
    }
}

fn init_profile(path: &Path, config: &ResolvedConfig, args: &InitArgs) -> CliResult {
    if path.exists() && !args.force {
        return Err(format!(
            "{} already exists; use --force to replace it",
            path.display()
        )
        .into());
    }

    let snapshot_copy = choose_bool(
        args.snapshot,
        args.no_snapshot,
        config.branch.snapshot_copy,
        "snapshot",
    )?;
    let open = choose_bool(args.open, args.no_open, config.branch.open, "open")?;
    let profile = ProfileFile {
        database: DatabaseProfile {
            dsn: Some(config.database.dsn.clone()),
            user: Some(config.database.user.clone()),
            password: Some(config.database.password.clone()),
            sysdba: Some(config.database.sysdba),
            install: Some(config.database.install),
        },
        branch: BranchProfile {
            from_pdb: Some(
                args.from_pdb
                    .clone()
                    .unwrap_or_else(|| config.branch.from_pdb.clone()),
            ),
            snapshot: None,
            snapshot_copy: Some(snapshot_copy),
            open: Some(open),
            profile_name: args
                .profile_name
                .clone()
                .or_else(|| config.branch.profile_name.clone()),
        },
    };

    fs::write(path, toml::to_string_pretty(&profile)?)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn read_profile(path: &Path) -> CliResult<ProfileFile> {
    if !path.exists() {
        return Ok(ProfileFile::default());
    }
    let contents = fs::read_to_string(path)?;
    Ok(toml::from_str(&contents)?)
}

fn resolve_config(cli: &Cli, profile: &ProfileFile) -> CliResult<ResolvedConfig> {
    let dsn = cli
        .dsn
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_DSN", "PDB_BRANCH_ROOT_DSN"]))
        .or_else(|| profile.database.dsn.clone())
        .unwrap_or_else(|| "localhost:1521/FREE".to_owned());
    let user = cli
        .user
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_SYS_USER"]))
        .or_else(|| profile.database.user.clone())
        .unwrap_or_else(|| "sys".to_owned());
    let password = cli
        .password
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_SYS_PASSWORD", "ORACLE_PWD"]))
        .or_else(|| profile.database.password.clone())
        .unwrap_or_else(|| "PdbBranch1_".to_owned());
    let sysdba_default = env_bool("PDB_BRANCH_SYSDBA")
        .or(profile.database.sysdba)
        .unwrap_or(true);
    let install_default = env_bool("PDB_BRANCH_INSTALL")
        .or(profile.database.install)
        .unwrap_or(true);

    Ok(ResolvedConfig {
        database: DatabaseResolved {
            dsn,
            user,
            password,
            sysdba: choose_bool(cli.sysdba, cli.no_sysdba, sysdba_default, "sysdba")?,
            install: choose_bool(cli.install, cli.no_install, install_default, "install")?,
        },
        branch: BranchResolved {
            from_pdb: env_first(&["PDB_BRANCH_PARENT_PDB"])
                .or_else(|| profile.branch.from_pdb.clone())
                .unwrap_or_else(|| "GOLDEN_MASTER".to_owned()),
            snapshot_copy: env_bool("PDB_BRANCH_SNAPSHOT_COPY")
                .or(profile.branch.snapshot_copy)
                .or(profile.branch.snapshot)
                .unwrap_or(true),
            open: env_bool("PDB_BRANCH_OPEN")
                .or(profile.branch.open)
                .unwrap_or(true),
            profile_name: env_first(&["PDB_BRANCH_PROFILE_NAME"])
                .or_else(|| profile.branch.profile_name.clone()),
        },
    })
}

fn apply_config_override(profile: &mut ProfileFile, value: &str) -> CliResult {
    let Some((key, raw_value)) = value.split_once('=') else {
        return Err(format!("config override must be KEY=VALUE, got {value:?}").into());
    };

    match key {
        "database.dsn" => profile.database.dsn = Some(raw_value.to_owned()),
        "database.user" => profile.database.user = Some(raw_value.to_owned()),
        "database.password" => profile.database.password = Some(raw_value.to_owned()),
        "database.sysdba" => profile.database.sysdba = Some(parse_bool(raw_value)?),
        "database.install" => profile.database.install = Some(parse_bool(raw_value)?),
        "branch.from" => profile.branch.from_pdb = Some(raw_value.to_owned()),
        "branch.snapshot" => profile.branch.snapshot = Some(parse_bool(raw_value)?),
        "branch.snapshot_copy" => profile.branch.snapshot_copy = Some(parse_bool(raw_value)?),
        "branch.open" => profile.branch.open = Some(parse_bool(raw_value)?),
        "branch.profile_name" => profile.branch.profile_name = Some(raw_value.to_owned()),
        _ => return Err(format!("unknown config key {key:?}").into()),
    }
    Ok(())
}

fn choose_bool(yes: bool, no: bool, default: bool, name: &str) -> CliResult<bool> {
    match (yes, no) {
        (true, true) => Err(format!("--{name} and --no-{name} cannot both be set").into()),
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        (false, false) => Ok(default),
    }
}

fn env_first(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env::var(name).ok())
}

fn env_bool(name: &str) -> Option<bool> {
    env::var(name)
        .ok()
        .and_then(|value| parse_bool(&value).ok())
}

fn parse_bool(value: &str) -> CliResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" => Ok(true),
        "0" | "false" | "no" | "n" | "off" => Ok(false),
        _ => Err(format!("invalid boolean value {value:?}").into()),
    }
}
