package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const tsLayout = "2006-01-02T15:04:05.000"

func equalRawTime(t time.Time, raw string) bool { return t.Format(tsLayout) == raw }

func hasPrefixIgnoringZone(s, raw string) bool { return strings.HasPrefix(s, raw) }

func TestFindLatestLog(t *testing.T) {
	tmp, err := os.MkdirTemp("", "tw-*")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(tmp)

	files := []struct {
		name string
		mod  time.Time
	}{
		{"upwork..20250410.log", time.Now().Add(-48 * time.Hour)},
		{"upwork..20250411.log", time.Now().Add(-24 * time.Hour)},
		{"upwork..20250412.log", time.Now()},                         // newest
		{"upwork.cmon.20250412.log", time.Now().Add(-1 * time.Hour)}, // ignored
	}
	for _, f := range files {
		p := filepath.Join(tmp, f.name)
		os.WriteFile(p, []byte("x"), 0644)
		os.Chtimes(p, f.mod, f.mod)
	}
	got := findLatestLog(tmp)
	want := filepath.Join(tmp, "upwork..20250412.log")
	if got != want {
		t.Fatalf("got %s want %s", got, want)
	}
}

// -----------------------------------------------------------------------------
// lastScreenshotInfo -----------------------------------------------------------
func TestLastScreenshotInfo(t *testing.T) {
	f, _ := os.CreateTemp("", "ss-*.log")
	defer os.Remove(f.Name())
	f.WriteString(`
[2025-04-10T10:00:00.000] foo
[2025-04-10T12:30:45.123] [INFO] main.shell.os_services - Electron Screensnap succeeded.
[2025-04-10T15:00:00.000] bar
[2025-04-10T18:45:30.456] [INFO] main.shell.os_services - Electron Screensnap succeeded.
`)
	f.Close()

	ts, line, err := lastScreenshotInfo(f.Name())
	if err != nil {
		t.Fatal(err)
	}
	if !equalRawTime(ts, "2025-04-10T18:45:30.456") {
		t.Fatalf("timestamp mismatch: %s", ts.Format(tsLayout))
	}
	if !strings.Contains(line, "18:45:30.456") {
		t.Fatalf("unexpected line: %q", line)
	}
}

// -----------------------------------------------------------------------------
// getAllScreenshotTimestamps ---------------------------------------------------
func TestGetAllScreenshotTimestamps(t *testing.T) {
	f, _ := os.CreateTemp("", "ss-*.log")
	defer os.Remove(f.Name())
	f.WriteString(`
[2025-04-10T10:30:45.123] [INFO] main.shell.os_services - Electron Screensnap succeeded.
[2025-04-10T11:00:00.000] foo
[2025-04-10T12:45:30.456] [INFO] main.shell.os_services - Electron Screensnap succeeded.
[2025-04-10T13:15:20.789] [INFO] main.shell.os_services - Electron Screensnap succeeded.
`)
	f.Close()

	got := getAllScreenshotTimestamps(f.Name())
	want := []string{
		"2025-04-10T10:30:45.123",
		"2025-04-10T12:45:30.456",
		"2025-04-10T13:15:20.789",
	}
	if len(got) != 3 {
		t.Fatalf("expected 3, got %d", len(got))
	}
	for i := range want {
		if i >= len(got) || !hasPrefixIgnoringZone(got[i], want[i]) {
			t.Fatalf("timestamp[%d] = %s, want prefix %s", i, got[i], want[i])
		}
	}
}
