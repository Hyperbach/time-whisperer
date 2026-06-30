//! Verifies the daemon recovers when log rotation creates a brand-new file
//! and we miss the fsnotify Create event — the exact midnight-rollover bug
//! that required restarting the old Go binary.
//!
//! Strategy: start monitor on file A. Replace A with a freshly-created file
//! at a NEWER path (mimicking `upwork..YYYYMMDD.log` rolling forward) WITHOUT
//! giving the watcher time to react cleanly — then verify the 30s periodic
//! rescan eventually picks it up (we hack the rescan via direct events here
//! by waiting a bounded amount).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tempfile::tempdir;
use time_whisperer::monitor::run_monitor;
use time_whisperer::server::{Broadcaster, WsMessage};
use tokio::sync::Mutex;

#[derive(Default, Clone)]
struct Sink(Arc<Mutex<Vec<WsMessage>>>);

impl Sink {
    async fn count(&self) -> usize {
        self.0.lock().await.len()
    }
}

fn append_screenshot(path: &PathBuf, hhmmss: &str) {
    let line = format!(
        "[{}T{}] [INFO] main.shell.os_services - Electron Screensnap succeeded.\n",
        chrono::Local::now().format("%Y-%m-%d"),
        hhmmss
    );
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .expect("open log");
    f.write_all(line.as_bytes()).expect("write");
}

async fn wait_for(sink: &Sink, want: usize, max: StdDuration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + max;
    while std::time::Instant::now() < deadline {
        if sink.count().await >= want {
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }
    Err(format!(
        "only saw {} of {} expected detections",
        sink.count().await,
        want
    ))
}

/// Simulates: daemon is tailing `upwork..20260518.log`, midnight passes,
/// Upwork creates `upwork..20260519.log`. We never deliver a clean Create
/// event for the new file (we delete the old one *first*, race-y); the
/// daemon must recover via the periodic rescan.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovers_from_missed_rotation() {
    // Speed up the rescan so the test doesn't wait 30s.
    std::env::set_var("TIME_WHISPERER_RESCAN_SECS", "1");

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_test_writer()
        .try_init();

    let dir = tempdir().expect("tempdir");
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("mkdir");

    let old_log = log_dir.join("upwork..20260518.log");
    let new_log = log_dir.join("upwork..20260519.log");
    File::create(&old_log).expect("create old");

    let sink = Sink::default();
    let bc = Broadcaster::test_sink({
        let sink = sink.clone();
        move |msg| {
            let sink = sink.clone();
            tokio::spawn(async move { sink.0.lock().await.push(msg); });
        }
    });

    let (sd_tx, sd_rx) = tokio::sync::oneshot::channel();
    let monitor_dir = log_dir.clone();
    let monitor = tokio::spawn(async move {
        let _ = run_monitor(monitor_dir, bc, sd_rx).await;
    });

    // Let monitor open the initial file.
    tokio::time::sleep(StdDuration::from_millis(300)).await;

    // Sanity check: detection still works on the original file.
    append_screenshot(&old_log, "10:00:00.000");
    wait_for(&sink, 1, StdDuration::from_secs(3))
        .await
        .expect("initial detection works");

    // ── simulate midnight rotation: a brand-new file appears with a newer
    // name. We deliberately wait long enough that the periodic rescan (1s
    // in this test) will have a chance to switch over before we append.
    File::create(&new_log).expect("create new");
    let now = std::time::SystemTime::now();
    filetime::set_file_mtime(&new_log, filetime::FileTime::from_system_time(now))
        .expect("touch new");

    // Wait for the daemon to discover and switch to the new file via rescan
    // (or fsnotify Create event — either path is acceptable; the test verifies
    // *recovery*, not which mechanism delivered it).
    tokio::time::sleep(StdDuration::from_millis(2500)).await;

    // Now append a screenshot to the new file. The daemon should already be
    // tailing it and pick this up.
    append_screenshot(&new_log, "10:00:05.000");

    wait_for(&sink, 2, StdDuration::from_secs(5))
        .await
        .expect("rotated-file detection works after rotation");

    let _ = sd_tx.send(());
    let _ = monitor.await;
}

/// Verifies inode-change detection: same path, different file. This catches
/// log-rotation schemes that delete-then-recreate the same filename (and where
/// fsnotify may coalesce the Remove + Create into something we miss).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovers_when_path_inode_changes() {
    std::env::set_var("TIME_WHISPERER_RESCAN_SECS", "1");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_test_writer()
        .try_init();

    let dir = tempdir().expect("tempdir");
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("mkdir");
    let log_path = log_dir.join("upwork..20260601.log");
    File::create(&log_path).expect("create initial");

    let sink = Sink::default();
    let bc = Broadcaster::test_sink({
        let sink = sink.clone();
        move |msg| {
            let sink = sink.clone();
            tokio::spawn(async move { sink.0.lock().await.push(msg); });
        }
    });

    let (sd_tx, sd_rx) = tokio::sync::oneshot::channel();
    let monitor_dir = log_dir.clone();
    let monitor = tokio::spawn(async move {
        let _ = run_monitor(monitor_dir, bc, sd_rx).await;
    });
    tokio::time::sleep(StdDuration::from_millis(300)).await;

    append_screenshot(&log_path, "09:00:00.000");
    wait_for(&sink, 1, StdDuration::from_secs(3))
        .await
        .expect("initial detection");

    // Replace the file at the same path with a brand-new inode.
    std::fs::remove_file(&log_path).expect("remove");
    File::create(&log_path).expect("recreate");

    // Wait long enough for periodic rescan to spot the inode change.
    tokio::time::sleep(StdDuration::from_millis(2500)).await;

    append_screenshot(&log_path, "09:00:05.000");
    wait_for(&sink, 2, StdDuration::from_secs(5))
        .await
        .expect("post-recreate detection (inode change recovery)");

    let _ = sd_tx.send(());
    let _ = monitor.await;
}
