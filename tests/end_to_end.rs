//! End-to-end tests that spawn the compiled `time-whisperer` binary.
//! Mirrors the Go `simple_test.go` suite.

use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::Message;

const CANDIDATE_PORTS: &[u16] = &[
    8887, 49205, 49231, 49267, 49303, 49327, 49411, 49437, 49471, 49513, 49559, 49607, 49633,
    49669, 49717, 49741, 49807, 49843, 49879, 49921, 49957, 50021, 50051, 50083, 50119, 50153,
    50207, 50239, 50273, 50311, 50359, 50413, 50441, 50483, 50509, 50551, 50617, 50653, 50677,
    50713, 50759, 50803, 50837, 50869, 50917, 50953, 51011, 51047, 51083, 51113,
];

fn binary_path() -> PathBuf {
    let mut path = std::env::current_exe().expect("current_exe");
    path.pop(); // drop test exe name
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("time-whisperer");
    path
}

fn pick_free_port() -> Option<u16> {
    for &p in CANDIDATE_PORTS {
        if TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return Some(p);
        }
    }
    None
}

struct Env {
    _tmp: TempDir,
    log_dir: PathBuf,
    log_file: PathBuf,
    config_path: PathBuf,
    port: u16,
    child: Option<Child>,
    log_tail: Arc<Mutex<String>>,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log_dir = tmp.path().join("upwork").join("logs");
        std::fs::create_dir_all(&log_dir).expect("mkdir log_dir");

        let log_file = log_dir.join(format!(
            "upwork..{}.log",
            chrono::Local::now().format("%Y%m%d")
        ));
        std::fs::File::create(&log_file).expect("create log_file");

        let port = pick_free_port().expect("free candidate port");
        // A real (temp) logPath: the daemon validates it and tees logs to both
        // the file and stdout, so detection lines still reach our captured pipe.
        let log_path = tmp.path().join("daemon.log");
        let cfg = json!({
            "debugMode": true,
            "logPath": log_path.to_string_lossy(),
            "upworkLogsDir": log_dir.to_string_lossy(),
            "webSocketPort": port,
        });
        let config_path = tmp.path().join("config.json");
        std::fs::write(&config_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();

        Self {
            _tmp: tmp,
            log_dir,
            log_file,
            config_path,
            port,
            child: None,
            log_tail: Arc::new(Mutex::new(String::new())),
        }
    }

    fn start(&mut self) {
        let bin = binary_path();
        assert!(bin.exists(), "binary not built at {}", bin.display());

        let mut cmd = Command::new(&bin);
        cmd.env("UPWORK_LOGS_DIR", &self.log_dir)
            .env("TIME_WHISPERER_CONFIG_PATH", &self.config_path)
            .env("GO_TEST", "1")
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().expect("spawn binary");

        // Pump stdout/stderr into log_tail.
        if let Some(out) = child.stdout.take() {
            let tail = self.log_tail.clone();
            std::thread::spawn(move || pump(out, tail));
        }
        if let Some(err) = child.stderr.take() {
            let tail = self.log_tail.clone();
            std::thread::spawn(move || pump(err, tail));
        }

        self.child = Some(child);
        std::thread::sleep(Duration::from_millis(800));
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            unsafe {
                libc_kill(child.id() as i32, 2 /*SIGINT*/);
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }

    fn append_screenshot(&self) -> String {
        let ts = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
        let line = format!(
            "[{}] [INFO] main.shell.os_services - Electron Screensnap succeeded.\n",
            ts
        );
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.log_file)
            .expect("append log");
        f.write_all(line.as_bytes()).expect("write line");
        ts
    }

    fn count_detections(&self) -> usize {
        let s = self.log_tail.lock().unwrap();
        s.matches("Screenshot detected at").count()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        self.stop();
    }
}

fn pump(mut reader: impl std::io::Read + Send + 'static, sink: Arc<Mutex<String>>) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let s = String::from_utf8_lossy(&buf[..n]).to_string();
                eprint!("{}", s);
                if let Ok(mut g) = sink.lock() {
                    g.push_str(&s);
                }
            }
        }
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
#[cfg(unix)]
#[allow(non_snake_case)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    kill(pid, sig)
}

