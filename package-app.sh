#!/bin/bash
# Build the macOS .app bundle and .dmg for the control panel + headless daemon.
#
# Layout produced:
#   <APP_NAME>.app/Contents/
#     Info.plist
#     MacOS/<APP_NAME>          <- the egui control panel (CFBundleExecutable)
#     Resources/worklogd        <- the headless daemon (LaunchAgent target)
#     Resources/default_config.json
#
# The daemon has no UI; the control panel installs it as a LaunchAgent.
set -euo pipefail

APP_NAME="${APP_NAME:-Worklog}"
BUNDLE_ID="${BUNDLE_ID:-com.hyperbach.worklog}"
VERSION="${VERSION:-1.0.0}"

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "==> Building release binaries (worklog-gui, time-whisperer)..."
GIT_COMMIT="$COMMIT" BUILD_DATE="$BUILD_DATE" \
  cargo build --release --bin worklog-gui --bin time-whisperer

APP="$ROOT/dist/${APP_NAME}.app"
echo "==> Assembling ${APP_NAME}.app ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "target/release/worklog-gui"   "$APP/Contents/MacOS/${APP_NAME}"
cp "target/release/time-whisperer" "$APP/Contents/Resources/worklogd"
chmod +x "$APP/Contents/MacOS/${APP_NAME}" "$APP/Contents/Resources/worklogd"

# Bundled default config (daemon discovers the Upwork dir on first run anyway).
if [ -f configs/macos/default_config.json ]; then
  cp configs/macos/default_config.json "$APP/Contents/Resources/default_config.json"
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

# Sign nested binaries first, then the bundle. With a Developer ID identity
# (APPLE_DEVELOPER_ID_APP) we use a secure timestamp + hardened runtime, both
# required for notarization. Without it we fall back to ad-hoc (local/dev).
if [ -n "${APPLE_DEVELOPER_ID_APP:-}" ]; then
  echo "==> Signing with Developer ID (hardened runtime): $APPLE_DEVELOPER_ID_APP"
  CS_OPTS=(--force --timestamp --options runtime --sign "$APPLE_DEVELOPER_ID_APP")
else
  echo "==> Ad-hoc signing (set APPLE_DEVELOPER_ID_APP to sign for distribution)..."
  CS_OPTS=(--force --timestamp=none --sign -)
fi
codesign "${CS_OPTS[@]}" "$APP/Contents/Resources/worklogd"
codesign "${CS_OPTS[@]}" "$APP/Contents/MacOS/${APP_NAME}"
codesign "${CS_OPTS[@]}" "$APP"
codesign --verify --deep --strict "$APP" && echo "    signature OK"

echo "==> Building .dmg ..."
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$ROOT/dist/${APP_NAME}-${VERSION}.dmg"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

echo ""
echo "Done."
echo "  App: $APP"
echo "  DMG: $DMG"
echo ""
echo "Install: open the .dmg, drag ${APP_NAME} to Applications."
echo "First launch (ad-hoc signed): right-click ${APP_NAME} -> Open -> Open."
echo "Then click 'Start at login' in the panel to install the background daemon."
