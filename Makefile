# Makefile for SneakTime - Upwork Screenshot Monitor

# Go commands
GOCMD=go
GOBUILD=$(GOCMD) build
GORUN=$(GOCMD) run
GOCLEAN=$(GOCMD) clean
BINARY_NAME=time-whisperer

# Version info
VERSION ?= 1.0.0
COMMIT=$(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
BUILD_DATE=$(shell date -u +%FT%T%z 2>/dev/null || date -u)

# Build flags
LDFLAGS=-ldflags "-X main.Version=$(VERSION) -X main.GitCommit=$(COMMIT) -X main.BuildDate=$(BUILD_DATE)"

# Installation paths
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CONFIG_DIR ?= $(HOME)/.config/time-whisperer

# OS detection
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S), Darwin)
	SERVICE_DIR = $(HOME)/Library/LaunchAgents
	SERVICE_FILE = com.hyperbach.time-whisperer.plist
else
	SERVICE_DIR = $(HOME)/.config/systemd/user
	SERVICE_FILE = time-whisperer.service
endif

# Define the phony targets - targets that don't represent files
.PHONY: all build run clean install uninstall darwin-service linux-service package package-macos package-ubuntu release build-with-nix build-darwin-nix build-linux-nix help config show-config test

# Help target - displays available commands
help: ## Show this help message
	@echo "SneakTime - Upwork Screenshot Monitor - Makefile Commands:"
	@echo "==========================================================="
	@echo
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
	@echo
	@echo "Examples:"
	@echo "  make build              Build the binary"
	@echo "  make install            Build and install the application"
	@echo "  make package-macos      Build and create macOS package"

all: build ## Default target: build the application

build: ## Build the binary with local toolchain
	$(GOBUILD) $(LDFLAGS) -o $(BINARY_NAME) main.go
	mkdir -p $(BINARY_NAME).app/Contents/Resources
	cp -r configs $(BINARY_NAME).app/Contents/Resources/
	
	# Copy platform-specific config to root directory for development convenience
ifeq ($(UNAME_S), Darwin)
	cp configs/macos/default_config.json config.json
else ifeq ($(UNAME_S), Linux)
	cp configs/linux/default_config.json config.json
else
	cp configs/windows/default_config.json config.json
endif

test: ## Run all tests with verbose output
	$(GOCMD) test -v ./...

run: ## Run the application without installing (development)
	$(GORUN) main.go

clean: ## Remove built files and packages
	$(GOCLEAN)
	rm -f $(BINARY_NAME)
	rm -rf build/
	rm -f *.dmg *.deb

install: build ## Build and install the application
	mkdir -p $(CONFIG_DIR)
	@echo "Installing to $(BINDIR)..."
	@if [ -w $(BINDIR) ]; then \
		mkdir -p $(BINDIR); \
		cp $(BINARY_NAME) $(BINDIR)/; \
		echo "The time-whisperer has been installed to $(BINDIR)/$(BINARY_NAME)"; \
	else \
		echo "Need administrator privileges to install to $(BINDIR)"; \
		sudo mkdir -p $(BINDIR); \
		sudo cp $(BINARY_NAME) $(BINDIR)/; \
		echo "The time-whisperer has been installed to $(BINDIR)/$(BINARY_NAME) (with sudo)"; \
	fi
	@echo
ifeq ($(UNAME_S), Darwin)
	@echo "To set up as a service, run: make darwin-service"
else
	@echo "To set up as a service, run: make linux-service"
endif

uninstall: ## Uninstall the application and its service
	rm -f $(BINDIR)/$(BINARY_NAME)
ifeq ($(UNAME_S), Darwin)
	launchctl unload -w $(SERVICE_DIR)/$(SERVICE_FILE) 2>/dev/null || true
	rm -f $(SERVICE_DIR)/$(SERVICE_FILE)
else
	systemctl --user stop $(SERVICE_FILE) 2>/dev/null || true
	systemctl --user disable $(SERVICE_FILE) 2>/dev/null || true
	rm -f $(SERVICE_DIR)/$(SERVICE_FILE)
endif
	@echo "Uninstalled time-whisperer"

