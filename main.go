package main

import (
	"bufio"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"github.com/fsnotify/fsnotify"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"time"

	"crypto/rand"
	"encoding/hex"
	"github.com/gorilla/websocket"
)

// Version information
var (
	Version   = "1.0.0"
	GitCommit = "unknown"
	BuildDate = "unknown"
)

// Candidate ports for WebSocket server
var candidatePorts = []int{
	8887, 49205, 49231, 49267, 49303, 49327,
	49411, 49437, 49471, 49513, 49559, 49607,
	49633, 49669, 49717, 49741, 49807, 49843,
	49879, 49921, 49957, 50021, 50051, 50083,
	50119, 50153, 50207, 50239, 50273, 50311,
	50359, 50413, 50441, 50483, 50509, 50551,
	50617, 50653, 50677, 50713, 50759, 50803,
	50837, 50869, 50917, 50953, 51011, 51047,
	51083, 51113,
}

// one mutex per live connection – guarantees single-writer semantics
var writeMu sync.Map // key *websocket.Conn  ➜  *sync.Mutex

// WebSocket server
var (
	upgrader = websocket.Upgrader{
		ReadBufferSize:  1024,
		WriteBufferSize: 1024,
		CheckOrigin: func(r *http.Request) bool {
			return true // Allow any origin for Chrome extension
		},
	}

	// Connected clients – value == true  ➜  handshake completed
	clients     = make(map[*websocket.Conn]bool)
	clientsLock = sync.Mutex{}

	// Connections that need to be closed by their reader goroutine
	pendingCloses     = make(map[*websocket.Conn]bool)
	pendingClosesLock = sync.Mutex{}
)

// Message types for WebSocket communication
type WSMessage struct {
	Type    string      `json:"type"`
	Payload interface{} `json:"payload,omitempty"`
}

type Config struct {
	DebugMode     bool   `json:"debugMode"`
	LogPath       string `json:"logPath"`
	UpworkLogsDir string `json:"upworkLogsDir"`
	WebSocketPort int    `json:"webSocketPort"`
}

func DefaultConfig() Config {
	home, _ := os.UserHomeDir()
	return Config{
		DebugMode:     false,
		LogPath:       filepath.Join(home, "time-whisperer.log"),
		UpworkLogsDir: "", // Empty - will be discovered and filled in
		WebSocketPort: 8887,
	}
}

// validateConfig validates configuration fields and returns true if valid
func validateConfig(cfg Config) (bool, string) {
	if cfg.LogPath == "" {
		return false, "logPath cannot be empty in config"
	}

	if cfg.UpworkLogsDir == "" {
		return false, "upworkLogsDir cannot be empty in config"
	}

	// Validate WebSocket port
	if cfg.WebSocketPort <= 0 || cfg.WebSocketPort > 65535 {
		return false, fmt.Sprintf("invalid webSocketPort: %d (must be between 1-65535)", cfg.WebSocketPort)
	}

	// Expand tilde in log path if present
	if strings.HasPrefix(cfg.LogPath, "~") {
		home, err := os.UserHomeDir()
		if err != nil {
			return false, fmt.Sprintf("failed to expand ~ in logPath: %v", err)
		}
		expandedPath := filepath.Join(home, cfg.LogPath[1:])

		// Check if the parent directory exists or can be created
		parentDir := filepath.Dir(expandedPath)
		if _, err := os.Stat(parentDir); os.IsNotExist(err) {
			if err := os.MkdirAll(parentDir, 0755); err != nil {
				return false, fmt.Sprintf("cannot create log directory %s: %v", parentDir, err)
			}
		}
	}

	return true, ""
}

// expandPath expands ~ in path to user's home directory
func expandPath(path string) string {
	if path == "" {
		return path
	}

	if strings.HasPrefix(path, "~") {
		home, err := os.UserHomeDir()
		if err != nil {
			return path // Return original if there's an error
		}
		return filepath.Join(home, path[1:])
	}

	return path
}

