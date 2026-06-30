//! Worklog — control panel for the background screenshot monitor.
//!
//! This is the user-facing app. The actual monitoring is done by a separate,
//! UI-less daemon that runs as a LaunchAgent (so it never appears on screen and
//! never lands in a screenshot). This panel installs/removes that agent, shows
//! status, and reports whether a client is connected to the daemon.
//! Closing the window exits the panel; the daemon keeps running.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("worklog-gui is only supported on macOS.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            // Height auto-fits to content at runtime (see update()); start close to
            // the expected size. Min must stay below the fitted height or it would
            // clamp the shrink and leave dead space at the bottom.
            .with_inner_size([440.0, 370.0])
            .with_min_inner_size([440.0, 280.0])
            .with_resizable(false)
            .with_title("Worklog"),
        ..Default::default()
    };
    eframe::run_native(
        "Worklog",
        options,
        Box::new(|cc| {
            app::setup_fonts(&cc.egui_ctx);
            app::configure_style(&cc.egui_ctx);
            Ok(Box::new(app::ControlPanel::new()))
        }),
    )
}

#[cfg(target_os = "macos")]
mod app {
    use eframe::egui;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use time_whisperer::config;
    use time_whisperer::launchagent::LaunchAgent;
    use time_whisperer::monitor;
    use time_whisperer::ports::CANDIDATE_PORTS;

