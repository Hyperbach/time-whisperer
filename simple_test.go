package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"github.com/gorilla/websocket"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// -----------------------------------------------------------------------------
// helpers ---------------------------------------------------------------------

// safeBuf provides a thread-safe wrapper around bytes.Buffer
type safeBuf struct {
	buf bytes.Buffer
	mu  sync.RWMutex
}

func (b *safeBuf) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *safeBuf) String() string {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return b.buf.String()
}

// env bundles paths / cmd handles for one test run
type env struct {
	tmpDir   string
	logDir   string
	logFile  string
	confPath string
	port     int
	cmd      *exec.Cmd
	buf      *safeBuf
}

func newEnv(t *testing.T) *env {
	t.Helper()

	tmp, err := os.MkdirTemp("", "tw-*")
	if err != nil {
		t.Fatalf("tmpdir: %v", err)
	}

	// pick a free port from candidatePorts
	var port int
	for _, p := range candidatePorts {
		ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", p))
		if err == nil {
			port = p
			ln.Close()
			break
		}
	}
	if port == 0 {
		os.RemoveAll(tmp)
		t.Fatalf("no free candidate port found")
	}

	logDir := filepath.Join(tmp, "upwork", "logs")
	if err := os.MkdirAll(logDir, 0o755); err != nil {
		os.RemoveAll(tmp)
		t.Fatalf("logdir: %v", err)
	}
	logFile := filepath.Join(logDir, fmt.Sprintf("upwork..%s.log",
		time.Now().Format("20060102")))

	// *** key fix #1: create an EMPTY log file before the app starts
	if err := os.WriteFile(logFile, nil, 0o644); err != nil {
		os.RemoveAll(tmp)
		t.Fatalf("logfile create: %v", err)
	}

	// write a minimal config (port is ignored by daemon, but included for completeness)
	cfg := struct {
		DebugMode     bool   `json:"debugMode"`
		LogPath       string `json:"logPath"`
		WebSocketPort int    `json:"webSocketPort"`
	}{
		DebugMode:     true,
		LogPath:       "",
		WebSocketPort: port,
	}
	js, _ := json.Marshal(cfg)
	confPath := filepath.Join(tmp, "config.json")
	if err := os.WriteFile(confPath, js, 0o644); err != nil {
		os.RemoveAll(tmp)
		t.Fatalf("config write: %v", err)
	}

	return &env{
		tmpDir:   tmp,
		logDir:   logDir,
		logFile:  logFile,
		confPath: confPath,
		port:     port,
	}
}

func (e *env) start(t *testing.T) {
	t.Helper()

	bin := "./time-whisperer"
	if _, err := os.Stat(bin); err != nil {
		t.Fatalf("binary not built at %s", bin)
	}

	e.buf = &safeBuf{}
	e.cmd = exec.Command(bin)
	e.cmd.Env = append(os.Environ(),
		"UPWORK_LOGS_DIR="+e.logDir,
		"TIME_WHISPERER_CONFIG_PATH="+e.confPath,
		"GO_TEST=1", // Force test mode in daemon
	)
	e.cmd.Stdout = io.MultiWriter(os.Stdout, e.buf)
	e.cmd.Stderr = io.MultiWriter(os.Stderr, e.buf)

	if err := e.cmd.Start(); err != nil {
		t.Fatalf("start: %v", err)
	}

	time.Sleep(1 * time.Second) // allow watcher & server to come up
}

func (e *env) stop() {
	if e.cmd != nil && e.cmd.Process != nil {
		_ = e.cmd.Process.Signal(os.Interrupt)
		e.cmd.Wait()
	}
	os.RemoveAll(e.tmpDir)
}

// append one screenshot line and return the ts string
func (e *env) addShot(t *testing.T) string {
	t.Helper()
	ts := time.Now().Format("2006-01-02T15:04:05.000")
	line := fmt.Sprintf("[%s] [INFO] main.shell.os_services - Electron Screensnap succeeded.\n", ts)
	f, err := os.OpenFile(e.logFile, os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		t.Fatalf("open log: %v", err)
	}
	_, _ = f.WriteString(line)
	f.Close()
	return ts
}

// count occurrences of either old *or* new detection wording
func (e *env) countDetections() int {
	out := e.buf.String()
	return strings.Count(out, "Screenshot detected at") +
		strings.Count(out, "New screenshot detected at")
}

// -----------------------------------------------------------------------------
// actual tests ----------------------------------------------------------------

func TestBasicScreenshotDetection(t *testing.T) {
	e := newEnv(t)
	defer e.stop()

	e.start(t)
	e.addShot(t)
	time.Sleep(2 * time.Second)

	if n := e.countDetections(); n == 0 {
		t.Fatalf("screenshot not detected:\n%s", e.buf.String())
	}
}

func TestTwoScreenshotDetection(t *testing.T) {
	e := newEnv(t)
	defer e.stop()

	e.start(t)
	e.addShot(t)
	time.Sleep(2 * time.Second)
	e.addShot(t)
	time.Sleep(2 * time.Second)

	if n := e.countDetections(); n < 2 {
		t.Fatalf("want ≥2 detections, got %d:\n%s", n, e.buf.String())
	}
}