darwin-service: ## Install and start as a LaunchAgent service on macOS
	mkdir -p $(SERVICE_DIR)
	@echo '<?xml version="1.0" encoding="UTF-8"?>' > $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '<plist version="1.0">' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '<dict>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>Label</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <string>com.hyperbach.time-whisperer</string>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>ProgramArguments</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <array>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '        <string>$(BINDIR)/$(BINARY_NAME)</string>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    </array>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>RunAtLoad</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <true/>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>KeepAlive</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <true/>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>StandardOutPath</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <string>$(CONFIG_DIR)/output.log</string>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <key>StandardErrorPath</key>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '    <string>$(CONFIG_DIR)/error.log</string>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '</dict>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '</plist>' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	launchctl load -w $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo "Service installed and started. To stop: launchctl unload -w $(SERVICE_DIR)/$(SERVICE_FILE)"

linux-service: ## Install and start as a systemd user service on Linux
	mkdir -p $(SERVICE_DIR)
	@echo '[Unit]' > $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'Description=SneakTime - Upwork Screenshot Monitor' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'After=network.target' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '[Service]' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'ExecStart=$(BINDIR)/$(BINARY_NAME)' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'Restart=always' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'RestartSec=10' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'StandardOutput=append:$(CONFIG_DIR)/output.log' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'StandardError=append:$(CONFIG_DIR)/error.log' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo '[Install]' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	@echo 'WantedBy=default.target' >> $(SERVICE_DIR)/$(SERVICE_FILE)
	systemctl --user daemon-reload
	systemctl --user enable $(SERVICE_FILE)
	systemctl --user start $(SERVICE_FILE)
	@echo "Service installed and started. To check status: systemctl --user status $(SERVICE_FILE)"

# Packaging targets
package: package-macos package-ubuntu ## Create packages for all supported platforms

package-macos: build ## Create a macOS DMG package
	@echo "Building macOS package..."
	VERSION=$(VERSION) ./package-macos.sh
	@echo "macOS package created!"

