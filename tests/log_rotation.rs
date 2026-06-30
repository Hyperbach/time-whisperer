//! Verifies the monitor survives copy-truncate rotation.
//!
//! Mirrors the Go test `TestTruncateWhileOpen` from `log_rotation_test.go`.

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
struct TestSink(Arc<Mutex<Vec<WsMessage>>>);

impl TestSink {
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
        .expect("open log for append");
    f.write_all(line.as_bytes()).expect("write line");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncate_while_open() {
    // 1s poll so the truncation is observed promptly.
    std::env::set_var("TIME_WHISPERER_POLL_SECS", "1");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .with_test_writer()
        .try_init();

    let dir = tempdir().expect("tempdir");
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("mkdir logs");
    let log_path = log_dir.join("upwork..20250523.log");
    File::create(&log_path).expect("create log");

    // Capture broadcasts via a test broadcaster.
    let sink = TestSink::default();
    let bc = Broadcaster::test_sink({
        let sink = sink.clone();
        move |msg| {
            let sink = sink.clone();
            tokio::spawn(async move { sink.0.lock().await.push(msg); });
        }
    });

    let (sd_tx, sd_rx) = tokio::sync::oneshot::channel();
    let dir_clone = log_dir.clone();
    let monitor = tokio::spawn(async move {
        let _ = run_monitor(dir_clone, bc, sd_rx).await;
    });

    // Allow watcher warm-up.
    tokio::time::sleep(StdDuration::from_millis(200)).await;

    append_screenshot(&log_path, "10:00:00.000");
    wait_for(&sink, 1, StdDuration::from_secs(3))
        .await
        .expect("first append detected");

    // copy-truncate simulation
    std::fs::OpenOptions::new()
        .write(true)
        .open(&log_path)
        .expect("open for truncate")
        .set_len(0)
        .expect("truncate");

    // Give the poll a chance to observe the shrunk file (size < cursor) and
    // reset to the start. With size-based tailing this is the deterministic
    // way to recover from copy-truncate.
    tokio::time::sleep(StdDuration::from_millis(1500)).await;

    append_screenshot(&log_path, "10:00:05.000");
    wait_for(&sink, 2, StdDuration::from_secs(3))
        .await
        .expect("second append detected after truncate");

    let _ = sd_tx.send(());
    let _ = monitor.await;
}

async fn wait_for(sink: &TestSink, want: usize, max: StdDuration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + max;
    while std::time::Instant::now() < deadline {
        if sink.count().await >= want {
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from_millis(50)).await;
    }
    Err(format!("only saw {} of {} expected detections", sink.count().await, want))
}