func getDefaultLogDir() string {
	home, _ := os.UserHomeDir()
	switch runtime.GOOS {
	case "darwin":
		return filepath.Join(home, "Library", "Application Support", "Upwork", "Upwork", "Logs")
	case "windows":
		return filepath.Join(home, "AppData", "Roaming", "Upwork", "Logs")
	default:
		return filepath.Join(home, ".config", "Upwork", "Logs")
	}
}

// discoverUpworkLogsDir tries to find the actual Upwork logs directory
// by checking predefined locations and verifying they contain log files
func discoverUpworkLogsDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}

	var candidatePaths []string
	
	switch runtime.GOOS {
	case "darwin":
		candidatePaths = []string{
			filepath.Join(home, "Library", "Application Support", "Upwork", "Upwork", "Logs"),
		}
	case "windows":
		candidatePaths = []string{
			filepath.Join(home, "AppData", "Roaming", "Upwork", "Logs"),
		}
	default: // linux
		candidatePaths = []string{
			filepath.Join(home, ".config", "Upwork", "Logs"),
			filepath.Join(home, ".Upwork", "Upwork", "Logs"),
		}
	}

	for _, path := range candidatePaths {
		log.Printf("Checking for Upwork logs in: %s", path)
		
		// Check if directory exists
		if _, err := os.Stat(path); os.IsNotExist(err) {
			log.Printf("Directory does not exist: %s", path)
			continue
		}
		
		// Check if directory contains upwork log files
		pattern := filepath.Join(path, "upwork.*.log")
		matches, err := filepath.Glob(pattern)
		if err != nil {
			log.Printf("Error checking for log files in %s: %v", path, err)
			continue
		}
		
		if len(matches) > 0 {
			log.Printf("Found %d upwork log file(s) in: %s", len(matches), path)
			return path
		} else {
			log.Printf("No upwork log files found in: %s", path)
		}
	}
	
	// Return empty string if no valid location found
	log.Printf("No valid Upwork logs directory discovered")
	return ""
}

// ensureUpworkLogsDir checks if UpworkLogsDir is empty and discovers it if needed
func ensureUpworkLogsDir(cfg *Config) {
	if cfg.UpworkLogsDir == "" {
		log.Printf("UpworkLogsDir is empty, attempting to discover...")
		if discoveredPath := discoverUpworkLogsDir(); discoveredPath != "" {
			cfg.UpworkLogsDir = discoveredPath
			log.Printf("Discovered and set UpworkLogsDir: %s", discoveredPath)
		} else {
			// Fallback to platform default if discovery fails
			cfg.UpworkLogsDir = getDefaultLogDir()
			log.Printf("Discovery failed, using default UpworkLogsDir: %s", cfg.UpworkLogsDir)
		}
	}
}

// getBundledConfigPath returns the path to the OS-specific bundled default config
func getBundledConfigPath() string {
	// Get the executable path
	execPath, err := os.Executable()
	if err != nil {
		return ""
	}

	execDir := filepath.Dir(execPath)

	// Check different locations based on platform and packaging
	switch runtime.GOOS {
	case "darwin":
		// For macOS app bundle
		bundleConfig := filepath.Join(execDir, "..", "Resources", "default_config.json")
		if _, err := os.Stat(bundleConfig); err == nil {
			return bundleConfig
		}
		// For developer build
		return filepath.Join(execDir, "configs", "macos", "default_config.json")
	case "windows":
		return filepath.Join(execDir, "configs", "windows", "default_config.json")
	default: // linux and others
		return filepath.Join(execDir, "configs", "linux", "default_config.json")
	}
}

