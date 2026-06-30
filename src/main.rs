use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env;
use std::path::PathBuf;
use time_whisperer::{config, logging, monitor, server};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_COMMIT: &str = match option_env!("GIT_COMMIT") {
    Some(v) => v,
    None => "unknown",
};
pub const BUILD_DATE: &str = match option_env!("BUILD_DATE") {
    Some(v) => v,
    None => "unknown",
};

#[derive(Parser, Debug)]
#[command(author, about = "SneakTime - Upwork Screenshot Monitor", disable_version_flag = true)]
struct Cli {
    /// Print version and exit
    #[arg(long)]
    version: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Install as a per-user LaunchAgent (start at login) and start it now
    Install,
    /// Stop and remove the LaunchAgent
    Uninstall,
    /// Print whether the LaunchAgent is installed and running
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.version {
        println!("SneakTime {} ({}, {})", VERSION, GIT_COMMIT, BUILD_DATE);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if let Some(cmd) = &cli.command {
        return run_agent_command(cmd);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = &cli.command;

    let cfg_path = config::config_path();
    let (cfg, cfg_source) = match config::load(&cfg_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Unable to read config {}: {}", cfg_path.display(), e);
            std::process::exit(1);
        }
    };

    let _log_guard = logging::init(&cfg.log_path, cfg.debug_mode);

    let abs_cfg = std::fs::canonicalize(&cfg_path).unwrap_or(cfg_path.clone());
    tracing::info!("Config file path: {}", abs_cfg.display());
    tracing::info!("Loaded config: {:?}", cfg);

    if let Err(msg) = config::validate(&cfg) {
        eprintln!("Configuration error: {}", msg);
        eprintln!("Config source: {}", cfg_source);
        eprintln!("Please fix your configuration and try again.");
        if let Ok(s) = serde_json::to_string_pretty(&cfg) {
            eprintln!("Current config content:\n{}", s);
        }
        std::process::exit(1);
    }

    tracing::info!(
        "SneakTime {} (commit {}, built {})",
        VERSION,
        GIT_COMMIT,
        BUILD_DATE
    );
    tracing::info!("Using configuration from: {}", cfg_source);
    tracing::info!("Logs are also written to {}", cfg.log_path);

    // Refuse to run a second daemon. Held for the whole process; released by the
    // kernel on exit/crash. Best-effort: a lockfile fs error must not brick us.
    let _instance_lock = match acquire_single_instance_lock() {
        Ok(Some(file)) => Some(file),
        Ok(None) => {
            tracing::warn!("Another Worklog daemon already holds the instance lock — exiting.");
            eprintln!("Another Worklog daemon is already running — exiting.");
            return Ok(());
        }
        Err(e) => {
            tracing::warn!("Could not acquire single-instance lock ({e}); continuing without it.");
            None
        }
    };

    let srv = server::start(VERSION.to_string(), GIT_COMMIT.to_string(), cfg.debug_mode).await?;
    tracing::info!("WebSocket server started on port {}", srv.port);

    let dir = env::var("UPWORK_LOGS_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&cfg.upwork_logs_dir));

    if dir.as_os_str().is_empty() {
        tracing::error!("cannot determine Upwork log directory");
        std::process::exit(1);
    }
    tracing::info!("Monitoring Upwork logs in {}", dir.display());

    let (monitor_shutdown_tx, monitor_shutdown_rx) = tokio::sync::oneshot::channel();
    let broadcaster = srv.broadcaster.clone();
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = monitor::run_monitor(dir, broadcaster, monitor_shutdown_rx).await {
            tracing::error!("Monitor exited with error: {}", e);
        }
    });

    wait_for_shutdown().await;
    tracing::info!("shutting down");

    let _ = monitor_shutdown_tx.send(());
    let _ = monitor_handle.await;
    srv.shutdown().await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

/// Single-instance guard. Acquires an exclusive advisory lock on a fixed
/// lockfile next to the config. The kernel releases it automatically when this
/// process exits or crashes, so there is no stale lock to reap (unlike a PID
/// file). Returns:
///   * `Ok(Some(file))` — we hold the lock; keep the handle alive for the whole
///     process so the lock is held until exit.
///   * `Ok(None)` — another daemon already holds the lock; the caller should exit.
///   * `Err(_)` — the lock could not be established (filesystem error); the
///     caller should continue without it rather than fail to run at all.
fn acquire_single_instance_lock() -> Result<Option<std::fs::File>> {
    use fs2::FileExt;
    let mut lock_path = config::config_path();
    lock_path.set_file_name("worklog.lock");
    if let Some(parent) = lock_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(target_os = "macos")]
fn run_agent_command(cmd: &Command) -> Result<()> {
    use time_whisperer::launchagent::LaunchAgent;
    let agent = LaunchAgent::with_default_label();
    match cmd {
        Command::Install => {
            let exe = std::env::current_exe()?;
            agent.install(&exe)?;
            println!("Installed and started LaunchAgent ({}).", agent.label);
            println!("Daemon: {}", exe.display());
            println!("It will now start automatically at login.");
        }
        Command::Uninstall => {
            agent.uninstall()?;
            println!("Removed LaunchAgent ({}). It will no longer start at login.", agent.label);
        }
        Command::Status => {
            println!("LaunchAgent: {}", agent.label);
            println!("  installed (start at login): {}", yes_no(agent.is_installed()));
            println!("  running now:                {}", yes_no(agent.is_running()));
            println!("  plist: {}", agent.plist_path().display());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
