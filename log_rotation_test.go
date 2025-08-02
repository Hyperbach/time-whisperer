// log_rotation_bug_test.go
package main

import (
	"bytes"
	"context"
	"log"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// TestTruncateWhileOpen verifies the monitor survives copy-truncate rotation.
func TestTruncateWhileOpen(t *testing.T) {
	tmpDir, _ := os.MkdirTemp("", "tw-rot-*")
	defer os.RemoveAll(tmpDir)

	logDir := filepath.Join(tmpDir, "logs")
	_ = os.MkdirAll(logDir, 0o755)
	logPath := filepath.Join(logDir, "upwork..20250523.log")
	_ = os.WriteFile(logPath, nil, 0o644)

	// capture log output
	var buf bytes.Buffer
	log.SetOutput(&buf)
	defer log.SetOutput(os.Stdout)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go runMonitor(ctx, logDir)
	time.Sleep(100 * time.Millisecond) // watcher warm-up

	waitFor := func(want int, why string) {
		deadline := time.After(2 * time.Second)
		for {
			if strings.Count(buf.String(), "Screenshot detected") >= want {
				return
			}
			select {
			case <-deadline:
				t.Fatalf("wanted %d detections %s, got log:\n%s", want, why, buf.String())
			default:
				time.Sleep(50 * time.Millisecond)
			}
		}
	}

	appendScreenshot(t, logPath, "10:00:00.000")
	waitFor(1, "after first append")

	// ── copy-truncate simulation ──────────────────────────────────────────────
	if err := os.Truncate(logPath, 0); err != nil {
		t.Fatalf("truncate: %v", err)
	}

	// *force* a zero-byte WRITE so the monitor sees the shrink immediately
	if f, err := os.OpenFile(logPath, os.O_WRONLY|os.O_APPEND, 0o644); err == nil {
		_, _ = f.Write([]byte{}) // generates WRITE event
		_ = f.Close()
	}
	time.Sleep(50 * time.Millisecond) // let event propagate
	// ──────────────────────────────────────────────────────────────────────────

	appendScreenshot(t, logPath, "10:00:05.000")
	waitFor(2, "after truncate + second append")
}

// ----- helpers --------------------------------------------------------------

func appendScreenshot(t *testing.T, path, hhmmss string) {
	t.Helper()
	line := "[" + time.Now().Format("2006-01-02T") + hhmmss +
		"] [INFO] main.shell.os_services - Electron Screensnap succeeded.\n"

	f, err := os.OpenFile(path, os.O_WRONLY|os.O_APPEND|os.O_CREATE, 0o644)
	if err != nil {
		t.Fatalf("open for append: %v", err)
	}
	defer f.Close()

	if _, err := f.WriteString(line); err != nil {
		t.Fatalf("append screenshot: %v", err)
	}
}
