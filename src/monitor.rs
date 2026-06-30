use crate::config::expand_path;
use crate::server::Broadcaster;
use crate::timestamp::parse_ts;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Local};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::sync::oneshot;

pub const SCREENSHOT_PATTERN: &str = "Electron Screensnap succeeded";

/// Glob for the screenshot log specifically. Upwork's main (renderer) log is
/// `upwork..YYYYMMDD.log` — note the *double* dot. Other files in the same
/// directory (`upwork.cmon.*.log` connection-monitor, `dash..upwork.*.log`)
/// never contain "Electron Screensnap succeeded", so we must NOT consider them:
/// they get written continuously and would otherwise win the most-recently-
/// modified race, causing the monitor to flap off the real screenshot log.
pub const SCREENSHOT_LOG_GLOB: &str = "upwork..*.log";

/// True if `name` is the screenshot log we tail (`upwork..*.log`, double dot).
fn is_screenshot_log(name: &str) -> bool {
    name.starts_with("upwork..") && name.ends_with(".log")
}

/// Finds the most-recently-modified screenshot log (`upwork..*.log`) in `dir`.
pub fn find_latest_log(dir: &Path) -> Option<PathBuf> {
    let pattern = dir.join(SCREENSHOT_LOG_GLOB);
    let entries = glob::glob(&pattern.to_string_lossy()).ok()?;

    let mut latest: Option<(PathBuf, SystemTime)> = None;
    for entry in entries.flatten() {
        // Belt-and-suspenders: the glob already excludes cmon/dash logs, but
        // re-check by name so the rule lives in one place.
        let is_match = entry
            .file_name()
            .and_then(|n| n.to_str())
            .map(is_screenshot_log)
            .unwrap_or(false);
        if !is_match {
            continue;
        }
        let meta = match fs::metadata(&entry) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().ok()?;
        match &latest {
            Some((_, t)) if *t >= mtime => {}
            _ => latest = Some((entry, mtime)),
        }
    }
    latest.map(|(p, _)| p)
}

/// Returns every screenshot timestamp present in `log_file`, formatted RFC3339-ish.
#[allow(dead_code)]
pub fn get_all_screenshot_timestamps(log_file: &Path) -> Vec<String> {
    let f = match File::open(log_file) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if line.contains(SCREENSHOT_PATTERN) {
            if let Some(ts) = parse_ts(&line) {
                out.push(ts.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true));
            }
        }
    }
    out
}

/// Returns the latest screenshot timestamp and its full line.
#[allow(dead_code)]
pub fn last_screenshot_info(
    log_file: &Path,
) -> Result<(Option<DateTime<Local>>, Option<String>)> {
    let f = File::open(log_file)
        .with_context(|| format!("opening {}", log_file.display()))?;
    let reader = BufReader::new(f);

    let mut latest: Option<DateTime<Local>> = None;
    let mut latest_line: Option<String> = None;
    for line in reader.lines().map_while(Result::ok) {
        if line.contains(SCREENSHOT_PATTERN) {
            if let Some(ts) = parse_ts(&line) {
                if latest.map(|t| ts > t).unwrap_or(true) {
                    latest = Some(ts);
                    latest_line = Some(line);
                }
            }
        }
    }
    Ok((latest, latest_line))
}

struct Tail {
    /// Display path (as discovered via glob).
    path: PathBuf,
    file: File,
    /// Byte offset of the next unread byte. Only ever advanced past a complete
    /// (newline-terminated) line, so a partial trailing line is never consumed
    /// until its newline arrives.
    pos: u64,
    /// Identity of the open file (unix only). Used to detect rotate-then-recreate
    /// where the path stays the same but the file behind it is new.
    #[cfg(unix)]
    inode: u64,
}

