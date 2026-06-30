#!/bin/bash
# Build a macOS .pkg installer (the Installer.app wizard) for Worklog.
#
# The wizard: Welcome -> Install (progress) -> Done. The payload drops
# Worklog.app into /Applications, and a postinstall script installs + starts
# the background daemon as a per-user LaunchAgent — so monitoring runs the
# moment the wizard finishes. The Worklog app is then just the status panel.
#
# Optional signing (needs an Apple Developer membership):
#   APPLE_DEVELOPER_ID_INSTALLER="Developer ID Installer: NAME (TEAMID)" ./package-pkg.sh
# Without it the .pkg is unsigned (Gatekeeper needs right-click->Open until the
# .pkg is signed *and* notarized).
set -euo pipefail

APP_NAME="${APP_NAME:-Worklog}"
PKG_ID="${PKG_ID:-com.hyperbach.worklog.pkg}"
VERSION="${VERSION:-1.0.0}"

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

# 1) Build the .app (reuses package-app.sh: cargo build + assemble + ad-hoc sign).
echo "==> Building ${APP_NAME}.app (via package-app.sh)..."
VERSION="$VERSION" ./package-app.sh >/dev/null
APP="$ROOT/dist/${APP_NAME}.app"
[ -d "$APP" ] || { echo "ERROR: $APP not found after package-app.sh"; exit 1; }

# 2) Stage the payload and build the component package (payload + postinstall).
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PKGROOT="$WORK/root"
mkdir -p "$PKGROOT/Applications"
cp -R "$APP" "$PKGROOT/Applications/"

# Disable bundle relocation. By default pkgbuild marks .app bundles relocatable,
# so macOS Installer will "shove" the install onto any other registered copy of
# Worklog.app (a mounted DMG, a dev build in dist/, …) instead of /Applications.
# A component plist with BundleIsRelocatable=false forces the declared location.
echo "==> pkgbuild --analyze (pin install location)..."
pkgbuild --analyze --root "$PKGROOT" "$WORK/component.plist"
# The analyze output is an array of bundle dicts; force every bundle's flag false.
/usr/libexec/PlistBuddy -c "Set :0:BundleIsRelocatable false" "$WORK/component.plist"

echo "==> pkgbuild (payload + postinstall)..."
pkgbuild \
  --root "$PKGROOT" \
  --component-plist "$WORK/component.plist" \
  --install-location / \
  --scripts "$ROOT/installer/scripts" \
  --identifier "$PKG_ID" \
  --version "$VERSION" \
  "$WORK/component.pkg"

# 3) Wrap in a distribution product (the wizard: welcome + conclusion panes).
cat > "$WORK/distribution.xml" <<DIST
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>${APP_NAME}</title>
    <welcome file="welcome.html" mime-type="text/html"/>
    <conclusion file="conclusion.html" mime-type="text/html"/>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <choices-outline>
        <line choice="default"/>
    </choices-outline>
    <choice id="default" title="${APP_NAME}">
        <pkg-ref id="${PKG_ID}"/>
    </choice>
    <pkg-ref id="${PKG_ID}" version="${VERSION}">component.pkg</pkg-ref>
</installer-gui-script>
DIST

mkdir -p "$ROOT/dist"
OUT="$ROOT/dist/${APP_NAME}-${VERSION}.pkg"
rm -f "$OUT"

echo "==> productbuild (wizard)..."
if [ -n "${APPLE_DEVELOPER_ID_INSTALLER:-}" ]; then
  productbuild \
    --distribution "$WORK/distribution.xml" \
    --resources "$ROOT/installer/resources" \
    --package-path "$WORK" \
    --sign "$APPLE_DEVELOPER_ID_INSTALLER" \
    "$OUT"
else
  productbuild \
    --distribution "$WORK/distribution.xml" \
    --resources "$ROOT/installer/resources" \
    --package-path "$WORK" \
    "$OUT"
  echo "    NOTE: unsigned installer (set APPLE_DEVELOPER_ID_INSTALLER to sign; notarize for clean Gatekeeper)."
fi

echo ""
echo "Done. Installer wizard: $OUT"