func getConfigPath() string {
	// 1. First check if there's a config file in the current directory (for development)
	currentDirConfig := "config.json"
	if _, err := os.Stat(currentDirConfig); err == nil {
		return currentDirConfig
	}

	// 2. Check if environment variable is set
	if p := os.Getenv("TIME_WHISPERER_CONFIG_PATH"); p != "" {
		return p
	}

	// 3. Use standard OS-specific locations
	home, _ := os.UserHomeDir()
	dir := filepath.Join(home, ".config", "time-whisperer")
	if runtime.GOOS == "darwin" {
		dir = filepath.Join(home, "Library", "Application Support", "TimeWhisperer")
	} else if runtime.GOOS == "windows" {
		dir = filepath.Join(home, "AppData", "Local", "TimeWhisperer")
	}
	return filepath.Join(dir, "config.json")
}

func loadConfig(p string) (Config, string, error) {
	// Try to load user config first
	configSource := ""
	if b, err := os.ReadFile(p); err == nil {
		var c Config
		if err := json.Unmarshal(b, &c); err != nil {
			// File exists but has invalid JSON - back it up before returning error
			// Use Windows-safe filename format (no colons)
			bakPath := fmt.Sprintf("%s.bak-%s", p, time.Now().UTC().Format("20060102T150405.000000000"))
			if renameErr := os.Rename(p, bakPath); renameErr != nil {
				return Config{}, "", fmt.Errorf("failed to back up invalid config: %w (original error: %v)", renameErr, err)
			}
			log.Printf("config: backed up invalid file to %s", bakPath)
			return Config{}, "", fmt.Errorf("invalid json: %w", err)
		}
		
		// Ensure UpworkLogsDir is discovered if empty
		originalDir := c.UpworkLogsDir
		ensureUpworkLogsDir(&c)
		if c.UpworkLogsDir != originalDir {
			// UpworkLogsDir was updated, save the improved config
			_ = saveConfig(c, p)
		}
		
		configSource = fmt.Sprintf("User config: %s", p)
		return c, configSource, nil
	} else if !os.IsNotExist(err) {
		// Return any I/O error other than file-not-found
		return Config{}, "", fmt.Errorf("failed to read config: %w", err)
	}

	// If user config doesn't exist, try bundled config
	bundledPath := getBundledConfigPath()
	if bundledPath != "" {
		if b, err := os.ReadFile(bundledPath); err == nil {
			var c Config
			if err := json.Unmarshal(b, &c); err == nil {
				// Ensure UpworkLogsDir is discovered if empty
				ensureUpworkLogsDir(&c)
				
				// Save a copy to user config path
				_ = saveConfig(c, p)
				configSource = fmt.Sprintf("Bundled config: %s", bundledPath)
				return c, configSource, nil
			}
		}
	}

	// Fallback to hardcoded defaults if no configs could be loaded
	c := DefaultConfig()
	
	// Ensure UpworkLogsDir is discovered if empty
	ensureUpworkLogsDir(&c)
	
	_ = saveConfig(c, p)
	configSource = "Default hardcoded config (no config file found)"
	return c, configSource, nil
}

func saveConfig(c Config, p string) error {
	_ = os.MkdirAll(filepath.Dir(p), 0o755)
	d, _ := json.MarshalIndent(c, "", "  ")
	return os.WriteFile(p, d, 0o644)
}

func initLog(path string, debug bool) *os.File {
	// Expand tilde in path if present
	path = expandPath(path)

	if path == "" {
		log.SetOutput(os.Stdout)
		if debug {
			log.SetFlags(log.Ldate | log.Ltime | log.Lshortfile)
		} else {
			log.SetFlags(log.Ldate | log.Ltime)
		}
		return nil
	}
	_ = os.MkdirAll(filepath.Dir(path), 0o755)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		log.SetOutput(os.Stdout)
		return nil
	}
	log.SetOutput(io.MultiWriter(os.Stdout, f))
	if debug {
		log.SetFlags(log.Ldate | log.Ltime | log.Lshortfile)
	} else {
		log.SetFlags(log.Ldate | log.Ltime)
	}
	return f
}