impl Tail {
    fn open(path: PathBuf, seek_to_end: bool) -> Result<Self> {
        let file = File::open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let len = file.metadata()?.len();
        let pos = if seek_to_end { len } else { 0 };
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            file.metadata()?.ino()
        };
        Ok(Self {
            path,
            file,
            pos,
            #[cfg(unix)]
            inode,
        })
    }

    /// Reads complete lines appended since the last call. Detects truncation
    /// (file shrank below our cursor) and rewinds to the start. A partial
    /// trailing line (no newline yet) is left unconsumed for the next call.
    fn read_new_lines(&mut self) -> std::io::Result<Vec<String>> {
        let size = self.file.metadata()?.len();
        if size < self.pos {
            // copy-truncate / in-place rotation: start over from the top.
            self.pos = 0;
        }
        if size <= self.pos {
            return Ok(Vec::new());
        }
        self.file.seek(SeekFrom::Start(self.pos))?;
        let to_read = (size - self.pos) as usize;
        let mut buf = vec![0u8; to_read];
        let n = self.file.read(&mut buf)?;
        buf.truncate(n);

        // Only consume up to the last complete line.
        let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else {
            return Ok(Vec::new()); // no complete line yet
        };
        self.pos += (last_nl + 1) as u64;
        let text = String::from_utf8_lossy(&buf[..=last_nl]);
        Ok(text.lines().map(|l| l.to_string()).collect())
    }

    /// True if the file currently at `self.path` has a different identity
    /// (inode) than the one we opened. Indicates a rotation we missed.
    #[cfg(unix)]
    fn path_inode_changed(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        match fs::metadata(&self.path) {
            Ok(m) => m.ino() != self.inode,
            // If the path is gone, treat that as "changed" — we want a re-scan.
            Err(_) => true,
        }
    }

    #[cfg(not(unix))]
    fn path_inode_changed(&self) -> bool {
        // No cheap identity check on non-unix; periodic find_latest_log handles it.
        false
    }
}

/// Reads any new lines from the current tail and broadcasts a notification for
/// each newly-seen screenshot.
fn drain_new_lines(
    tail: &mut Option<Tail>,
    seen: &mut HashMap<String, DateTime<Local>>,
    last_seen: &mut Option<DateTime<Local>>,
    keep: Duration,
    broadcaster: &Broadcaster,
) {
    let Some(t) = tail.as_mut() else { return };
    let lines = match t.read_new_lines() {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("Read error on {}: {}", t.path.display(), e);
            return;
        }
    };
    for line in lines {
        if !line.contains(SCREENSHOT_PATTERN) {
            continue;
        }
        let Some(ts) = parse_ts(&line) else { continue };
        if let Some(last) = *last_seen {
            if ts <= last {
                continue;
            }
        }
        let key = ts.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
        if seen.contains_key(&key) {
            continue;
        }
        seen.insert(key, ts);
        *last_seen = Some(ts);
        prune(seen, ts, keep);

        tracing::info!("Screenshot detected at {}", ts.format("%H:%M:%S"));
        notify_screenshot(broadcaster, ts);
    }
}

