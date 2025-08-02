package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLoadConfig_InvalidJSON(t *testing.T) {
	// Create a temporary directory for test
	tempDir, err := os.MkdirTemp("", "config-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	// Create an invalid config file with trailing comma (common JSON error)
	configPath := filepath.Join(tempDir, "config.json")
	invalidJSON := `{
		"debugMode": tru,
		"logPath": "/path/to/log",
		"upworkLogsDir": "/path/to/upwork",
		"webSocketPort": 8080
	}`

	if err := os.WriteFile(configPath, []byte(invalidJSON), 0644); err != nil {
		t.Fatalf("Failed to write invalid config: %v", err)
	}

	// Try to load the config
	_, _, err = loadConfig(configPath)

	// Verify the error is returned
	if err == nil {
		t.Fatal("Expected error for invalid JSON, but got nil")
	}

	// Check if the error message includes "invalid json"
	if !strings.Contains(err.Error(), "invalid json") {
		t.Errorf("Expected error to contain 'invalid json', got: %v", err)
	}

	// Check that the original config.json no longer exists
	if _, err := os.Stat(configPath); !os.IsNotExist(err) {
		t.Errorf("Original config file should not exist, but it does")
	}

	// Check that a backup file was created
	files, err := os.ReadDir(tempDir)
	if err != nil {
		t.Fatalf("Failed to read temp dir contents: %v", err)
	}

	var foundBackup bool
	for _, file := range files {
		if strings.HasPrefix(file.Name(), "config.json.bak-") {
			foundBackup = true

			// Verify backup contains the original content
			backupPath := filepath.Join(tempDir, file.Name())
			content, err := os.ReadFile(backupPath)
			if err != nil {
				t.Fatalf("Failed to read backup file: %v", err)
			}

			if string(content) != invalidJSON {
				t.Errorf("Backup file content doesn't match original: %s", string(content))
			}
			break
		}
	}

	if !foundBackup {
		t.Error("No backup file was created")
	}
}

func TestLoadConfig_NonExistentFile(t *testing.T) {
	// Create a temporary directory for test
	tempDir, err := os.MkdirTemp("", "config-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	// Use a non-existent config path
	configPath := filepath.Join(tempDir, "nonexistent-config.json")

	// Try to load the config
	_, source, err := loadConfig(configPath)

	// Should not return an error for missing file
	if err != nil {
		t.Fatalf("Expected no error for missing file, got: %v", err)
	}

	// Should fallback to defaults
	if !strings.Contains(source, "Default hardcoded config") {
		t.Errorf("Expected default config source, got: %s", source)
	}

	// Verify a config file was created at the path
	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		t.Errorf("Expected config file to be created, but it wasn't")
	}
}