// startWebSocketServer starts the WebSocket server on the first available port
func startWebSocketServer(ctx context.Context, mux *http.ServeMux) (int, error) {
	var lastErr error
	for _, port := range candidatePorts {
		log.Printf("Trying port %d for WebSocket server", port)
		server := &http.Server{
			Addr:    fmt.Sprintf(":%d", port),
			Handler: mux,
		}

		ln, err := net.Listen("tcp", server.Addr)
		if err != nil {
			log.Printf("Failed to bind port %d: %v", port, err)
			if strings.Contains(err.Error(), "address already in use") {
				lastErr = err
				continue // try next port
			}
			return 0, err // fatal error
		}

		go func() {
			if err := server.Serve(ln); err != nil && err != http.ErrServerClosed {
				log.Printf("WebSocket server error: %v", err)
			}
		}()

		go func() {
			<-ctx.Done()
			log.Println("Shutting down WebSocket server")
			server.Shutdown(context.Background())
		}()

		log.Printf("Using port %d for WebSocket server", port)
		return port, nil
	}
	return 0, fmt.Errorf("no free candidate port: %v", lastErr)
}

// handleWebSocket upgrades the HTTP request and starts the handshake.
func handleWebSocket(w http.ResponseWriter, r *http.Request) {
	log.Printf("New WebSocket connection attempt from %s", r.RemoteAddr)
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("upgrade failed: %v", err)
		return
	}

	// Mark as "not yet authenticated"
	clientsLock.Lock()
	clients[conn] = false
	clientsLock.Unlock()

	// Generate 16-byte random token
	tokenBytes := make([]byte, 16)
	if _, err := rand.Read(tokenBytes); err != nil {
		log.Printf("rng: %v", err)
		conn.Close()
		return
	}
	token := hex.EncodeToString(tokenBytes)
	log.Printf("Generated authentication token for client %s", conn.RemoteAddr())

	// Send challenge
	sendMessage(conn, WSMessage{
		Type: "hello",
		Payload: map[string]any{
			"token":   token,
			"version": Version,
		},
	})
	log.Printf("Sent hello challenge to client %s", conn.RemoteAddr())

	// Abort if the extension never answers
	timer := time.AfterFunc(5*time.Second, func() {
		log.Printf("handshake timeout %s", conn.RemoteAddr())
		// Send close message
		conn.WriteMessage(websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.ClosePolicyViolation, "handshake timeout"))

		// Clean up resources - avoid leaking map entries and mutexes
		clientsLock.Lock()
		delete(clients, conn)
		clientsLock.Unlock()

		// Also clean up the write mutex
		writeMu.Delete(conn)

		// Finally close the connection
		conn.Close()

		log.Printf("Cleaned up resources for timed out connection %s", conn.RemoteAddr())
	})

	// Start reader
	go handleWebSocketMessages(conn, token, timer)
}