/// Runs the log monitor: tails the latest `upwork.*.log` in `dir` and broadcasts
/// `screenshot_detected` for each new "Electron Screensnap succeeded" line.
///
/// Returns when `shutdown_rx` resolves.
pub async fn run_monitor(
    dir: PathBuf,
    broadcaster: Broadcaster,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<()> {
    let dir = expand_path(dir.to_string_lossy().as_ref());
    if !dir.exists() {
        return Err(anyhow!("upwork logs directory does not exist: {}", dir.display()));
    }

    let keep = Duration::hours(48);
    let mut seen: HashMap<String, DateTime<Local>> = HashMap::new();
    let mut last_seen: Option<DateTime<Local>> = None;
    let mut tail: Option<Tail> = None;

    // `first_open` controls whether we skip the existing contents (seek to EOF).
    // The very first open at startup skips history so we don't replay the whole
    // day. Every later switch (rotation, inode swap) reads from the start and
    // relies on `last_seen` + the dedup set, so a screenshot written to a new
    // file *before* we noticed it is never lost.
    let open_current = |tail: &mut Option<Tail>, first_open: bool| -> Result<()> {
        let Some(fname) = find_latest_log(&dir) else {
            return Ok(());
        };
        if let Some(t) = tail.as_ref() {
            // Same path AND same underlying file (inode) → keep tailing.
            if t.path == fname && !t.path_inode_changed() {
                return Ok(());
            }
            if t.path == fname && t.path_inode_changed() {
                tracing::info!(
                    "Tailed file {} was replaced (inode changed) — reopening from start",
                    t.path.display()
                );
            }
        }
        // Drop previous before opening new — guarantees we never hold a stale fd.
        *tail = None;
        let new_tail = Tail::open(fname.clone(), first_open)?;
        tracing::info!("Monitoring log file: {}", new_tail.path.display());
        *tail = Some(new_tail);
        Ok(())
    };

    if let Err(e) = open_current(&mut tail, true) {
        tracing::warn!("Initial log open failed, will retry: {}", e);
    }

    // Poll-based tailing. We deliberately do NOT use filesystem notifications:
    // macOS FSEvents coalesces/drops modify events for a held-open, continuously-
    // appended log (Go's fsnotify avoided this only because it used kqueue), so
    // events are not a reliable wakeup. Polling is simple, deterministic, cross-
    // platform, and cheap — a stat plus a read of only the newly-appended bytes.
    //
    //  * content poll  — read appended lines from the current file (default 1s)
    //  * rotation poll — switch to a new day's file / detect inode swap (default 15s)
    let poll_secs: u64 = std::env::var("TIME_WHISPERER_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let rescan_secs: u64 = std::env::var("TIME_WHISPERER_RESCAN_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    tracing::info!(
        "Polling for screenshots every {}s (rotation check every {}s)",
        poll_secs.max(1),
        rescan_secs.max(1)
    );

    let mut poll_tick = tokio::time::interval(std::time::Duration::from_secs(poll_secs.max(1)));
    poll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    poll_tick.tick().await; // skip immediate first tick

    let mut rescan_tick = tokio::time::interval(std::time::Duration::from_secs(rescan_secs.max(1)));
    rescan_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    rescan_tick.tick().await; // skip immediate first tick

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Monitor shutting down");
                return Ok(());
            }
            _ = rescan_tick.tick() => {
                if let Err(e) = open_current(&mut tail, false) {
                    tracing::warn!("Rotation rescan error: {}", e);
                }
            }
            _ = poll_tick.tick() => {
                drain_new_lines(&mut tail, &mut seen, &mut last_seen, keep, &broadcaster);
            }
        }
    }
}

fn prune(seen: &mut HashMap<String, DateTime<Local>>, now: DateTime<Local>, keep: Duration) {
    seen.retain(|_, v| now.signed_duration_since(*v) <= keep);
}

