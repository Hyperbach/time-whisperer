#!/bin/bash

set -e

echo "Creating macOS DMG package for SneakTime..."
echo "==============================================="

# Configuration
APP_NAME="TimeWhisperer"
VERSION="${VERSION:-1.0.0}"  # Use env var if set, otherwise default to 1.0.0
BINARY_NAME="time-whisperer"
BINARY_PATH="${BINARY_PATH:-${BINARY_NAME}}"  # Allow override for Nix-built binaries
DMG_NAME="${APP_NAME}-${VERSION}.dmg"
VOLUME_NAME="${APP_NAME} ${VERSION}"

echo "Building version ${VERSION}"
echo "Using binary from: ${BINARY_PATH}"

# Create necessary directories
mkdir -p build/{payload/Applications/${APP_NAME}.app/Contents/{MacOS,Resources},dmg}

# Build the binary (if not exists and BINARY_PATH not specified)
if [ ! -f "${BINARY_PATH}" ] && [ "${BINARY_PATH}" = "${BINARY_NAME}" ]; then
    echo "Building ${BINARY_NAME}..."
    go build -o ${BINARY_NAME} main.go
fi

# Copy binary to app bundle
cp "${BINARY_PATH}" "build/payload/Applications/${APP_NAME}.app/Contents/MacOS/${BINARY_NAME}"
chmod +x "build/payload/Applications/${APP_NAME}.app/Contents/MacOS/${BINARY_NAME}"

# Copy default configs to Resources directory
mkdir -p "build/payload/Applications/${APP_NAME}.app/Contents/Resources"
cp -r configs "build/payload/Applications/${APP_NAME}.app/Contents/Resources/"
cp "configs/macos/default_config.json" "build/payload/Applications/${APP_NAME}.app/Contents/Resources/default_config.json"

# Create Info.plist
cat > build/payload/Applications/${APP_NAME}.app/Contents/Info.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>launcher</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.hyperbach.timewhisperer</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.12</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSUIElement</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>© $(date +%Y) HyperBach</string>
</dict>
</plist>
EOF

# Create a basic icon if it doesn't exist
if [ ! -f "icon.png" ]; then
    echo "Creating a basic icon..."
    # Here you would ideally use a proper icon, but for now we'll use a basic script
    # In a real app, you'd use a proper icon creation tool or designer-made icon
    cat > build/payload/Applications/${APP_NAME}.app/Contents/Resources/AppIcon.icns << EOF
(This is a placeholder - real apps need proper icon files)
EOF
else
    # Convert png to icns (this is simplified, real apps need proper icon conversion)
    cp icon.png build/payload/Applications/${APP_NAME}.app/Contents/Resources/AppIcon.icns
fi

# Create a launcher script
cat > build/payload/Applications/${APP_NAME}.app/Contents/MacOS/launcher << EOF
#!/bin/bash

# Get the directory of the script
DIR="\$(cd "\$(dirname "\${BASH_SOURCE[0]}")" && pwd)"

# Create config directory
mkdir -p "\$HOME/.config/time-whisperer"

# Check if first run and offer to install service
FIRST_RUN_MARKER="\$HOME/.config/time-whisperer/.first_run_complete"
if [ ! -f "\$FIRST_RUN_MARKER" ]; then
    osascript -e 'display dialog "Would you like SneakTime to start automatically when you log in?" buttons {"Yes", "No"} default button "Yes" with title "SneakTime Setup"' > /dev/null 2>&1
    if [ \$? -eq 0 ]; then
        # User clicked "Yes"
        "\$DIR/setup-service" > /dev/null 2>&1
        osascript -e 'display dialog "SneakTime will now start automatically when you log in." buttons {"OK"} default button "OK" with title "Setup Complete"' > /dev/null 2>&1
    fi
    # Mark first run as complete
    touch "\$FIRST_RUN_MARKER"
fi

# Run the app
"\$DIR/${BINARY_NAME}" &

exit 0
EOF

chmod +x build/payload/Applications/${APP_NAME}.app/Contents/MacOS/launcher

# Create service setup script that users can run if they want
cat > build/payload/Applications/${APP_NAME}.app/Contents/MacOS/setup-service << EOF
#!/bin/bash

SERVICE_DIR="\$HOME/Library/LaunchAgents"
SERVICE_FILE="com.hyperbach.time-whisperer.plist"
CONFIG_DIR="\$HOME/.config/time-whisperer"
APP_PATH="\$(cd "\$(dirname "\${BASH_SOURCE[0]}")" && pwd)"

mkdir -p "\$SERVICE_DIR"
mkdir -p "\$CONFIG_DIR"