// handleWebSocketMessages handles incoming messages from WebSocket clients
func handleWebSocketMessages(conn *websocket.Conn, expectedToken string, t *time.Timer) {
	defer func() {
		clientsLock.Lock()
		delete(clients, conn)
		clientCount := len(clients)
		clientsLock.Unlock()

		// Clean up the write mutex to prevent resource leaks
		writeMu.Delete(conn)

		// Also clean up from pendingCloses if it's there
		pendingClosesLock.Lock()
		delete(pendingCloses, conn)
		pendingClosesLock.Unlock()

		conn.Close()
		log.Printf("WS client disconnected %s (remaining clients: %d)", conn.RemoteAddr(), clientCount)
	}()

	authed := false

	for {
		// Check if this connection was marked for closing by sendMessage
		pendingClosesLock.Lock()
		shouldClose := pendingCloses[conn]
		pendingClosesLock.Unlock()

		if shouldClose {
			return // This will trigger the deferred cleanup and close
		}

		var msg WSMessage
		// Set a read deadline to ensure we don't block forever if the connection is silently dropped
		conn.SetReadDeadline(time.Now().Add(90 * time.Second))
		if err := conn.ReadJSON(&msg); err != nil {
			if websocket.IsUnexpectedCloseError(err,
				websocket.CloseGoingAway, websocket.CloseAbnormalClosure) {
				log.Printf("ws read: %v", err)
			}
			return
		}
		conn.SetReadDeadline(time.Time{}) // Reset deadline after successful read

		// ────────────────── handshake path ──────────────────
		if !authed {
			if msg.Type != "hello_ack" {
				log.Printf("Expected hello_ack but got %s from %s", msg.Type, conn.RemoteAddr())
				conn.Close()
				return
			}
			pl, _ := msg.Payload.(map[string]any)
			if tok, ok := pl["token"].(string); !ok || tok != expectedToken {
				log.Printf("Invalid token from client %s", conn.RemoteAddr())
				conn.Close()
				return
			}

			// Success 🎉
			t.Stop()
			authed = true
			clientsLock.Lock()
			clients[conn] = true
			clientCount := 0
			for _, auth := range clients {
				if auth {
					clientCount++
				}
			}
			clientsLock.Unlock()
			log.Printf("Authentication successful for client %s (authenticated clients: %d)", conn.RemoteAddr(), clientCount)

			sendMessage(conn, WSMessage{
				Type: "connected",
				Payload: map[string]any{
					"timestamp": time.Now().Format(time.RFC3339),
				},
			})
			continue
		}

		// ─────────── normal, post-handshake messages ───────────
		switch msg.Type {
		case "ping":
			sendMessage(conn, WSMessage{
				Type: "pong",
				Payload: map[string]any{
					"timestamp": time.Now().Format(time.RFC3339),
				},
			})
		default:
			log.Printf("unknown msg %q from %s", msg.Type, conn.RemoteAddr())
		}
	}
}

// sendMessage writes a JSON frame to the client, serialising with any
// concurrent broadcast via a per-connection mutex.
func sendMessage(conn *websocket.Conn, msg WSMessage) {
	muIface, _ := writeMu.LoadOrStore(conn, &sync.Mutex{})
	mu := muIface.(*sync.Mutex)

	mu.Lock()
	err := conn.WriteJSON(msg)
	mu.Unlock()

	if err != nil {
		log.Printf("write to %s failed: %v", conn.RemoteAddr(), err)

		// Signal to the reader goroutine that this connection should be closed
		pendingClosesLock.Lock()
		pendingCloses[conn] = true
		pendingClosesLock.Unlock()

		// Remove from clients map but let the reader goroutine handle the actual close
		clientsLock.Lock()
		delete(clients, conn)
		writeMu.Delete(conn)
		clientsLock.Unlock()
	}
}

// broadcastMessage sends one JSON frame to every authenticated client,
// re-using the same safe writer used by sendMessage.
func broadcastMessage(msg WSMessage) {
	clientsLock.Lock()
	// Snapshot the targets while holding the lock.
	targets := make([]*websocket.Conn, 0, len(clients))
	for c, ok := range clients {
		if ok { // only fully authenticated
			targets = append(targets, c)
		}
	}
	clientsLock.Unlock()

	log.Printf("Broadcasting %q to %d client(s)", msg.Type, len(targets))

	for _, c := range targets {
		sendMessage(c, msg) // already mutex-protected
	}
}

// notifyScreenshot sends a screenshot detection notification to all connected clients
func notifyScreenshot(timestamp time.Time) {
	broadcastMessage(WSMessage{
		Type: "screenshot_detected",
		Payload: map[string]any{
			"timestamp": timestamp.Format("15:04:05"),
			"time":      timestamp.Format(time.RFC3339),
		},
	})
}

const screenshotPattern = "Electron Screensnap succeeded"

