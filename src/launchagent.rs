//! macOS LaunchAgent management for the background daemon.
//!
//! The daemon has no UI of its own; it runs as a per-user LaunchAgent so it
//! starts at login and stays alive. This module writes/removes the plist and
//! loads/unloads it via `launchctl`.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default agent label (neutral on purpose). Override to rebrand.
pub const DEFAULT_LABEL: &str = "com.hyperbach.worklog";

/// Returns a human-readable reason if `program` sits in a location unsuitable
/// for a LaunchAgent — i.e. one that is read-only and/or disappears: a mounted
/// disk image (`/Volumes/...`) or a Gatekeeper-translocated path. `None` means
/// the location is fine to install from.
pub fn unsuitable_install_location(program: &Path) -> Option<String> {
    let s = program.to_string_lossy();
    if s.contains("/AppTranslocation/") {
        return Some(
            "The app is running from a temporary, read-only location (macOS Gatekeeper \
             translocation). Drag it into your Applications folder, then open it from there."
                .to_string(),
        );
    }
    if s.starts_with("/Volumes/") {
        return Some(
            "The app is running from a mounted disk image or external volume. Drag it into your \
             Applications folder, then open it from Applications."
                .to_string(),
        );
    }
    None
}

pub struct LaunchAgent {
    pub label: String,
}