package-ubuntu: build ## Create an Ubuntu/Debian DEB package
	@echo "Building Ubuntu/Debian package..."
	VERSION=$(VERSION) ./create-ubuntu-deb.sh
	@echo "Copying config files to package..."
	# Ensure configs directory exists in the package build directory
	mkdir -p debian/usr/share/time-whisperer/configs
	cp -r configs/* debian/usr/share/time-whisperer/configs/
	cp configs/linux/default_config.json debian/usr/share/time-whisperer/default_config.json
	@echo "Ubuntu/Debian package created!"


# Build with Nix (detects OS automatically)
build-with-nix: ## Build the application using Nix (automatic OS detection)
ifeq ($(UNAME_S), Darwin)
	@$(MAKE) build-darwin-nix
else
	@$(MAKE) build-linux-nix
endif

# Build for macOS using Nix
build-darwin-nix: ## Build the application using Nix for macOS
	@echo "Building with Nix for macOS..."
	rm -rf dist
	mkdir -p dist
	chmod 755 dist
	nix build .#default --system x86_64-darwin --out-link result-macos-amd64
	
	@echo "Contents of Nix build result:"
	ls -la result-macos-amd64
	
	@echo "Determining actual store path (follow symlink):"
	STORE_PATH=$$(readlink -f result-macos-amd64); \
	echo "Actual store path: $$STORE_PATH"; \
	echo ""; \
	echo "Searching for executables in the build result:"; \
	find -L $$STORE_PATH -type f -perm +111 | sort; \
	echo ""; \
	if [ -f $$STORE_PATH/bin/timewhisperer ]; then \
		echo "Found binary at standard location"; \
		cp $$STORE_PATH/bin/timewhisperer dist/timewhisperer-macos-amd64; \
	elif [ -f $$STORE_PATH/timewhisperer ]; then \
		echo "Found binary at root of output"; \
		cp $$STORE_PATH/timewhisperer dist/timewhisperer-macos-amd64; \
	else \
		echo "Searching for binary by file type..."; \
		BINARY_PATH=$$(find -L $$STORE_PATH -type f -perm +111 -exec file {} \; | grep "Mach-O 64-bit executable" | head -1 | cut -d: -f1); \
		if [ -n "$$BINARY_PATH" ]; then \
			echo "Found binary at: $$BINARY_PATH"; \
			cp "$$BINARY_PATH" dist/timewhisperer-macos-amd64 || { \
				echo "Permission issue - trying with sudo"; \
				sudo cp "$$BINARY_PATH" dist/timewhisperer-macos-amd64; \
				sudo chown $$(id -u):$$(id -g) dist/timewhisperer-macos-amd64; \
			}; \
		else \
			echo "ERROR: Could not locate the binary in the build output"; \
			find -L $$STORE_PATH -type f | xargs file | grep executable; \
			exit 1; \
		fi; \
	fi
	
	chmod 644 dist/timewhisperer-macos-amd64
	shasum -a 256 dist/timewhisperer-macos-amd64 > dist/timewhisperer-macos-amd64.sha256
	@echo "SHA256: $$(cat dist/timewhisperer-macos-amd64.sha256)"

# Build for Linux using Nix
build-linux-nix: ## Build the application using Nix for Linux
	@echo "Building with Nix for Linux..."
	rm -rf dist
	mkdir -p dist
	chmod 755 dist
	nix build .#default --system x86_64-linux --out-link result-linux-amd64
	
	@echo "Contents of Nix build result:"
	ls -la result-linux-amd64
	
	@echo "Determining actual store path (follow symlink):"
	STORE_PATH=$$(readlink -f result-linux-amd64); \
	echo "Actual store path: $$STORE_PATH"; \
	echo ""; \
	echo "Searching for executables in the build result:"; \
	find -L $$STORE_PATH -type f -executable | sort; \
	echo ""; \
	if [ -f $$STORE_PATH/bin/timewhisperer ]; then \
		echo "Found binary at standard location"; \
		cp $$STORE_PATH/bin/timewhisperer dist/timewhisperer-linux-amd64 || { \
			echo "Permission issue - trying with sudo"; \
			sudo cp $$STORE_PATH/bin/timewhisperer dist/timewhisperer-linux-amd64; \
			sudo chown $$(id -u):$$(id -g) dist/timewhisperer-linux-amd64; \
		}; \
	elif [ -f $$STORE_PATH/timewhisperer ]; then \
		echo "Found binary at root of output"; \
		cp $$STORE_PATH/timewhisperer dist/timewhisperer-linux-amd64 || { \
			echo "Permission issue - trying with sudo"; \
			sudo cp $$STORE_PATH/timewhisperer dist/timewhisperer-linux-amd64; \
			sudo chown $$(id -u):$$(id -g) dist/timewhisperer-linux-amd64; \
		}; \
	else \
		echo "Searching for binary by file type..."; \
		BINARY_PATH=$$(find -L $$STORE_PATH -type f -executable -exec file {} \; | grep "ELF 64-bit" | head -1 | cut -d: -f1); \
		if [ -n "$$BINARY_PATH" ]; then \
			echo "Found binary at: $$BINARY_PATH"; \
			cp "$$BINARY_PATH" dist/timewhisperer-linux-amd64 || { \
				echo "Permission issue - trying with sudo"; \
				sudo cp "$$BINARY_PATH" dist/timewhisperer-linux-amd64; \
				sudo chown $$(id -u):$$(id -g) dist/timewhisperer-linux-amd64; \
			}; \
		else \
			echo "ERROR: Could not locate the binary in the build output"; \
			find -L $$STORE_PATH -type f | xargs file | grep executable; \
			exit 1; \
		fi; \
	fi
	
	chmod 644 dist/timewhisperer-linux-amd64
	sha256sum dist/timewhisperer-linux-amd64 > dist/timewhisperer-linux-amd64.sha256
	@echo "SHA256: $$(cat dist/timewhisperer-linux-amd64.sha256)"

# Configuration targets
config: ## Generate default configuration file
	go run generate-config.go

show-config: ## Show the current configuration path
	@echo "Default config location: $$(go run -c 'package main; import "fmt"; import "os"; func main() { fmt.Println(GetDefaultConfigPath()) }' generate-config.go)"
	@if [ -f "$$(go run -c 'package main; import "fmt"; import "os"; func main() { fmt.Println(GetDefaultConfigPath()) }' generate-config.go)" ]; then \
		echo "Current config:"; \
		cat "$$(go run -c 'package main; import "fmt"; import "os"; func main() { fmt.Println(GetDefaultConfigPath()) }' generate-config.go)"; \
	else \
		echo "Config file does not exist. Run 'make config' to create it."; \
	fi 

release:
	git tag -a v$(VERSION) -m "Release $(VERSION)"
	git push origin v$(VERSION)
