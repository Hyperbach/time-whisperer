use crate::config::expand_path;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// Optional guard returned by `init`. Drop with the program.
pub struct LogGuard {
    _file_handle: Option<std::fs::File>,
}

/// Initialise tracing to stdout (+ optionally a log file).
pub fn init(log_path: &str, debug: bool) -> LogGuard {
    let path: PathBuf = expand_path(log_path);

    let default_filter = if debug { "info,time_whisperer=debug" } else { "info" };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    if path.as_os_str().is_empty() {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_writer(std::io::stdout)
            .init();
        return LogGuard { _file_handle: None };
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            // Tee stdout + file by composing two MakeWriters.
            let file_clone = file.try_clone().expect("clone log file handle");
            let writer = std::io::stdout.and(move || {
                file_clone
                    .try_clone()
                    .expect("clone log file handle for write")
            });
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .with_writer(writer)
                .init();
            LogGuard { _file_handle: Some(file) }
        }
        Err(_) => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .with_writer(std::io::stdout)
                .init();
            LogGuard { _file_handle: None }
        }
    }
}