impl LaunchAgent {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into() }
    }

    pub fn with_default_label() -> Self {
        Self::new(DEFAULT_LABEL)
    }

    /// `~/Library/LaunchAgents/<label>.plist`
    pub fn plist_path(&self) -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("Library/LaunchAgents")
            .join(format!("{}.plist", self.label))
    }

    /// Directory where the agent writes its stdout/stderr.
    fn log_dir() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("Library/Logs/Worklog")
    }

    /// Generates the launchd plist XML pointing at `program`. Pure + testable.
    /// `program` must be an absolute path (launchd does not expand `~`).
    pub fn plist_contents(&self, program: &Path) -> String {
        let log_dir = Self::log_dir();
        let out = log_dir.join("output.log");
        let err = log_dir.join("error.log");
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Background</string>
    <key>StandardOutPath</key>
    <string>{out}</string>
    <key>StandardErrorPath</key>
    <string>{err}</string>
</dict>
</plist>
"#,
            label = xml_escape(&self.label),
            program = xml_escape(&program.to_string_lossy()),
            out = xml_escape(&out.to_string_lossy()),
            err = xml_escape(&err.to_string_lossy()),
        )
    }

    pub fn is_installed(&self) -> bool {
        self.plist_path().exists()
    }

    /// Writes the plist (pointing at `program`) and loads it.
    pub fn install(&self, program: &Path) -> Result<()> {
        if !program.is_absolute() {
            return Err(anyhow!(
                "daemon path must be absolute for launchd: {}",
                program.display()
            ));
        }
        if let Some(reason) = unsuitable_install_location(program) {
            return Err(anyhow!(reason));
        }
        let plist = self.plist_path();
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::create_dir_all(Self::log_dir()).ok();
        std::fs::write(&plist, self.plist_contents(program))
            .with_context(|| format!("writing {}", plist.display()))?;

        // Reload cleanly: bootout an existing instance (ignore errors), then bootstrap.
        let _ = self.bootout();
        self.bootstrap(&plist)?;
        self.wait_until(true, std::time::Duration::from_secs(3));
        Ok(())
    }

    /// Unloads and removes the plist.
    pub fn uninstall(&self) -> Result<()> {
        let _ = self.bootout();
        self.wait_until(false, std::time::Duration::from_secs(3));
        let plist = self.plist_path();
        if plist.exists() {
            std::fs::remove_file(&plist)
                .with_context(|| format!("removing {}", plist.display()))?;
        }
        Ok(())
    }

    /// Loads the agent (resume) using the existing plist.
    pub fn start(&self) -> Result<()> {
        let plist = self.plist_path();
        if !plist.exists() {
            return Err(anyhow!("not installed (no plist at {})", plist.display()));
        }
        self.bootstrap(&plist)?;
        self.wait_until(true, std::time::Duration::from_secs(3));
        Ok(())
    }

    /// Unloads the agent (pause) but keeps the plist so it can be resumed.
    pub fn stop(&self) -> Result<()> {
        self.bootout()?;
        self.wait_until(false, std::time::Duration::from_secs(3));
        Ok(())
    }

    /// Polls `is_running` until it matches `want` or the timeout elapses.
    /// launchd load/unload is asynchronous, so this keeps status accurate.
    fn wait_until(&self, want: bool, timeout: std::time::Duration) {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.is_running() == want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }

    /// True if launchd currently has the agent loaded.
    pub fn is_running(&self) -> bool {
        // `launchctl print gui/<uid>/<label>` exits 0 when the service is loaded.
        let Some(uid) = current_uid() else { return false };
        Command::new("launchctl")
            .arg("print")
            .arg(format!("gui/{}/{}", uid, self.label))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn bootstrap(&self, plist: &Path) -> Result<()> {
        let uid = current_uid().ok_or_else(|| anyhow!("could not determine uid"))?;
        // Make sure it isn't marked disabled from a previous bootout.
        let _ = Command::new("launchctl")
            .args(["enable", &format!("gui/{}/{}", uid, self.label)])
            .output();
        let out = Command::new("launchctl")
            .arg("bootstrap")
            .arg(format!("gui/{}", uid))
            .arg(plist)
            .output()
            .context("running launchctl bootstrap")?;
        if out.status.success() {
            return Ok(());
        }
        // Fall back to the legacy loader (still works on current macOS).
        let legacy = Command::new("launchctl")
            .args(["load", "-w"])
            .arg(plist)
            .output()
            .context("running launchctl load -w")?;
        if legacy.status.success() {
            return Ok(());
        }
        Err(anyhow!(
            "launchctl bootstrap failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }

    fn bootout(&self) -> Result<()> {
        let uid = current_uid().ok_or_else(|| anyhow!("could not determine uid"))?;
        let out = Command::new("launchctl")
            .arg("bootout")
            .arg(format!("gui/{}/{}", uid, self.label))
            .output()
            .context("running launchctl bootout")?;
        if out.status.success() {
            return Ok(());
        }
        // Legacy fallback.
        let plist = self.plist_path();
        if plist.exists() {
            let _ = Command::new("launchctl")
                .args(["unload", "-w"])
                .arg(&plist)
                .output();
        }
        Ok(())
    }
}

fn current_uid() -> Option<u32> {
    let out = Command::new("id").arg("-u").output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_path_uses_label() {
        let a = LaunchAgent::new("com.example.test");
        let p = a.plist_path();
        assert!(p.ends_with("Library/LaunchAgents/com.example.test.plist"));
    }

    #[test]
    fn plist_contents_includes_program_and_label() {
        let a = LaunchAgent::new("com.example.test");
        let xml = a.plist_contents(Path::new("/Applications/Worklog.app/Contents/Resources/daemon"));
        assert!(xml.contains("<string>com.example.test</string>"));
        assert!(xml.contains("/Applications/Worklog.app/Contents/Resources/daemon"));
        assert!(xml.contains("<key>RunAtLoad</key>"));
        assert!(xml.contains("<key>KeepAlive</key>"));
        // Well-formed-ish: single dict, plist wrapper present.
        assert!(xml.starts_with("<?xml"));
        assert!(xml.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn plist_escapes_xml_metacharacters() {
        let a = LaunchAgent::new("com.a&b.test");
        let xml = a.plist_contents(Path::new("/tmp/x<y>z"));
        assert!(xml.contains("com.a&amp;b.test"));
        assert!(xml.contains("/tmp/x&lt;y&gt;z"));
        assert!(!xml.contains("x<y>z"));
    }

    #[test]
    fn install_rejects_relative_program_path() {
        let a = LaunchAgent::new("com.example.reltest");
        let err = a.install(Path::new("relative/daemon")).unwrap_err();
        assert!(format!("{}", err).contains("absolute"));
    }

    #[test]
    fn unsuitable_location_flags_dmg_and_translocation() {
        assert!(unsuitable_install_location(Path::new(
            "/Volumes/Worklog/Worklog.app/Contents/Resources/worklogd"
        ))
        .is_some());
        assert!(unsuitable_install_location(Path::new(
            "/private/var/folders/ab/AppTranslocation/XYZ/d/Worklog.app/Contents/Resources/worklogd"
        ))
        .is_some());
    }

    #[test]
    fn suitable_location_in_applications_is_ok() {
        assert!(unsuitable_install_location(Path::new(
            "/Applications/Worklog.app/Contents/Resources/worklogd"
        ))
        .is_none());
    }

    #[test]
    fn install_refuses_from_dmg() {
        let a = LaunchAgent::new("com.example.dmgtest");
        let err = a
            .install(Path::new("/Volumes/Worklog/Worklog.app/Contents/Resources/worklogd"))
            .unwrap_err();
        assert!(format!("{}", err).contains("Applications"));
    }
}
