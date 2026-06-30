use std::process::Command;

fn main() {
    // Allow Makefile / CI to override; otherwise discover.
    let commit = std::env::var("GIT_COMMIT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".into())
    });

    let date = std::env::var("BUILD_DATE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| chrono_like_now_utc());

    println!("cargo:rustc-env=GIT_COMMIT={}", commit);
    println!("cargo:rustc-env=BUILD_DATE={}", date);

    // Re-run when HEAD moves or env overrides change.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=BUILD_DATE");
}

/// Returns the current UTC time as RFC3339, computed without pulling chrono
/// as a build-dep. Fallback to epoch if the system clock is unreadable.
fn chrono_like_now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Y/M/D from days since epoch (1970-01-01).
    let days = now / 86_400;
    let secs_in_day = now % 86_400;
    let (h, m, s) = (secs_in_day / 3600, (secs_in_day / 60) % 60, secs_in_day % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

// Howard Hinnant's date algorithm — converts days since 1970-01-01 to (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { (mp + 3) as u32 } else { (mp - 9) as u32 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
