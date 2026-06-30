pub mod config;
#[cfg(target_os = "macos")]
pub mod launchagent;
pub mod logging;
pub mod monitor;
pub mod ports;
pub mod server;
pub mod timestamp;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
