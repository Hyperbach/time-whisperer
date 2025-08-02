#!/bin/bash

set -e

# Detect OS
OS="$(uname -s)"
BINARY_NAME="time-whisperer"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="$HOME/.config/time-whisperer"

echo "SneakTime - Upwork Screenshot Monitor"
echo "=========================================="
echo

# Check if we have the binary
if [ ! -f "./$BINARY_NAME" ]; then
    echo "Building $BINARY_NAME..."
    go build -o $BINARY_NAME main.go
fi

# Create config directory
mkdir -p "$CONFIG_DIR"

# Install the binary
echo "Installing $BINARY_NAME to $INSTALL_DIR..."
sudo cp "./$BINARY_NAME" "$INSTALL_DIR/"
sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo "Installation complete!"

# Service setup
if [ "$OS" = "Darwin" ]; then
    # macOS setup
    SERVICE_DIR="$HOME/Library/LaunchAgents"
    SERVICE_FILE="com.hyperbach.time-whisperer.plist"
    
    read -p "Would you like to set up SneakTime to start automatically? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        mkdir -p "$SERVICE_DIR"
        cat > "$SERVICE_DIR/$SERVICE_FILE" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hyperbach.time-whisperer</string>
    <key>ProgramArguments</key>
    <array>
        <string>$INSTALL_DIR/$BINARY_NAME</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$CONFIG_DIR/output.log</string>
    <key>StandardErrorPath</key>
    <string>$CONFIG_DIR/error.log</string>
</dict>
</plist>
EOF
        launchctl load -w "$SERVICE_DIR/$SERVICE_FILE"
        echo "Service installed and started!"
        echo "To check status: launchctl list | grep com.hyperbach.time-whisperer"
    else
        echo "No service installed. You can run '$BINARY_NAME' manually."
    fi
elif [ "$OS" = "Linux" ]; then
    # Linux setup
    SERVICE_DIR="$HOME/.config/systemd/user"
    SERVICE_FILE="time-whisperer.service"
    
    read -p "Would you like to set up SneakTime to start automatically? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        mkdir -p "$SERVICE_DIR"
        cat > "$SERVICE_DIR/$SERVICE_FILE" << EOF
[Unit]
Description=SneakTime - Upwork Screenshot Monitor
After=network.target

[Service]
ExecStart=$INSTALL_DIR/$BINARY_NAME
Restart=always
RestartSec=10
StandardOutput=append:$CONFIG_DIR/output.log
StandardError=append:$CONFIG_DIR/error.log

[Install]
WantedBy=default.target
EOF
        systemctl --user daemon-reload
        systemctl --user enable "$SERVICE_FILE"
        systemctl --user start "$SERVICE_FILE"
        echo "Service installed and started!"
        echo "To check status: systemctl --user status $SERVICE_FILE"
    else
        echo "No service installed. You can run '$BINARY_NAME' manually."
    fi
fi

echo
echo "To use SneakTime, simply run: $BINARY_NAME"
echo "Thanks for installing!" 