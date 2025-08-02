# SneakTime - Upwork Screenshot Monitor

A lightweight Go daemon that monitors Upwork's time tracking application to detect screenshots in real-time and provides notifications via WebSocket API.

## Features

- **Real-time Screenshot Detection**: Monitors Upwork log files for "Electron Screensnap succeeded" events
- **WebSocket Server**: Provides authenticated WebSocket API for client connections
- **Token-based Authentication**: Secure handshake protocol for client connections
- **Multi-platform Support**: Runs on macOS, Linux, and Windows
- **Configuration Management**: JSON-based configuration with platform-specific defaults
- **Health Endpoint**: HTTP endpoint for service health monitoring

## How It Works

The tool monitors the Upwork application logs and watches for entries containing the signature "Electron Screensnap succeeded" that indicate when a screenshot is captured.

## Installation

### Prerequisites

- Go 1.21+ (for building from source)
- Upwork Desktop App installed and generating log files

### Installation

#### Option 1: Build from Source
```bash
# Clone the repository
git clone https://github.com/hyperbach/time-whisperer.git
cd time-whisperer

# Build
make build

# Run
./time-whisperer
```

#### Option 2: Install via Make
```bash
# Build and install
make install

# Set up as service (optional)
make darwin-service  # macOS
make linux-service   # Linux
```

## Usage

### Running the Application

```bash
# Run directly
./time-whisperer

# Check version
./time-whisperer -version
```

The daemon will:
1. Load configuration from platform-specific location
2. Start WebSocket server on first available candidate port (starting with 8887)
3. Begin monitoring Upwork log directory for new `upwork.*.log` files
4. Send `screenshot_detected` messages to authenticated WebSocket clients

### WebSocket API

The daemon exposes a WebSocket endpoint at `/ws` with the following message types:

**Authentication Flow:**
1. Client connects to WebSocket
2. Server sends `hello` with authentication token
3. Client responds with `hello_ack` containing the token
4. Server confirms with `connected` message

**Message Types:**
- `screenshot_detected`: Sent when new screenshot is detected
- `ping`/`pong`: Heartbeat messages
- `hello`/`hello_ack`/`connected`: Authentication handshake

**Health Endpoint:**
- `GET /health`: Returns JSON status with version and timestamp

### Logging

By default, logs are written to both console and file:
- **Default log path**: `~/time-whisperer.log` (configurable)
- **Log content**: Startup info, WebSocket connections, screenshot detections, errors
- **Debug mode**: Enables additional logging with file/line information

### Environment Variables

- `UPWORK_LOGS_DIR`: Override the default Upwork log directory path

Example:
```bash
UPWORK_LOGS_DIR=/custom/path/to/logs time-whisperer
```

## Uninstallation

```bash
# Stop service (if installed)
make uninstall

# Remove configuration files
rm -rf ~/.config/time-whisperer      # Linux
rm -rf ~/Library/Application\ Support/TimeWhisperer  # macOS
```

## Default Upwork Log Locations

The daemon monitors these directories by default:
- **macOS**: `~/Library/Application Support/Upwork/Upwork/Logs`
- **Linux**: `~/.config/Upwork/Logs`
- **Windows**: `~/AppData/Roaming/Upwork/Logs`

Override with `UPWORK_LOGS_DIR` environment variable.


## Development

### Building
```bash
# Build binary
make build

# Run tests
make test

# Run integration tests
./run_integration_tests.sh

# Clean build artifacts
make clean
```

### Available Make Targets
- `make build` - Build the binary
- `make run` - Run without installing
- `make test` - Run unit tests
- `make install` - Install to system
- `make uninstall` - Remove from system
- `make package` - Create distribution packages

## Configuration

SneakTime uses a configuration file located at:

- macOS: `~/Library/Application Support/TimeWhisperer/config.json`
- Linux: `~/.config/time-whisperer/config.json`
- Windows: `%APPDATA%\Local\TimeWhisperer\config.json`

### Configuration Options

You can customize the daemon behavior by editing the configuration file:

```json
{
  "debugMode": false,
  "logPath": "~/time-whisperer.log",
  "upworkLogsDir": "~/Library/Application Support/Upwork/Upwork/Logs",
  "webSocketPort": 8887
}
```

Configuration options:
- `debugMode`: Enable debug logging and `/test/broadcast` endpoint
- `logPath`: Path for application log file (supports `~` expansion)
- `upworkLogsDir`: Directory containing Upwork log files
- `webSocketPort`: Preferred WebSocket port (falls back to candidate list)

## WebSocket Port Selection

The Time Whisperer daemon uses a deterministic list of candidate ports for its WebSocket server. On startup, it will attempt to bind to the first available port from the following list:

```
8887, 49205, 49231, 49267, 49303, 49327,
49411, 49437, 49471, 49513, 49559, 49607,
49633, 49669, 49717, 49741, 49807, 49843,
49879, 49921, 49957, 50021, 50051, 50083,
50119, 50153, 50207, 50239, 50273, 50311,
50359, 50413, 50441, 50483, 50509, 50551,
50617, 50653, 50677, 50713, 50759, 50803,
50837, 50869, 50917, 50953, 51011, 51047,
51083, 51113
```

If all candidate ports are in use, the daemon will exit with a clear error message. Client applications can use the same list to probe for the running daemon, ensuring reliable connectivity.

### Security Note

**Security Note:** The WebSocket server:
- Binds only to localhost (127.0.0.1)
- Uses token-based authentication for connections
- Disables CORS checks for local communication
- Should not be exposed to external networks

## License

MIT