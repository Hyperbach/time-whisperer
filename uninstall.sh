#!/bin/bash

set -e

# Detect OS
OS="$(uname -s)"
BINARY_NAME="time-whisperer"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="$HOME/.config/time-whisperer"

echo "SneakTime - Uninstaller"
echo "============================"
echo

# Stop and remove service
if [ "$OS" = "Darwin" ]; then
    # macOS uninstall
    SERVICE_DIR="$HOME/Library/LaunchAgents"
    SERVICE_FILE="com.hyperbach.time-whisperer.plist"
    
    if [ -f "$SERVICE_DIR/$SERVICE_FILE" ]; then
        echo "Stopping and removing service..."
        launchctl unload -w "$SERVICE_DIR/$SERVICE_FILE" 2>/dev/null || true
        rm -f "$SERVICE_DIR/$SERVICE_FILE"
        echo "Service removed."
    fi
elif [ "$OS" = "Linux" ]; then
    # Linux uninstall
    SERVICE_DIR="$HOME/.config/systemd/user"
    SERVICE_FILE="time-whisperer.service"
    
    if [ -f "$SERVICE_DIR/$SERVICE_FILE" ]; then
        echo "Stopping and removing service..."
        systemctl --user stop "$SERVICE_FILE" 2>/dev/null || true
        systemctl --user disable "$SERVICE_FILE" 2>/dev/null || true
        rm -f "$SERVICE_DIR/$SERVICE_FILE"
        systemctl --user daemon-reload
        echo "Service removed."
    fi
fi

# Remove binary
if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    echo "Removing $BINARY_NAME from $INSTALL_DIR..."
    sudo rm -f "$INSTALL_DIR/$BINARY_NAME"
    echo "Binary removed."
fi

# Ask about config
read -p "Would you like to remove all configuration and logs? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    if [ -d "$CONFIG_DIR" ]; then
        echo "Removing configuration directory..."
        rm -rf "$CONFIG_DIR"
        echo "Configuration removed."
    fi
else
    echo "Configuration preserved at $CONFIG_DIR"
fi

echo
echo "SneakTime has been uninstalled."
echo "Thanks for trying it out!" 