fn wait_for(check: impl Fn() -> bool, max: Duration) -> bool {
    let deadline = std::time::Instant::now() + max;
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn ensure_built() {
    let bin = binary_path();
    if !bin.exists() {
        // Build with cargo.
        let status = Command::new(env!("CARGO"))
            .args(["build", "--bin", "time-whisperer"])
            .status()
            .expect("cargo build");
        assert!(status.success(), "cargo build failed");
    }
}

#[test]
fn basic_screenshot_detection() {
    ensure_built();
    let mut e = Env::new();
    e.start();
    e.append_screenshot();
    assert!(
        wait_for(|| e.count_detections() >= 1, Duration::from_secs(4)),
        "screenshot not detected\n=== output ===\n{}",
        e.log_tail.lock().unwrap()
    );
}

#[test]
fn three_screenshot_detections() {
    ensure_built();
    let mut e = Env::new();
    e.start();
    for _ in 0..3 {
        e.append_screenshot();
        std::thread::sleep(Duration::from_secs(2));
    }
    assert!(
        wait_for(|| e.count_detections() >= 3, Duration::from_secs(4)),
        "want >=3 detections, got {}\n=== output ===\n{}",
        e.count_detections(),
        e.log_tail.lock().unwrap()
    );
}

#[test]
fn existing_screenshots_not_reported() {
    ensure_built();
    let mut e = Env::new();
    // Append before the binary starts.
    let line = format!(
        "[{}] [INFO] main.shell.os_services - Electron Screensnap succeeded.\n",
        Local::now().format("%Y-%m-%dT%H:%M:%S%.3f")
    );
    write_to(&e.log_file, &line);
    write_to(&e.log_file, &line);

    e.start();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        e.count_detections(),
        0,
        "existing screenshots reported as new\n=== output ===\n{}",
        e.log_tail.lock().unwrap()
    );
}

fn write_to(p: &Path, s: &str) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(p)
        .expect("append");
    f.write_all(s.as_bytes()).expect("write");
}

// ---------- WebSocket handshake tests ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_unauthenticated_times_out() {
    ensure_built();
    let mut e = Env::new();
    e.start();

    let url = format!("ws://127.0.0.1:{}/ws", e.port);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");

    // Read hello.
    let hello = ws.next().await.expect("hello").expect("ws msg");
    let txt = match hello {
        Message::Text(t) => t,
        other => panic!("expected text hello, got {:?}", other),
    };
    let v: Value = serde_json::from_str(&txt).expect("json");
    assert_eq!(v["type"], "hello");

    // Do not respond. Expect a close within ~7s.
    let close = tokio::time::timeout(Duration::from_secs(7), ws.next()).await;
    let msg = close
        .expect("did not close in time")
        .expect("stream closed")
        .expect("read frame");
    match msg {
        Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), 1008, "expected policy violation");
        }
        Message::Close(None) => {}
        other => panic!("expected close frame, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_authenticated_receives_broadcast() {
    ensure_built();
    let mut e = Env::new();
    e.start();

    let url = format!("ws://127.0.0.1:{}/ws", e.port);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");

    let hello = ws.next().await.unwrap().unwrap();
    let hello_txt = match hello {
        Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    let hv: Value = serde_json::from_str(&hello_txt).unwrap();
    assert_eq!(hv["type"], "hello");
    let token = hv["payload"]["token"].as_str().unwrap().to_string();

    let ack = json!({"type": "hello_ack", "payload": {"token": token}}).to_string();
    ws.send(Message::Text(ack)).await.expect("send ack");

    // Should receive "connected".
    let connected = ws.next().await.unwrap().unwrap();
    let c_txt = match connected {
        Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    let cv: Value = serde_json::from_str(&c_txt).unwrap();
    assert_eq!(cv["type"], "connected");

    // Now POST to /test/broadcast.
    let body = json!({"Type": "test_broadcast", "Payload": {"foo": "bar"}});
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/test/broadcast", e.port))
        .json(&body)
        .send()
        .await
        .expect("post broadcast");
    assert!(resp.status().is_success(), "broadcast post failed: {:?}", resp);

    // Should receive it on the socket.
    let mut got = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "test_broadcast" {
                    got = true;
                    break;
                }
            }
            _ => continue,
        }
    }
    assert!(got, "did not receive test broadcast");
}