// getAllScreenshotTimestamps extracts screenshot timestamps from log file
func getAllScreenshotTimestamps(logFile string) []string {
	f, err := os.Open(logFile)
	if err != nil {
		return nil
	}
	defer f.Close()

	var timestamps []string
	sc := bufio.NewScanner(f)

	for sc.Scan() {
		line := sc.Text()
		if strings.Contains(line, screenshotPattern) {
			if ts := parseTS(line); !ts.IsZero() {
				timestamps = append(timestamps, ts.Format(time.RFC3339Nano))
			}
		}
	}
	return timestamps
}

// lastScreenshotInfo returns the most‑recent screenshot timestamp and the full log line
func lastScreenshotInfo(logFile string) (time.Time, string, error) {
	f, err := os.Open(logFile)
	if err != nil {
		return time.Time{}, "", err
	}
	defer f.Close()

	var latest time.Time
	var latestLine string
	sc := bufio.NewScanner(f)

	for sc.Scan() {
		line := sc.Text()
		if strings.Contains(line, screenshotPattern) {
			if ts := parseTS(line); !ts.IsZero() && ts.After(latest) {
				latest = ts
				latestLine = line
			}
		}
	}
	return latest, latestLine, sc.Err()
}

// findLatestLog focuses only on upwork..*.log files which contain screenshot info
func findLatestLog(dir string) string {
	pattern := filepath.Join(expandPath(dir), "upwork.*.log")

	files, _ := filepath.Glob(pattern)
	if len(files) == 0 {
		return ""
	}

	var latest string
	var latestTime time.Time
	for _, file := range files {
		if fi, err := os.Stat(file); err == nil && fi.ModTime().After(latestTime) {
			latest = file
			latestTime = fi.ModTime()
		}
	}
	return latest
}

// parseTS returns the timestamp that sits inside the first [...] pair.
// It accepts Upwork's "2025-05-12T11:26:23.318" (no zone) as well as the
// full RFC 3339 variants. On failure it returns time.Time{}.
func parseTS(line string) time.Time {
	start := strings.IndexByte(line, '[')
	if start == -1 {
		return time.Time{}
	}
	end := strings.IndexByte(line[start+1:], ']')
	if end == -1 {
		return time.Time{}
	}
	s := line[start+1 : start+1+end]

	// Fast-path: exact "YYYY-MM-DDThh:mm:ss.mmm"
	if len(s) == len("2006-01-02T15:04:05.000") {
		if t, err := time.ParseInLocation("2006-01-02T15:04:05.000", s, time.Local); err == nil {
			return t
		}
	}

	// Fallback to the full RFC3339 layouts (with or without nanoseconds)
	if t, err := time.Parse(time.RFC3339Nano, s); err == nil {
		return t
	}
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t
	}

	return time.Time{}
}

