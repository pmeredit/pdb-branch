use clap::{Args, Parser, Subcommand};
use futures::executor::block_on;
use oracle::{Connection, Connector, Privilege};
use pdb_branch::{
    BranchClient, BranchInfo, BranchOptions, CleanupOptions, RemoteCopyOptions,
    ResourcePlanOptions, RustOracleExecutor,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

    #[arg(long, value_name = "REMOTE", global = true)]
    remote: Option<String>,

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
    Remote(RemoteArgs),
    Branch(BranchArgs),
    Push(PushArgs),
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

    #[arg(long, value_name = "REMOTE")]
    remote: Option<String>,

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
struct PushArgs {
    #[arg(value_name = "REMOTE")]
    target_remote: String,

    #[arg(value_name = "SOURCE[:TARGET]")]
    branch: String,

    #[arg(long = "source", value_name = "REMOTE")]
    source_remote: Option<String>,

    #[arg(long = "db-link", value_name = "DB_LINK")]
    source_db_link: Option<String>,

    #[arg(long = "create-file-dest", value_name = "PATH")]
    create_file_dest: Option<String>,

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
}

#[derive(Debug, Args)]
struct RemoteArgs {
    #[command(subcommand)]
    command: Option<RemoteCommand>,
}

#[derive(Debug, Subcommand)]
enum RemoteCommand {
    Add(RemoteAddArgs),
    Remove(RemoteNameArgs),
    Show(RemoteNameArgs),
    Default(RemoteNameArgs),
}

#[derive(Debug, Args)]
struct RemoteAddArgs {
    name: String,

    #[arg(long, value_name = "DSN")]
    dsn: String,

    #[arg(long, value_name = "USER")]
    user: Option<String>,

    #[arg(long, value_name = "PASSWORD")]
    password: Option<String>,

    #[arg(long)]
    sysdba: bool,

    #[arg(long = "no-sysdba")]
    no_sysdba: bool,

    #[arg(long)]
    install: bool,

    #[arg(long = "no-install")]
    no_install: bool,

    #[arg(long = "source-db-link", value_name = "DB_LINK")]
    source_db_link: Option<String>,

    #[arg(long = "create-file-dest", value_name = "PATH")]
    create_file_dest: Option<String>,

    #[arg(long = "default")]
    make_default: bool,
}

#[derive(Debug, Args)]
struct RemoteNameArgs {
    name: String,
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
    remote_name: String,
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
    source_db_link: Option<String>,
    create_file_dest: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    default_remote: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    remotes: BTreeMap<String, RemoteProfile>,
    branch: BranchProfile,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct RemoteProfile {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    source_db_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    create_file_dest: Option<String>,
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

    match &cli.command {
        Command::Init(args) => {
            let config = resolve_config(&cli, &profile, args.remote.as_deref())?;
            init_profile(&profile_path, &config, args)
        }
        Command::Install => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = connect(&config.database)?;
            block_on(client.ensure_installed())?;
            println!("Installed pdb_branch package on {}", config.remote_name);
            Ok(())
        }
        Command::Remote(args) => run_remote(&profile_path, &mut profile, args),
        Command::Push(args) => run_push(&cli, &profile, args),
        Command::Branch(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            run_branch(&client, &config.branch, args)
        }
        Command::Open(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            block_on(client.open_branch(&args.branch, args.profile_name.as_deref()))?;
            println!("Opened branch {} on {}", args.branch, config.remote_name);
            Ok(())
        }
        Command::Close(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            block_on(client.close_branch(&args.branch, !args.no_immediate))?;
            println!("Closed branch {} on {}", args.branch, config.remote_name);
            Ok(())
        }
        Command::Score(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            block_on(client.record_score(&args.branch, args.score, args.notes.as_deref()))?;
            println!(
                "Recorded score {} for {} on {}",
                args.score, args.branch, config.remote_name
            );
            Ok(())
        }
        Command::Promote(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            block_on(client.promote(&args.branch, args.notes.as_deref()))?;
            println!("Promoted branch {} on {}", args.branch, config.remote_name);
            Ok(())
        }
        Command::Cleanup(args) => {
            let config = resolve_config(&cli, &profile, None)?;
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
            println!("Cleaned up branches on {}", config.remote_name);
            Ok(())
        }
        Command::ResourcePlan(args) => {
            let config = resolve_config(&cli, &profile, None)?;
            let client = ready_client(&config)?;
            let plan_name = args.plan_name.as_deref().unwrap_or("PDB_BRANCH_PLAN");
            block_on(client.configure_resource_plan(ResourcePlanOptions {
                plan_name,
                activate: args.activate,
            }))?;
            println!(
                "Configured resource plan {plan_name} on {}",
                config.remote_name
            );
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

fn run_push(cli: &Cli, profile: &ProfileFile, args: &PushArgs) -> CliResult {
    let source_remote = resolve_remote_name(cli, profile, args.source_remote.as_deref())?;
    let target_config = resolve_config(cli, profile, Some(&args.target_remote))?;

    if source_remote == target_config.remote_name {
        return Err("push target remote must differ from the source remote".into());
    }

    let (source_pdb, target_branch) = parse_push_branch(&args.branch)?;
    let source_db_link = args
        .source_db_link
        .as_deref()
        .or(target_config.database.source_db_link.as_deref())
        .ok_or_else(|| {
            format!(
                "push to remote {:?} requires --db-link or remotes.{}.source_db_link",
                target_config.remote_name, target_config.remote_name
            )
        })?;
    let open = choose_bool(args.open, args.no_open, target_config.branch.open, "open")?;
    let profile_name = args
        .profile_name
        .as_deref()
        .or(target_config.branch.profile_name.as_deref());
    let create_file_dest = args
        .create_file_dest
        .as_deref()
        .or(target_config.database.create_file_dest.as_deref());

    let client = ready_client(&target_config)?;
    block_on(client.copy_branch_from_remote(
        target_branch,
        RemoteCopyOptions {
            source_pdb,
            source_db_link,
            open_branch: open,
            profile_name,
            expires_at: args.expires_at.as_deref(),
            notes: args.notes.as_deref(),
            create_file_dest,
        },
    ))?;

    println!(
        "Pushed {source_remote}/{source_pdb} to {}/{}",
        target_config.remote_name, target_branch
    );
    Ok(())
}

fn parse_push_branch(value: &str) -> CliResult<(&str, &str)> {
    match value.split_once(':') {
        Some((source, target)) if !source.is_empty() && !target.is_empty() => Ok((source, target)),
        Some(_) => Err("push ref must be SOURCE or SOURCE:TARGET".into()),
        None if !value.is_empty() => Ok((value, value)),
        None => Err("push branch is required".into()),
    }
}

fn run_remote(path: &Path, profile: &mut ProfileFile, args: &RemoteArgs) -> CliResult {
    match &args.command {
        None => {
            print_remotes(profile);
            Ok(())
        }
        Some(RemoteCommand::Add(args)) => add_remote(path, profile, args),
        Some(RemoteCommand::Remove(args)) => remove_remote(path, profile, &args.name),
        Some(RemoteCommand::Show(args)) => show_remote(profile, &args.name),
        Some(RemoteCommand::Default(args)) => set_default_remote(path, profile, &args.name),
    }
}

fn add_remote(path: &Path, profile: &mut ProfileFile, args: &RemoteAddArgs) -> CliResult {
    if profile.remotes.contains_key(&args.name) {
        return Err(format!("remote {:?} already exists", args.name).into());
    }

    let sysdba = choose_bool(args.sysdba, args.no_sysdba, true, "sysdba")?;
    let install = choose_bool(args.install, args.no_install, true, "install")?;
    profile.remotes.insert(
        args.name.clone(),
        RemoteProfile {
            dsn: Some(args.dsn.clone()),
            user: Some(args.user.clone().unwrap_or_else(|| "sys".to_owned())),
            password: Some(
                args.password
                    .clone()
                    .or_else(|| env_first(&["PDB_BRANCH_SYS_PASSWORD", "ORACLE_PWD"]))
                    .unwrap_or_else(|| "PdbBranch1_".to_owned()),
            ),
            sysdba: Some(sysdba),
            install: Some(install),
            source_db_link: args.source_db_link.clone(),
            create_file_dest: args.create_file_dest.clone(),
        },
    );

    if args.make_default || profile.default_remote.is_none() {
        profile.default_remote = Some(args.name.clone());
    }

    write_profile(path, profile)?;
    println!("Added remote {}", args.name);
    Ok(())
}

fn remove_remote(path: &Path, profile: &mut ProfileFile, name: &str) -> CliResult {
    if profile.remotes.remove(name).is_none() {
        return Err(format!("remote {name:?} is not configured").into());
    }

    if profile.default_remote.as_deref() == Some(name) {
        profile.default_remote = profile.remotes.keys().next().cloned();
    }

    write_profile(path, profile)?;
    println!("Removed remote {name}");
    Ok(())
}

fn set_default_remote(path: &Path, profile: &mut ProfileFile, name: &str) -> CliResult {
    if !profile.remotes.contains_key(name) {
        return Err(format!("remote {name:?} is not configured").into());
    }

    profile.default_remote = Some(name.to_owned());
    write_profile(path, profile)?;
    println!("Default remote is now {name}");
    Ok(())
}

fn show_remote(profile: &ProfileFile, name: &str) -> CliResult {
    let remote = profile
        .remotes
        .get(name)
        .ok_or_else(|| format!("remote {name:?} is not configured"))?;
    println!("name = {name}");
    println!("dsn = {}", remote.dsn.as_deref().unwrap_or("-"));
    println!("user = {}", remote.user.as_deref().unwrap_or("-"));
    println!(
        "password = {}",
        if remote.password.is_some() {
            "(set)"
        } else {
            "-"
        }
    );
    println!(
        "sysdba = {}",
        remote
            .sysdba
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "install = {}",
        remote
            .install
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "source_db_link = {}",
        remote.source_db_link.as_deref().unwrap_or("-")
    );
    println!(
        "create_file_dest = {}",
        remote.create_file_dest.as_deref().unwrap_or("-")
    );
    Ok(())
}

fn print_remotes(profile: &ProfileFile) {
    if profile.remotes.is_empty() {
        println!("No remotes configured");
        return;
    }

    println!("{:<16} {:<7} {:<16} DSN", "NAME", "DEFAULT", "USER");
    for (name, remote) in &profile.remotes {
        let default_marker = if profile.default_remote.as_deref() == Some(name) {
            "*"
        } else {
            ""
        };
        println!(
            "{:<16} {:<7} {:<16} {}",
            name,
            default_marker,
            remote.user.as_deref().unwrap_or("-"),
            remote.dsn.as_deref().unwrap_or("-")
        );
    }
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
    let mut remotes = BTreeMap::new();
    remotes.insert(
        config.remote_name.clone(),
        RemoteProfile {
            dsn: Some(config.database.dsn.clone()),
            user: Some(config.database.user.clone()),
            password: Some(config.database.password.clone()),
            sysdba: Some(config.database.sysdba),
            install: Some(config.database.install),
            source_db_link: config.database.source_db_link.clone(),
            create_file_dest: config.database.create_file_dest.clone(),
        },
    );
    let profile = ProfileFile {
        default_remote: Some(config.remote_name.clone()),
        remotes,
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

    write_profile(path, &profile)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn write_profile(path: &Path, profile: &ProfileFile) -> CliResult {
    fs::write(path, toml::to_string_pretty(profile)?)?;
    Ok(())
}

fn read_profile(path: &Path) -> CliResult<ProfileFile> {
    if !path.exists() {
        return Ok(ProfileFile::default());
    }
    let contents = fs::read_to_string(path)?;
    Ok(toml::from_str(&contents)?)
}

fn resolve_config(
    cli: &Cli,
    profile: &ProfileFile,
    remote_override: Option<&str>,
) -> CliResult<ResolvedConfig> {
    let remote_name = resolve_remote_name(cli, profile, remote_override)?;
    let remote = profile
        .remotes
        .get(&remote_name)
        .cloned()
        .unwrap_or_default();
    let direct_dsn = cli
        .dsn
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_DSN", "PDB_BRANCH_ROOT_DSN"]));
    if !profile.remotes.contains_key(&remote_name) && direct_dsn.is_none() {
        return Err(format!(
            "remote {remote_name:?} is not configured; run `pdb init --remote {remote_name} ...` or `pdb remote add {remote_name} ...`"
        )
        .into());
    }

    let dsn = direct_dsn
        .or(remote.dsn)
        .ok_or_else(|| format!("remote {remote_name:?} does not have a DSN"))?;
    let user = cli
        .user
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_SYS_USER"]))
        .or(remote.user)
        .unwrap_or_else(|| "sys".to_owned());
    let password = cli
        .password
        .clone()
        .or_else(|| env_first(&["PDB_BRANCH_SYS_PASSWORD", "ORACLE_PWD"]))
        .or(remote.password)
        .unwrap_or_else(|| "PdbBranch1_".to_owned());
    let sysdba_default = env_bool("PDB_BRANCH_SYSDBA")
        .or(remote.sysdba)
        .unwrap_or(true);
    let install_default = env_bool("PDB_BRANCH_INSTALL")
        .or(remote.install)
        .unwrap_or(true);
    let source_db_link = env_first(&["PDB_BRANCH_SOURCE_DB_LINK"]).or(remote.source_db_link);
    let create_file_dest = env_first(&["PDB_BRANCH_CREATE_FILE_DEST"]).or(remote.create_file_dest);

    Ok(ResolvedConfig {
        remote_name,
        database: DatabaseResolved {
            dsn,
            user,
            password,
            sysdba: choose_bool(cli.sysdba, cli.no_sysdba, sysdba_default, "sysdba")?,
            install: choose_bool(cli.install, cli.no_install, install_default, "install")?,
            source_db_link,
            create_file_dest,
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

fn resolve_remote_name(
    cli: &Cli,
    profile: &ProfileFile,
    remote_override: Option<&str>,
) -> CliResult<String> {
    if let Some(remote) = remote_override {
        return Ok(remote.to_owned());
    }
    if let Some(remote) = &cli.remote {
        return Ok(remote.clone());
    }
    if let Some(remote) = env_first(&["PDB_BRANCH_REMOTE"]) {
        return Ok(remote);
    }
    if let Some(remote) = &profile.default_remote {
        return Ok(remote.clone());
    }
    if profile.remotes.len() == 1 {
        return Ok(profile.remotes.keys().next().cloned().unwrap());
    }
    if cli.dsn.is_some() || env_first(&["PDB_BRANCH_DSN", "PDB_BRANCH_ROOT_DSN"]).is_some() {
        return Ok("origin".to_owned());
    }

    Err("no remote selected; set default_remote, pass --remote, or run `pdb init`".into())
}

fn apply_config_override(profile: &mut ProfileFile, value: &str) -> CliResult {
    let Some((key, raw_value)) = value.split_once('=') else {
        return Err(format!("config override must be KEY=VALUE, got {value:?}").into());
    };

    if key == "default_remote" || key == "remote" {
        profile.default_remote = Some(raw_value.to_owned());
        return Ok(());
    }

    if let Some(rest) = key.strip_prefix("remotes.") {
        let Some((remote_name, field)) = rest.rsplit_once('.') else {
            return Err(
                format!("remote config override must be remotes.NAME.FIELD, got {key:?}").into(),
            );
        };
        let remote = profile.remotes.entry(remote_name.to_owned()).or_default();
        match field {
            "dsn" => remote.dsn = Some(raw_value.to_owned()),
            "user" => remote.user = Some(raw_value.to_owned()),
            "password" => remote.password = Some(raw_value.to_owned()),
            "sysdba" => remote.sysdba = Some(parse_bool(raw_value)?),
            "install" => remote.install = Some(parse_bool(raw_value)?),
            "source_db_link" => remote.source_db_link = Some(raw_value.to_owned()),
            "create_file_dest" => remote.create_file_dest = Some(raw_value.to_owned()),
            _ => return Err(format!("unknown remote config key {field:?}").into()),
        }
        return Ok(());
    }

    match key {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_push_refspecs() {
        assert_eq!(
            parse_push_branch("EXPERIMENT_042").unwrap(),
            ("EXPERIMENT_042", "EXPERIMENT_042")
        );
        assert_eq!(
            parse_push_branch("EXPERIMENT_042:QA_COPY").unwrap(),
            ("EXPERIMENT_042", "QA_COPY")
        );
    }

    #[test]
    fn rejects_empty_push_refspec_parts() {
        assert!(parse_push_branch(":QA_COPY").is_err());
        assert!(parse_push_branch("EXPERIMENT_042:").is_err());
    }

    #[test]
    fn config_override_updates_named_remote() {
        let mut profile = ProfileFile::default();

        apply_config_override(&mut profile, "default_remote=qa").unwrap();
        apply_config_override(&mut profile, "remotes.qa.dsn=localhost:1521/QA").unwrap();
        apply_config_override(&mut profile, "remotes.qa.user=sys").unwrap();
        apply_config_override(&mut profile, "remotes.qa.sysdba=true").unwrap();
        apply_config_override(&mut profile, "remotes.qa.source_db_link=PDB_BRANCH_ORIGIN").unwrap();

        let remote = profile.remotes.get("qa").unwrap();
        assert_eq!(profile.default_remote.as_deref(), Some("qa"));
        assert_eq!(remote.dsn.as_deref(), Some("localhost:1521/QA"));
        assert_eq!(remote.user.as_deref(), Some("sys"));
        assert_eq!(remote.sysdba, Some(true));
        assert_eq!(remote.source_db_link.as_deref(), Some("PDB_BRANCH_ORIGIN"));
    }
}