func TestThreeScreenshotDetection(t *testing.T) {
	e := newEnv(t)
	defer e.stop()

	e.start(t)
	for i := 0; i < 3; i++ {
		e.addShot(t)
		time.Sleep(2 * time.Second)
	}
	if n := e.countDetections(); n < 3 {
		t.Fatalf("want ≥3 detections, got %d:\n%s", n, e.buf.String())
	}
}

func TestExistingScreenshotsNotReported(t *testing.T) {
	e := newEnv(t)
	// two shots *before* the app starts
	e.addShot(t)
	e.addShot(t)

	e.start(t)
	time.Sleep(3 * time.Second)

	if n := e.countDetections(); n != 0 {
		t.Fatalf("existing screenshots reported as new:\n%s", e.buf.String())
	}
	e.stop()
}

func TestScreenshotNotReportedTwice(t *testing.T) {
	e := newEnv(t)
	e.start(t)
	e.addShot(t)
	time.Sleep(3 * time.Second)

	if n := e.countDetections(); n == 0 {
		t.Fatalf("first detection missing")
	}
	e.stop()

	// restart app with same log file
	e2 := &env{
		tmpDir:   e.tmpDir,
		logDir:   e.logDir,
		logFile:  e.logFile,
		confPath: e.confPath,
		port:     e.port,
	}
	defer e2.stop()

	e2.start(t)
	time.Sleep(3 * time.Second)

	if n := e2.countDetections(); n != 0 {
		t.Fatalf("duplicate detection after restart:\n%s", e2.buf.String())
	}
}

func TestWebSocketHandshake_UnauthenticatedTimesOut(t *testing.T) {
	e := newEnv(t)
	defer e.stop()

	e.start(t)
	defer e.stop()

	u := url.URL{Scheme: "ws", Host: fmt.Sprintf("127.0.0.1:%d", e.port), Path: "/ws"}
	c, _, err := websocket.DefaultDialer.Dial(u.String(), nil)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	defer c.Close()

	// Wait for hello
	var msg map[string]interface{}
	if err := c.ReadJSON(&msg); err != nil {
		t.Fatalf("read hello: %v", err)
	}
	if msg["type"] != "hello" {
		t.Fatalf("expected hello, got %v", msg["type"])
	}

	// Do NOT respond. Wait for close.
	c.SetReadDeadline(time.Now().Add(7 * time.Second))
	_, _, err = c.ReadMessage()
	if err == nil {
		t.Fatalf("expected close due to timeout, got no error")
	}
	if !websocket.IsCloseError(err, websocket.ClosePolicyViolation) {
		t.Fatalf("expected policy violation close, got: %v", err)
	}
}

func TestWebSocketHandshake_AuthenticatedReceivesBroadcast(t *testing.T) {
	e := newEnv(t)
	defer e.stop()

	e.start(t)

	u := url.URL{Scheme: "ws", Host: fmt.Sprintf("localhost:%d", e.port), Path: "/ws"}
	c, _, err := websocket.DefaultDialer.Dial(u.String(), nil)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	defer c.Close()

	// Wait for hello
	var msg map[string]interface{}
	if err := c.ReadJSON(&msg); err != nil {
		t.Fatalf("read hello: %v", err)
	}
	if msg["type"] != "hello" {
		t.Fatalf("expected hello, got %v", msg["type"])
	}
	payload := msg["payload"].(map[string]interface{})
	token := payload["token"].(string)

	// Respond with hello_ack
	helloAck := map[string]interface{}{
		"type":    "hello_ack",
		"payload": map[string]interface{}{"token": token},
	}
	if err := c.WriteJSON(helloAck); err != nil {
		t.Fatalf("write hello_ack: %v", err)
	}

	// Should receive connected
	if err := c.ReadJSON(&msg); err != nil {
		t.Fatalf("read connected: %v", err)
	}
	if msg["type"] != "connected" {
		t.Fatalf("expected connected, got %v", msg["type"])
	}

	// Now trigger a broadcast
	broadcastURL := fmt.Sprintf("http://localhost:%d/test/broadcast", e.port)
	msgBody, _ := json.Marshal(map[string]interface{}{
		"Type":    "test_broadcast",
		"Payload": map[string]interface{}{"foo": "bar"},
	})
	resp, err := http.Post(broadcastURL, "application/json", bytes.NewReader(msgBody))
	if err != nil {
		t.Fatalf("failed to POST to /test/broadcast: %v", err)
	}
	resp.Body.Close()

	// Give the broadcast time to be processed
	time.Sleep(500 * time.Millisecond)

	// Should receive the broadcast
	gotBroadcast := false
	deadline := time.Now().Add(3 * time.Second)
	c.SetReadDeadline(deadline)
	for time.Now().Before(deadline) {
		if err := c.ReadJSON(&msg); err != nil {
			t.Fatalf("read broadcast: %v", err)
		}
		if msg["type"] == "test_broadcast" {
			gotBroadcast = true
			break
		}
	}
	if !gotBroadcast {
		t.Fatalf("did not receive test broadcast")
	}
}
