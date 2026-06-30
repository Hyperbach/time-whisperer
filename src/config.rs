use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "debugMode")]
    pub debug_mode: bool,
    #[serde(default, rename = "logPath")]
    pub log_path: String,
    #[serde(default, rename = "upworkLogsDir")]
    pub upwork_logs_dir: String,
    #[serde(default, rename = "webSocketPort")]
    pub web_socket_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            debug_mode: false,
            log_path: home.join("time-whisperer.log").to_string_lossy().into_owned(),
            upwork_logs_dir: String::new(),
            web_socket_port: 8887,
        }
    }
}

/// Expands a leading `~` to the user's home directory.
pub fn expand_path(path: &str) -> PathBuf {
    if path.is_empty() {
        return PathBuf::new();
    }
    if let Some(rest) = path.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            let trimmed = rest.strip_prefix('/').unwrap_or(rest);
            return home.join(trimmed);
        }
    }
    PathBuf::from(path)
}

pub fn validate(cfg: &Config) -> Result<(), String> {
    if cfg.log_path.is_empty() {
        return Err("logPath cannot be empty in config".into());
    }
    if cfg.upwork_logs_dir.is_empty() {
        return Err("upworkLogsDir cannot be empty in config".into());
    }
    if cfg.web_socket_port == 0 {
        return Err(format!(
            "invalid webSocketPort: {} (must be between 1-65535)",
            cfg.web_socket_port
        ));
    }
    if cfg.log_path.starts_with('~') {
        let expanded = expand_path(&cfg.log_path);
        if let Some(parent) = expanded.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(format!(
                        "cannot create log directory {}: {}",
                        parent.display(),
                        e
                    ));
                }
            }
        }
    }
    Ok(())
}

fn default_upwork_log_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Upwork/Upwork/Logs")
    } else if cfg!(target_os = "windows") {
        home.join("AppData/Roaming/Upwork/Logs")
    } else {
        home.join(".config/Upwork/Logs")
    }
}

/// Tries to discover the Upwork logs directory by probing known locations.
pub fn discover_upwork_logs_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support/Upwork/Upwork/Logs")]
    } else if cfg!(target_os = "windows") {
        vec![home.join("AppData/Roaming/Upwork/Logs")]
    } else {
        vec![
            home.join(".config/Upwork/Logs"),
            home.join(".Upwork/Upwork/Logs"),
        ]
    };

    for path in candidates {
        tracing::info!("Checking for Upwork logs in: {}", path.display());
        if !path.exists() {
            tracing::info!("Directory does not exist: {}", path.display());
            continue;
        }
        let pattern = path.join("upwork.*.log");
        match glob::glob(&pattern.to_string_lossy()) {
            Ok(matches) => {
                let count = matches.filter_map(Result::ok).count();
                if count > 0 {
                    tracing::info!("Found {} upwork log file(s) in: {}", count, path.display());
                    return Some(path);
                }
                tracing::info!("No upwork log files found in: {}", path.display());
            }
            Err(e) => tracing::warn!("Error checking for log files in {}: {}", path.display(), e),
        }
    }
    tracing::warn!("No valid Upwork logs directory discovered");
    None
}

fn ensure_upwork_logs_dir(cfg: &mut Config) {
    if cfg.upwork_logs_dir.is_empty() {
        tracing::info!("UpworkLogsDir is empty, attempting to discover...");
        if let Some(p) = discover_upwork_logs_dir() {
            cfg.upwork_logs_dir = p.to_string_lossy().into_owned();
            tracing::info!("Discovered and set UpworkLogsDir: {}", cfg.upwork_logs_dir);
        } else {
            let fallback = default_upwork_log_dir();
            cfg.upwork_logs_dir = fallback.to_string_lossy().into_owned();
            tracing::info!(
                "Discovery failed, using default UpworkLogsDir: {}",
                cfg.upwork_logs_dir
            );
        }
    }
}

/// Returns the path to the OS-specific bundled default config (if present).
fn bundled_config_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    if cfg!(target_os = "macos") {
        let in_bundle = dir.join("../Resources/default_config.json");
        if in_bundle.exists() {
            return Some(in_bundle);
        }
        Some(dir.join("configs/macos/default_config.json"))
    } else if cfg!(target_os = "windows") {
        Some(dir.join("configs/windows/default_config.json"))
    } else {
        Some(dir.join("configs/linux/default_config.json"))
    }
}