fn notify_screenshot(broadcaster: &Broadcaster, ts: DateTime<Local>) {
    let payload = json!({
        "timestamp": ts.format("%H:%M:%S").to_string(),
        "time": ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    });
    broadcaster.broadcast(crate::server::WsMessage {
        msg_type: "screenshot_detected".into(),
        payload: Some(payload),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn append(path: &Path, s: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn read_new_lines_handles_appends_partials_and_truncation() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("upwork..20260523.log");
        File::create(&p).unwrap().write_all(b"line1\n").unwrap();

        // seek_to_end = false → read from the top.
        let mut t = Tail::open(p.clone(), false).unwrap();
        assert_eq!(t.read_new_lines().unwrap(), vec!["line1"]);
        // Nothing new on a second call.
        assert_eq!(t.read_new_lines().unwrap(), Vec::<String>::new());

        // Append a full line.
        append(&p, "line2\n");
        assert_eq!(t.read_new_lines().unwrap(), vec!["line2"]);

        // Append a partial line (no newline) — must NOT be consumed yet.
        append(&p, "partial-no-newline");
        assert_eq!(t.read_new_lines().unwrap(), Vec::<String>::new());

        // Complete the partial line — now it comes through whole.
        append(&p, "-now-complete\n");
        assert_eq!(
            t.read_new_lines().unwrap(),
            vec!["partial-no-newline-now-complete"]
        );

        // Truncate (copy-truncate rotation) and write fresh content.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(0)
            .unwrap();
        append(&p, "after-truncate\n");
        assert_eq!(t.read_new_lines().unwrap(), vec!["after-truncate"]);
    }

    #[test]
    fn read_new_lines_seek_to_end_skips_existing_history() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("upwork..20260523.log");
        File::create(&p)
            .unwrap()
            .write_all(b"old1\nold2\n")
            .unwrap();

        // seek_to_end = true → existing content skipped.
        let mut t = Tail::open(p.clone(), true).unwrap();
        assert_eq!(t.read_new_lines().unwrap(), Vec::<String>::new());

        append(&p, "new1\n");
        assert_eq!(t.read_new_lines().unwrap(), vec!["new1"]);
    }

    #[test]
    fn find_latest_log_picks_newest_screenshot_log_and_ignores_others() {
        use filetime::{set_file_mtime, FileTime};
        let dir = tempdir().unwrap();
        let now = SystemTime::now();
        let h = std::time::Duration::from_secs(3600);

        // (name, mtime). The cmon and dash files are NEWER than every screenshot
        // log, to prove they are excluded by name rather than just losing the
        // mtime race.
        let files = [
            ("upwork..20250410.log", now - h * 48),
            ("upwork..20250411.log", now - h * 24),
            ("upwork..20250412.log", now - h * 2), // newest screenshot log
            ("upwork.cmon.20250412.log", now),     // newest overall, must be ignored
            ("dash..upwork.20250412.log", now),    // newest overall, must be ignored
        ];
        for (name, t) in files {
            let p = dir.path().join(name);
            File::create(&p).unwrap().write_all(b"x").unwrap();
            set_file_mtime(&p, FileTime::from_system_time(t)).unwrap();
        }

        let got = find_latest_log(dir.path()).unwrap();
        assert_eq!(
            got.file_name().unwrap().to_str().unwrap(),
            "upwork..20250412.log",
            "should pick the newest upwork..*.log and ignore cmon/dash logs"
        );
    }

    #[test]
    fn is_screenshot_log_classifies_correctly() {
        assert!(is_screenshot_log("upwork..20260523.log"));
        assert!(!is_screenshot_log("upwork.cmon.20260523.log"));
        assert!(!is_screenshot_log("dash..upwork.20260523.log"));
        assert!(!is_screenshot_log("upwork..20260523.log.bak"));
    }

    #[test]
    fn last_screenshot_info_returns_latest() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("upwork..20250410.log");
        let mut f = File::create(&p).unwrap();
        writeln!(f, "[2025-04-10T10:00:00.000] foo").unwrap();
        writeln!(f, "[2025-04-10T12:30:45.123] [INFO] main.shell.os_services - Electron Screensnap succeeded.").unwrap();
        writeln!(f, "[2025-04-10T15:00:00.000] bar").unwrap();
        writeln!(f, "[2025-04-10T18:45:30.456] [INFO] main.shell.os_services - Electron Screensnap succeeded.").unwrap();
        drop(f);

        let (ts, line) = last_screenshot_info(&p).unwrap();
        let ts = ts.unwrap();
        assert_eq!(ts.format("%Y-%m-%dT%H:%M:%S%.3f").to_string(), "2025-04-10T18:45:30.456");
        assert!(line.unwrap().contains("18:45:30.456"));
    }

    #[test]
    fn get_all_screenshot_timestamps_returns_all() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("upwork..20250410.log");
        let mut f = File::create(&p).unwrap();
        writeln!(f, "[2025-04-10T10:30:45.123] [INFO] main.shell.os_services - Electron Screensnap succeeded.").unwrap();
        writeln!(f, "[2025-04-10T11:00:00.000] foo").unwrap();
        writeln!(f, "[2025-04-10T12:45:30.456] [INFO] main.shell.os_services - Electron Screensnap succeeded.").unwrap();
        writeln!(f, "[2025-04-10T13:15:20.789] [INFO] main.shell.os_services - Electron Screensnap succeeded.").unwrap();
        drop(f);

        let got = get_all_screenshot_timestamps(&p);
        assert_eq!(got.len(), 3);
    }
}