// runMonitor tails the newest Upwork log in dir and emits events whenever a
// new “Electron Screensnap succeeded” line appears.
//
// This function is designed to be robust against several log rotation schemes,
// including rename, copy-truncate, and a daily forced check at midnight.
func runMonitor(ctx context.Context, dir string) {
	w, err := fsnotify.NewWatcher()
	if err != nil {
		log.Fatalf("fsnotify: %v", err)
	}
	defer w.Close()

	const keep = 48 * time.Hour // dedup window
	type entry struct{ t time.Time }
	seen := make(map[string]entry) // key = RFC3339Nano timestamp
	var lastSeen time.Time         // strictly-monotone guard

	expandedDir := expandPath(dir)
	if err := w.Add(expandedDir); err != nil {
		log.Fatalf("watch %s: %v", expandedDir, err)
	}

	// ───────────────────────────────── current tail state ─────────────────────────────
	var (
		current *os.File
		rdr     *bufio.Reader
	)

	// openCurrent is a safe, idempotent function to find and open the latest log.
	openCurrent := func() error {
		fname := findLatestLog(expandedDir)
		if fname == "" {
			return nil // No log file found, nothing to do.
		}
		if current != nil && current.Name() == fname {
			return nil // Already tailing the newest file.
		}

		// If we have a file open, close it and clear the state immediately.
		// This is the critical fix to prevent using a closed file descriptor
		// if the subsequent os.Open call fails.
		if current != nil {
			current.Close()
			current = nil
			rdr = nil
		}

		f, err := os.Open(fname)
		if err != nil {
			return err
		}

		if _, err := f.Seek(0, io.SeekEnd); err != nil {
			f.Close()
			return err
		}
		current, rdr = f, bufio.NewReaderSize(f, 64*1024)
		log.Printf("Monitoring log file: %s", fname)
		return nil
	}

	// Perform initial opening of the log file.
	if err := openCurrent(); err != nil {
		log.Printf("Initial log open failed, will retry: %v", err)
	}

	prune := func(now time.Time) {
		for k, v := range seen {
			if now.Sub(v.t) > keep {
				delete(seen, k)
			}
		}
	}

	// ───────────────────────────────── midnight reset timer ──────────────────────────
	nextMidnight := func() time.Time {
		t := time.Now()
		year, month, day := t.Date()
		// time.Date correctly handles day+1 rolling over the month/year.
		return time.Date(year, month, day+1, 0, 0, 0, 0, t.Location())
	}

	// Given the current time (Fri, Aug 1 2025 00:32 CEST), the next midnight is
	// Sat, Aug 2 2025 00:00 CEST. The timer will be set for ~23.5 hours.
	duration := time.Until(nextMidnight())
	timer := time.NewTimer(duration)
	log.Printf("Scheduled next midnight log file check in %v", duration.Round(time.Second))
	defer timer.Stop()

	// ───────────────────────────────── event loop ────────────────────────────────────
	for {
		select {
		case <-ctx.Done():
			if current != nil {
				current.Close()
			}
			return

		case <-timer.C:
			log.Printf("Midnight: Forcing a re-scan of the log directory.")
			if err := openCurrent(); err != nil {
				log.Printf("Error during midnight log re-scan: %v", err)
			}
			// Use the defensive stop-then-reset pattern.
			timer.Reset(time.Until(nextMidnight()))

		case ev := <-w.Events:
			// Watch for Create, Rename, and Remove to robustly handle rotation.
			if ev.Op&(fsnotify.Create|fsnotify.Rename|fsnotify.Remove) != 0 &&
				strings.HasPrefix(filepath.Base(ev.Name), "upwork.") {
				log.Printf("Filesystem event (%s on %s) triggered log re-scan.", ev.Op, filepath.Base(ev.Name))
				if err := openCurrent(); err != nil {
					log.Printf("Error opening new log file after %s event: %v", ev.Op, err)
				}
			}

			if current == nil || ev.Name != current.Name() || ev.Op&fsnotify.Write == 0 {
				continue
			}

			for {
				if off, _ := current.Seek(0, io.SeekCurrent); off > 0 {
					if fi, err := current.Stat(); err == nil && fi.Size() < off {
						if _, err := current.Seek(0, io.SeekStart); err == nil {
							rdr.Reset(current)
						}
						break
					}
				}

				line, err := rdr.ReadString('\n')
				if err != nil {
					if err != io.EOF {
						log.Printf("Read error on %s: %v", current.Name(), err)
						// Attempt to recover by reopening.
						if openErr := openCurrent(); openErr != nil {
							log.Printf("Error re-opening log after read error: %v", openErr)
						}
					}
					break
				}

				if !strings.Contains(line, screenshotPattern) {
					continue
				}

				ts := parseTS(line)
				if ts.IsZero() || !ts.After(lastSeen) {
					continue
				}

				key := ts.Format(time.RFC3339Nano)
				if _, dup := seen[key]; dup {
					continue
				}
				seen[key] = entry{t: ts}
				lastSeen = ts
				prune(ts)

				log.Printf("Screenshot detected at %s", ts.Format("15:04:05"))
				notifyScreenshot(ts)
			}

		case err := <-w.Errors:
			log.Printf("watch error: %v", err)
		}
	}
}