pub fn config_path() -> PathBuf {
    // 1. local dev: config.json in cwd
    let cwd_cfg = PathBuf::from("config.json");
    if cwd_cfg.exists() {
        return cwd_cfg;
    }
    // 2. env var override
    if let Ok(p) = env::var("TIME_WHISPERER_CONFIG_PATH") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    // 3. OS-specific
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/TimeWhisperer")
    } else if cfg!(target_os = "windows") {
        home.join("AppData/Local/TimeWhisperer")
    } else {
        home.join(".config/time-whisperer")
    };
    dir.join("config.json")
}

pub fn save(cfg: &Config, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let body = serde_json::to_vec_pretty(cfg).context("serializing config")?;
    fs::write(path, body).with_context(|| format!("writing config to {}", path.display()))?;
    Ok(())
}

/// Loads a config from `path`, falling back to bundled defaults, then hardcoded defaults.
/// Returns `(config, source_description)`.
pub fn load(path: &Path) -> Result<(Config, String)> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Config>(&bytes) {
            Ok(mut cfg) => {
                let original = cfg.upwork_logs_dir.clone();
                ensure_upwork_logs_dir(&mut cfg);
                if cfg.upwork_logs_dir != original {
                    let _ = save(&cfg, path);
                }
                let source = format!("User config: {}", path.display());
                Ok((cfg, source))
            }
            Err(parse_err) => {
                // Back up the invalid file and return an error.
                let stamp = Utc::now().format("%Y%m%dT%H%M%S%.9f");
                let bak = format!("{}.bak-{}", path.display(), stamp);
                fs::rename(path, &bak).with_context(|| {
                    format!("failed to back up invalid config to {}", bak)
                })?;
                tracing::warn!("config: backed up invalid file to {}", bak);
                Err(anyhow!("invalid json: {}", parse_err))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Fall through to bundled/default
            if let Some(bp) = bundled_config_path() {
                if let Ok(bytes) = fs::read(&bp) {
                    if let Ok(mut cfg) = serde_json::from_slice::<Config>(&bytes) {
                        ensure_upwork_logs_dir(&mut cfg);
                        let _ = save(&cfg, path);
                        return Ok((cfg, format!("Bundled config: {}", bp.display())));
                    }
                }
            }
            let mut cfg = Config::default();
            ensure_upwork_logs_dir(&mut cfg);
            let _ = save(&cfg, path);
            Ok((cfg, "Default hardcoded config (no config file found)".into()))
        }
        Err(e) => Err(anyhow!("failed to read config: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_invalid_json_backs_up_and_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let invalid = br#"{
            "debugMode": tru,
            "logPath": "/path/to/log",
            "upworkLogsDir": "/path/to/upwork",
            "webSocketPort": 8080
        }"#;
        fs::write(&path, invalid).unwrap();

        let err = load(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("invalid json"), "expected 'invalid json', got: {}", msg);

        assert!(!path.exists(), "original config.json should be moved");
        let backup_found = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("config.json.bak-"));
        assert!(backup_found, "expected a config.json.bak-* backup file");
    }

    #[test]
    fn loads_missing_file_falls_back_to_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let (_cfg, source) = load(&path).expect("should fall back");
        assert!(
            source.contains("Default hardcoded config") || source.contains("Bundled config"),
            "unexpected source: {}",
            source
        );
        assert!(path.exists(), "config should be written to disk");
    }

    #[test]
    fn validate_rejects_empty_log_path() {
        let cfg = Config {
            debug_mode: false,
            log_path: String::new(),
            upwork_logs_dir: "/x".into(),
            web_socket_port: 8887,
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_empty_upwork_dir() {
        let cfg = Config {
            debug_mode: false,
            log_path: "/x".into(),
            upwork_logs_dir: String::new(),
            web_socket_port: 8887,
        };
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn expand_path_handles_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_path("~/foo"), home.join("foo"));
        assert_eq!(expand_path("/abs"), PathBuf::from("/abs"));
        assert_eq!(expand_path(""), PathBuf::new());
    }
}