cat > "\$SERVICE_DIR/\$SERVICE_FILE" << EOFINNER
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hyperbach.time-whisperer</string>
    <key>ProgramArguments</key>
    <array>
        <string>\$APP_PATH/${BINARY_NAME}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>\$CONFIG_DIR/output.log</string>
    <key>StandardErrorPath</key>
    <string>\$CONFIG_DIR/error.log</string>
</dict>
</plist>
EOFINNER

launchctl load -w "\$SERVICE_DIR/\$SERVICE_FILE"
echo "Service installed and started!"
echo "TimeWhisperer will now start automatically when you log in."

exit 0
EOF

chmod +x build/payload/Applications/${APP_NAME}.app/Contents/MacOS/setup-service

# Create uninstaller script
cat > build/payload/Applications/${APP_NAME}.app/Contents/MacOS/uninstall << EOF
#!/bin/bash

SERVICE_DIR="\$HOME/Library/LaunchAgents"
SERVICE_FILE="com.hyperbach.time-whisperer.plist"
CONFIG_DIR="\$HOME/.config/time-whisperer"
APP_DIR="/Applications/${APP_NAME}.app"

# Stop and remove service
if [ -f "\$SERVICE_DIR/\$SERVICE_FILE" ]; then
    echo "Stopping and removing service..."
    launchctl unload -w "\$SERVICE_DIR/\$SERVICE_FILE" 2>/dev/null || true
    rm -f "\$SERVICE_DIR/\$SERVICE_FILE"
    echo "Service removed."
fi

# Ask about config
read -p "Would you like to remove all configuration and logs? (y/n) " -n 1 -r
echo
if [[ \$REPLY =~ ^[Yy]$ ]]; then
    if [ -d "\$CONFIG_DIR" ]; then
        echo "Removing configuration directory..."
        rm -rf "\$CONFIG_DIR"
        echo "Configuration removed."
    fi
else
    echo "Configuration preserved at \$CONFIG_DIR"
fi

# Remove app
echo "Removing application..."
rm -rf "\$APP_DIR"
echo "Application removed."

echo "Uninstallation complete."
EOF

chmod +x build/payload/Applications/${APP_NAME}.app/Contents/MacOS/uninstall

# Create application versioning file
cat > build/payload/Applications/${APP_NAME}.app/Contents/version.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>BuildVersion</key>
	<string>${VERSION}</string>
	<key>CFBundleShortVersionString</key>
	<string>${VERSION}</string>
	<key>CFBundleVersion</key>
	<string>${VERSION}</string>
	<key>ProjectName</key>
	<string>${APP_NAME}</string>
	<key>SourceVersion</key>
	<string>${VERSION}</string>
</dict>
</plist>
EOF

# Create a README file
cat > build/payload/README.txt << EOF
# SneakTime
## Upwork Screenshot Monitor

Version: ${VERSION}

### Simple Installation:
1. Drag the TimeWhisperer app to your Applications folder
2. Double-click to launch SneakTime
3. When you see "TimeWhisperer is from an unidentified developer":
   - Right-click (or Control+click) on the app icon
   - Select "Open" from the menu
   - Click "Open" in the dialog that appears
   - This only needs to be done once

### What Does SneakTime Do?
- Monitors for Upwork screenshots in the background
- Notifies you when a screenshot is taken
- Runs silently without cluttering your desktop

### Uninstallation:
Simply double-click the app and select "Uninstall SneakTime" from the menu bar icon (⏱️)

For advanced users (command line uninstall):
/Applications/TimeWhisperer.app/Contents/MacOS/uninstall

### Support
For issues or questions, visit: https://github.com/kagel/time-whisperer
EOF

# Attempt code signing if a valid identity is available
if [ -n "${APPLE_DEVELOPER_ID}" ]; then
    echo "Attempting to code sign the application..."
    
    # Sign the app
    codesign --force --options runtime --sign "${APPLE_DEVELOPER_ID}" "build/payload/Applications/${APP_NAME}.app"
    
    # Verify the signature
    codesign --verify --verbose "build/payload/Applications/${APP_NAME}.app"
    
    echo "Code signing completed."
else
    echo "No APPLE_DEVELOPER_ID provided. The app will not be code signed."
    echo "Users will need to right-click and select 'Open' the first time they run the app."
fi

# Create a simpler DMG without fancy customization
# This is more reliable across different macOS versions
echo "Creating DMG file..."
hdiutil create -volname "${VOLUME_NAME}" \
               -srcfolder build/payload \
               -ov -format UDZO \
               "${DMG_NAME}"

echo "DMG package created: ${DMG_NAME}"
echo "Distribute this file to users for easy installation." 