    // A small, friendly palette.
    const GREEN: egui::Color32 = egui::Color32::from_rgb(0x33, 0xA8, 0x55);
    const AMBER: egui::Color32 = egui::Color32::from_rgb(0xD9, 0x95, 0x28);
    const GREY: egui::Color32 = egui::Color32::from_rgb(0x9A, 0x9A, 0x9A);
    const RED: egui::Color32 = egui::Color32::from_rgb(0xCC, 0x44, 0x44);
    // macOS-style accent blue for the primary action button.
    const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x2F, 0x70, 0xE6);
    const WHITE: egui::Color32 = egui::Color32::WHITE;

    /// This app's own build identity. Version alone can't tell two builds apart
    /// (both can be "1.0.2"); the git commit is what actually distinguishes them.
    const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
    const APP_COMMIT: &str = match option_env!("GIT_COMMIT") {
        Some(c) => c,
        None => "unknown",
    };

    /// Live view of the daemon's WebSocket server, kept fresh by a background
    /// thread that polls the daemon's /health endpoint on localhost.
    #[derive(Default, Clone)]
    struct DaemonLink {
        /// The daemon's /health responded (the server is up).
        reachable: bool,
        /// Number of connected clients (authenticated WS clients).
        clients: usize,
        /// Version the daemon reports (empty if unknown).
        version: String,
        /// Git commit the daemon reports (empty for builds older than this field).
        commit: String,
    }

    /// Load the macOS system font (San Francisco) so the panel renders in the
    /// native UI typeface instead of egui's bundled Ubuntu-Light — the single
    /// biggest lever on "looks like a Mac app". egui's defaults stay as fallback
    /// for any glyph SF lacks. Silently no-ops if no system font can be read.
    pub fn setup_fonts(ctx: &egui::Context) {
        // SFNS.ttf is San Francisco (the system UI font); Arial is a guaranteed
        // plain-TTF fallback in case the SF variable font can't be parsed.
        const CANDIDATES: &[&str] = &[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ];
        for path in CANDIDATES {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("system".to_owned(), egui::FontData::from_owned(bytes));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "system".to_owned());
            ctx.set_fonts(fonts);
            return;
        }
    }

    /// Visual polish: rounder corners and roomier padding on top of egui's
    /// theme-correct widget colors (so buttons read as native push buttons in
    /// both light and dark mode). The accent-blue primary button provides the
    /// one strong call-to-action.
    pub fn configure_style(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        let rounding = egui::Rounding::same(7.0);
        let w = &mut style.visuals.widgets;
        w.noninteractive.rounding = rounding;
        w.inactive.rounding = rounding;
        w.hovered.rounding = rounding;
        w.active.rounding = rounding;
        w.open.rounding = rounding;
        ctx.set_style(style);
    }

    pub struct ControlPanel {
        agent: LaunchAgent,
        daemon: Option<PathBuf>,
        upwork_dir: Option<PathBuf>,
        location_problem: Option<String>,
        last_refresh: Instant,
        installed: bool,
        running: bool,
        last_shot: Option<String>,
        message: String,
        link: Arc<Mutex<DaemonLink>>,
        /// Last window height we requested, to avoid resending resize commands.
        fitted_h: f32,
    }

    impl ControlPanel {
        pub fn new() -> Self {
            let upwork_dir = resolve_upwork_dir();
            let daemon = resolve_daemon_path();
            let location_problem = daemon
                .as_ref()
                .and_then(|d| time_whisperer::launchagent::unsuitable_install_location(d));
            let link = Arc::new(Mutex::new(DaemonLink::default()));
            spawn_status_poller(link.clone());
            let mut me = Self {
                agent: LaunchAgent::with_default_label(),
                daemon,
                upwork_dir,
                location_problem,
                last_refresh: Instant::now() - Duration::from_secs(60),
                installed: false,
                running: false,
                last_shot: None,
                message: String::new(),
                link,
                fitted_h: 0.0,
            };
            me.refresh();
            me
        }

        fn refresh(&mut self) {
            self.installed = self.agent.is_installed();
            self.running = self.agent.is_running();
            self.last_shot = self.upwork_dir.as_ref().and_then(|d| {
                let latest = monitor::find_latest_log(d)?;
                let (ts, _) = monitor::last_screenshot_info(&latest).ok()?;
                ts.map(|t| t.format("%H:%M:%S").to_string())
            });
            self.last_refresh = Instant::now();
        }

        fn do_install(&mut self) {
            match &self.daemon {
                Some(d) => match self.agent.install(d) {
                    Ok(()) => self.message = "All set — Worklog is watching now.".into(),
                    Err(e) => self.message = format!("Couldn't turn it on: {e}"),
                },
                None => {
                    self.message = "Couldn't find the background helper next to this app.".into()
                }
            }
            self.refresh();
        }

        fn do_uninstall(&mut self) {
            match self.agent.uninstall() {
                Ok(()) => self.message = "Turned off. It won't start at login anymore.".into(),
                Err(e) => self.message = format!("Couldn't turn it off: {e}"),
            }
            self.refresh();
        }

        fn do_stop(&mut self) {
            match self.agent.stop() {
                Ok(()) => self.message = "Paused.".into(),
                Err(e) => self.message = format!("Couldn't pause: {e}"),
            }
            self.refresh();
        }

        fn do_start(&mut self) {
            match self.agent.start() {
                Ok(()) => self.message = "Back on — watching again.".into(),
                Err(e) => self.message = format!("Couldn't resume: {e}"),
            }
            self.refresh();
        }
    }

    impl eframe::App for ControlPanel {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            if self.last_refresh.elapsed() > Duration::from_secs(2) {
                self.refresh();
            }
            let link = self.link.lock().unwrap().clone();

            // Y of the bottom of the content, captured during layout so we can
            // auto-fit the window height to it (no dead space at the bottom).
            let mut content_bottom = 0.0_f32;

            // Generous, consistent margins so content doesn't hug the window edges.
            let panel_frame = egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin {
                left: 22.0,
                right: 22.0,
                top: 20.0,
                bottom: 16.0,
            });
            egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
                ui.heading("Worklog");
                ui.label(egui::RichText::new("Keeps an eye on your Upwork screenshots.").weak());
                ui.add_space(16.0);

                // Friendly, plain-language headline.
                let (headline, color, sub) = if !self.installed {
                    (
                        "Not set up yet",
                        GREY,
                        "Turn it on and Worklog will watch quietly in the background.",
                    )
                } else if self.running {
                    (
                        "You're covered",
                        GREEN,
                        "Worklog is watching for screenshots right now.",
                    )
                } else {
                    ("Paused", AMBER, "Worklog isn't watching at the moment.")
                };
                ui.horizontal(|ui| {
                    dot(ui, color, true, 13.0);
                    ui.add_space(7.0);
                    ui.label(egui::RichText::new(headline).size(19.0).strong().color(color));
                });
                ui.label(egui::RichText::new(sub).weak());

                ui.add_space(14.0);

                // Status card: full-width, subtly bordered, with breathing room
                // between rows.
                let card_border = if ui.visuals().dark_mode {
                    egui::Color32::from_gray(64)
                } else {
                    egui::Color32::from_gray(214)
                };
                egui::Frame::none()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(1.0, card_border))
                    .inner_margin(egui::Margin::same(14.0))
                    .rounding(10.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 10.0;
                        // Monitoring.
                        status_row(
                            ui,
                            self.running,
                            if self.running {
                                "Watching for screenshots"
                            } else {
                                "Monitoring is paused"
                            },
                            if self.running { GREEN } else { GREY },
                        );

                        // Client connection (live). Backend-agnostic: any client
                        // that completes the handshake counts — we don't name it.
                        let (client_on, client_text) = if !self.running {
                            (false, "Client connection — start Worklog first".to_string())
                        } else if link.reachable && link.clients > 0 {
                            let n = link.clients;
                            (
                                true,
                                if n == 1 {
                                    "Client connected".to_string()
                                } else {
                                    format!("{n} clients connected")
                                },
                            )
                        } else if link.reachable {
                            (false, "No client connected yet".to_string())
                        } else {
                            (false, "Connecting to the background helper…".to_string())
                        };
                        status_row(ui, client_on, &client_text, if client_on { GREEN } else { GREY });

                        // Last screenshot — green once we've actually seen one.
                        let seen = self.last_shot.is_some();
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            dot(ui, if seen { GREEN } else { GREY }, true, 11.0);
                            ui.add_space(6.0);
                            ui.label(format!(
                                "Last screenshot seen: {}",
                                self.last_shot.clone().unwrap_or_else(|| "none yet".into())
                            ));
                        });
                    });

                ui.add_space(16.0);

                // Guard: refuse to install from a DMG / read-only / translocated
                // location — the LaunchAgent path would break on eject/reboot.
                if let Some(problem) = self.location_problem.clone() {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(0x3a, 0x2a, 0x00))
                        .inner_margin(egui::Margin::same(10.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Move Worklog to Applications first")
                                    .strong()
                                    .color(egui::Color32::from_rgb(0xFF, 0xCC, 0x44)),
                            );
                            ui.add_space(3.0);
                            ui.label(problem);
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Drag Worklog into Applications, open it from there, then turn it on.",
                                )
                                .small()
                                .weak(),
                            );
                        });
                } else if !self.installed {
                    let mut clicked = false;
                    ui.vertical_centered(|ui| {
                        clicked = primary_button(ui, "Turn on Worklog", 190.0);
                    });
                    if clicked {
                        self.do_install();
                    }
                    ui.add_space(4.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "Runs automatically at login — no window, nothing in your Dock.",
                            )
                            .small()
                            .weak(),
                        );
                    });
                } else {
                    // Defer the click action until after the UI closures release
                    // their borrow of `self`.
                    let mut action: Option<fn(&mut ControlPanel)> = None;
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            let toggle = if self.running { "Pause" } else { "Resume" };
                            if primary_button(ui, toggle, 130.0) {
                                action = Some(if self.running {
                                    ControlPanel::do_stop
                                } else {
                                    ControlPanel::do_start
                                });
                            }
                            if secondary_button(ui, "Turn off", 130.0) {
                                action = Some(ControlPanel::do_uninstall);
                            }
                        });
                    });
                    if let Some(act) = action {
                        act(self);
                    }
                }

                if self.daemon.is_none() {
                    ui.add_space(8.0);
                    ui.colored_label(
                        RED,
                        "Background helper not found. Run from the installed app, or set WORKLOG_DAEMON.",
                    );
                }

                // The running background helper is a *different build* than this
                // app. Usual cause of a stale-looking status: an old daemon still
                // holding the port, reporting no connected client. Versions can match
                // ("1.0.2") yet the commit differs, so we compare commits.
                if link.reachable && APP_COMMIT != "unknown" && link.commit != APP_COMMIT {
                    let which = if link.commit.is_empty() {
                        "an older build".to_string()
                    } else {
                        format!("build {}", link.commit)
                    };
                    let ver = if link.version.is_empty() {
                        String::new()
                    } else {
                        format!(" (v{})", link.version)
                    };
                    ui.add_space(8.0);
                    ui.colored_label(
                        AMBER,
                        format!(
                            "Heads up: the background helper is {which}{ver}, not this app's build — \
                             restart Worklog (Turn off, then on) to sync them."
                        ),
                    );
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);
                // One-line slot: a transient action message when present, else the
                // ambient close hint. Same height either way, so the auto-fitted
                // window doesn't jump as messages come and go.
                if self.message.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "You can close this window — Worklog keeps running quietly in the background.",
                        )
                        .small()
                        .weak(),
                    );
                } else {
                    ui.label(egui::RichText::new(&self.message).small().italics().weak());
                }

                // Build footer. Shows the daemon's build too when it's connected
                // and matches, so support questions are answerable at a glance.
                ui.add_space(4.0);
                let footer = if link.reachable && !link.commit.is_empty() && link.commit == APP_COMMIT {
                    format!("Worklog {APP_VERSION} · {APP_COMMIT}  (app + helper)")
                } else {
                    format!("Worklog {APP_VERSION} · {APP_COMMIT}")
                };
                ui.label(egui::RichText::new(footer).small().weak());
                // cursor (not min_rect, which egui expands to fill) marks the true
                // bottom of laid-out content.
                content_bottom = ui.cursor().min.y;
            });

            // Auto-fit the window to the content: content bottom + a small, even
            // bottom margin. Keeps the bottom padding consistent and small in every
            // state (no dead space, no clipping), and re-fits if content changes.
            let desired_h = (content_bottom + 16.0).round();
            if (self.fitted_h - desired_h).abs() > 0.5 {
                self.fitted_h = desired_h;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(440.0, desired_h)));
            }

            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    /// Paint a status dot — filled or hollow ring — vertically centered on the
    /// current text row. Drawn with the painter (not a glyph) so it always shows;
    /// egui's bundled font lacks the ●/○ characters and rendered them as boxes.
    fn dot(ui: &mut egui::Ui, color: egui::Color32, filled: bool, diameter: f32) {
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, row_h), egui::Sense::hover());
        let c = rect.center();
        let r = diameter / 2.0;
        if filled {
            ui.painter().circle_filled(c, r, color);
        } else {
            ui.painter().circle_stroke(c, r - 0.5, egui::Stroke::new(1.5, color));
        }
    }

    /// One status line: a colored dot + label.
    fn status_row(ui: &mut egui::Ui, on: bool, text: &str, color: egui::Color32) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            dot(ui, color, on, 11.0);
            ui.add_space(6.0);
            ui.label(text);
        });
    }

    /// Accent-blue filled primary button with a Mac-like height.
    fn primary_button(ui: &mut egui::Ui, text: &str, min_w: f32) -> bool {
        ui.add(
            egui::Button::new(egui::RichText::new(text).color(WHITE).strong())
                .fill(ACCENT)
                .min_size(egui::vec2(min_w, 30.0)),
        )
        .clicked()
    }

    /// Plain secondary button (theme-correct fill) matching the primary's height.
    fn secondary_button(ui: &mut egui::Ui, text: &str, min_w: f32) -> bool {
        ui.add(egui::Button::new(text).min_size(egui::vec2(min_w, 30.0)))
            .clicked()
    }

    /// Background thread: poll the daemon's /health on localhost and publish a
    /// fresh [`DaemonLink`] so the UI can show live connection status without
    /// blocking the render thread.
    fn spawn_status_poller(link: Arc<Mutex<DaemonLink>>) {
        std::thread::spawn(move || {
            let mut cached_port: Option<u16> = None;
            loop {
                // Try the last-known-good port first, then scan the candidates.
                let ports = cached_port
                    .into_iter()
                    .chain(CANDIDATE_PORTS.iter().copied());
                let mut found: Option<(u16, Health)> = None;
                for port in ports {
                    if let Some(h) = probe_health(port) {
                        found = Some((port, h));
                        break;
                    }
                }
                if let Ok(mut l) = link.lock() {
                    match found {
                        Some((port, h)) => {
                            *l = DaemonLink {
                                reachable: true,
                                clients: h.clients,
                                version: h.version,
                                commit: h.commit,
                            };
                            cached_port = Some(port);
                        }
                        None => {
                            *l = DaemonLink::default();
                            cached_port = None;
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    /// What the daemon reports on /health.
    struct Health {
        clients: usize,
        version: String,
        commit: String,
    }

    /// GET /health from a localhost port; returns the daemon's reported state if
    /// it answers there. Raw TCP keeps the GUI dependency-free.
    fn probe_health(port: u16) -> Option<Health> {
        use std::io::{Read, Write};
        use std::net::{SocketAddr, TcpStream};
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(250)).ok()?;
        stream.set_read_timeout(Some(Duration::from_millis(600))).ok();
        stream.set_write_timeout(Some(Duration::from_millis(600))).ok();
        stream
            .write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        let body = text.split("\r\n\r\n").nth(1)?;
        let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
        if v.get("status").and_then(|s| s.as_str()) != Some("ok") {
            return None;
        }
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string()
        };
        Some(Health {
            // `clients`/`commit` are absent on builds predating those fields → treated as 0 / "".
            clients: v.get("clients").and_then(|c| c.as_u64()).unwrap_or(0) as usize,
            version: str_field("version"),
            commit: str_field("commit"),
        })
    }

    /// Locate the daemon binary to point the LaunchAgent at.
    fn resolve_daemon_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("WORKLOG_DAEMON") {
            let p = PathBuf::from(p);
            if p.exists() {
                return p.canonicalize().ok().or(Some(p));
            }
        }
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let candidates = [
            dir.join("../Resources/worklogd"), // app bundle: Contents/MacOS -> Contents/Resources
            dir.join("worklogd"),
            dir.join("time-whisperer"), // dev: sibling in target/<profile>
        ];
        for c in candidates {
            if c.exists() {
                return c.canonicalize().ok().or(Some(c));
            }
        }
        None
    }

    fn resolve_upwork_dir() -> Option<PathBuf> {
        let (cfg, _src) = config::load(&config::config_path()).ok()?;
        if cfg.upwork_logs_dir.is_empty() {
            return None;
        }
        Some(config::expand_path(&cfg.upwork_logs_dir))
    }
}