func main() {
	log.SetOutput(os.Stdout)

	showVer := flag.Bool("version", false, "print version and exit")
	flag.Parse()
	if *showVer {
		fmt.Printf("SneakTime %s (%s, %s)\n", Version, GitCommit, BuildDate)
		return
	}

	cfgPath := getConfigPath()
	cfg, configSource, err := loadConfig(cfgPath)
	if err != nil {
		log.Fatalf("Unable to read config %s: %v", cfgPath, err)
	}

	// Log which config file is being used (convert to absolute path for clarity)
	if absCfgPath, err := filepath.Abs(cfgPath); err == nil {
		log.Printf("Config file path: %s", absCfgPath)
	} else {
		log.Printf("Config file path: %s", cfgPath)
	}

	// Log full config
	log.Printf("Loaded config: %+v", cfg)

	// Validate the config
	if valid, errMsg := validateConfig(cfg); !valid {
		fmt.Fprintf(os.Stderr, "Configuration error: %s\n", errMsg)
		fmt.Fprintf(os.Stderr, "Config source: %s\n", configSource)
		fmt.Fprintf(os.Stderr, "Please fix your configuration and try again.\n")

		// Dump config for debugging
		jsonBytes, _ := json.MarshalIndent(cfg, "", "  ")
		fmt.Fprintf(os.Stderr, "Current config content:\n%s\n", string(jsonBytes))

		os.Exit(1)
	}

	logFile := initLog(cfg.LogPath, cfg.DebugMode)
	if logFile != nil {
		defer logFile.Close()
	}

	// Set up context with cancellation for clean shutdown
	ctx, stop := context.WithCancel(context.Background())
	defer stop()

	// Log version information on startup
	log.Printf("SneakTime %s (commit %s, built %s)", Version, GitCommit, BuildDate)

	// Log the config source
	log.Printf("Using configuration from: %s", configSource)

	log.Printf("Logs are also written to %s", cfg.LogPath)

	// Register handlers on the default mux
	http.HandleFunc("/ws", handleWebSocket)
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(map[string]any{
			"status":    "ok",
			"version":   Version,
			"timestamp": time.Now().Format(time.RFC3339),
		})
	})

	// Always register /test/broadcast in test mode for integration tests
	if os.Getenv("GO_TEST") == "1" || cfg.DebugMode {
		log.Printf("Debug mode: registering /test/broadcast handler")
		http.HandleFunc("/test/broadcast", func(w http.ResponseWriter, r *http.Request) {
			if r.Method != http.MethodPost {
				w.WriteHeader(http.StatusMethodNotAllowed)
				return
			}
			var msg WSMessage
			if err := json.NewDecoder(r.Body).Decode(&msg); err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			broadcastMessage(msg)
			w.WriteHeader(http.StatusOK)
		})
	}

	// Start WebSocket server with deterministic port probing
	chosenPort, err := startWebSocketServer(ctx, http.DefaultServeMux)
	if err != nil {
		log.Fatalf("Failed to start WebSocket server: %v", err)
	}
	log.Printf("WebSocket server started on port %d", chosenPort)

	// Set up Upwork log monitoring
	dir := cfg.UpworkLogsDir
	if env := os.Getenv("UPWORK_LOGS_DIR"); env != "" {
		dir = env
	}
	if dir == "" {
		log.Fatalln("cannot determine Upwork log directory")
	}

	log.Printf("Monitoring Upwork logs in %s", dir)

	go runMonitor(ctx, dir)

	// Set up signal handling for graceful shutdown
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig

	log.Println("shutting down")
	stop() // Stop all background goroutines

	// Give time for graceful shutdown
	time.Sleep(500 * time.Millisecond)
}
