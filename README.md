# Worklog — Upwork Screenshot Monitor

A lightweight **Rust** daemon that watches Upwork's time-tracking app and detects
when it captures a screenshot, in real time, exposing the events over a local
WebSocket API. On macOS it ships with **Worklog**, a small native control panel.

## Components

| Binary | What it is |
|---|---|
| `time-whisperer` | The headless daemon: watches Upwork logs, runs the localhost WebSocket server, exposes `/health`. Runs as a background service. |
| `worklog-gui` | macOS control-panel app ("Worklog"): turn monitoring on/off, see live status and whether a client is connected. Target-gated to macOS. |

## Features

- **Real-time screenshot detection** — watches Upwork log files for the "Electron Screensnap succeeded" signature
- **WebSocket API** — authenticated, token-based handshake; localhost only
- **Single-instance guard** — an advisory file lock means only one daemon ever runs; a second invocation exits cleanly
- **Health endpoint** — `GET /health` reports status, version, build commit, and the number of connected clients
- **Signed & notarized macOS installer** — Developer ID signed `.pkg`/`.dmg`, notarized by Apple (installs with no Gatekeeper warning)
- **Client-agnostic** — any client that completes the handshake counts; the daemon and GUI don't assume a specific client

## How It Works

The daemon tails the Upwork application logs and watches for entries containing
"Electron Screensnap succeeded", which mark a captured screenshot. Each event is
broadcast as a `screenshot_detected` message to authenticated WebSocket clients.

## Installation

### macOS (recommended)

Download the latest **`Worklog-<version>-macos-arm64.pkg`** from the
[Releases](https://github.com/Hyperbach/time-whisperer/releases) page and
double-click it. The installer wizard puts the app in `/Applications` and starts
the background monitor as a per-user LaunchAgent (starts at login, no Dock icon,
never appears in a screenshot). The `.pkg` is Developer ID signed and notarized,
so it installs without Gatekeeper warnings.

A drag-to-Applications **`.dmg`** is also published as an alternative.

### Linux

Download **`time-whisperer-linux-amd64`** from Releases:

```bash
chmod +x time-whisperer-linux-amd64
./time-whisperer-linux-amd64
```

(Wire it into a systemd user service yourself if you want it to start at login.)

### Build from source

Requires a stable Rust toolchain (`cargo`).

```bash
git clone https://github.com/Hyperbach/time-whisperer.git
cd time-whisperer
cargo build --release
# daemon:                 target/release/time-whisperer
# control panel (macOS):  target/release/worklog-gui
```

## Usage

```bash
# Run the daemon in the foreground
./time-whisperer

# Print version (and build commit / date)
./time-whisperer --version
```

On **macOS** the daemon can manage its own LaunchAgent:

```bash
time-whisperer install     # install the LaunchAgent and start it now
time-whisperer status      # show whether it's installed / running
time-whisperer uninstall   # stop and remove the LaunchAgent
```

Most users never touch the CLI — the **Worklog** app does install/start/stop and
shows live status (monitoring active, client connected, last screenshot seen).

The daemon will:
1. Acquire the single-instance lock (a second daemon exits immediately)
2. Load configuration from the platform-specific location
3. Bind the WebSocket server to the first free candidate port (starting at 8887)
4. Watch the Upwork log directory for `upwork.*.log` files
5. Broadcast `screenshot_detected` to authenticated clients

### WebSocket API

The daemon exposes a WebSocket endpoint at `/ws`:

**Authentication flow**
1. Client connects
2. Server sends `hello` with an authentication token
3. Client replies `hello_ack` echoing the token
4. Server confirms with `connected`

**Message types**
- `screenshot_detected` — a new screenshot was detected
- `ping` / `pong` — heartbeat
- `hello` / `hello_ack` / `connected` — handshake

**Health endpoint**
- `GET /health` → JSON: `status`, `version`, `commit`, `clients` (connected client count), `timestamp`

### Environment variables

- `UPWORK_LOGS_DIR` — override the Upwork log directory
- `WORKLOG_DAEMON` — (used by the GUI) path to the daemon binary to manage

## Configuration

Config file location:

- macOS: `~/Library/Application Support/TimeWhisperer/config.json`
- Linux: `~/.config/time-whisperer/config.json`
- Windows: `%APPDATA%\Local\TimeWhisperer\config.json`
- Development: `config.json` in the current directory

```json
{
  "debugMode": false,
  "logPath": "~/Library/Logs/TimeWhisperer/time-whisperer.log",
  "upworkLogsDir": "~/Library/Application Support/Upwork/Upwork/Logs",
  "webSocketPort": 8887
}
```

- `debugMode` — enable debug logging and the `/test/broadcast` endpoint
- `logPath` — application log file (supports `~`)
- `upworkLogsDir` — directory containing Upwork logs (auto-discovered on first run if empty)
- `webSocketPort` — preferred port (falls back to the candidate list)

### Default Upwork log locations

- macOS: `~/Library/Application Support/Upwork/Upwork/Logs`
- Linux: `~/.config/Upwork/Logs`
- Windows: `~/AppData/Roaming/Upwork/Logs`

## WebSocket port selection

The daemon binds the first free port from a deterministic candidate list,
starting with `8887` (49205, 49231, … and so on). Clients probe the same list to
find the running daemon. The list is shared between the daemon and clients so
connectivity is reliable across restarts.

### Security

The WebSocket server:
- binds only to localhost (`127.0.0.1`)
- requires token-based authentication
- is intended for local communication and must not be exposed to external networks

## Platforms

- **macOS (arm64)** — app + daemon; built, signed, and notarized in CI
- **Linux (amd64)** — daemon binary; built in CI
- **Windows** — the source has Windows path handling, but CI does not currently build or package a Windows release

## Development

```bash
cargo build --release       # build
cargo test                  # unit + integration tests
./run_integration_tests.sh  # integration tests serially (port-bound)
```

Releases are produced by `.github/workflows/release.yml` — a cargo-native
pipeline (no Nix). Pushing a `v*` tag builds, signs, notarizes, and publishes a
GitHub Release. See [VERIFICATION.md](VERIFICATION.md) to verify a download.

## License

MIT
