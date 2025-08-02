#!/bin/bash
set -euo pipefail

# Determine script directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="${ROOT_DIR}/dist"

# Create dist directory if it doesn't exist
mkdir -p "$DIST_DIR"

# Determine platform
UNAME_S=$(uname -s)
UNAME_M=$(uname -m)

case "$UNAME_S" in
  Darwin)
    if [ "$UNAME_M" = "arm64" ]; then
      PLATFORM="macos-arm64"
      NIX_SYSTEM="aarch64-darwin"
    else
      PLATFORM="macos-amd64"
      NIX_SYSTEM="x86_64-darwin"
    fi
    ;;
  Linux)
    if [ "$UNAME_M" = "aarch64" ] || [ "$UNAME_M" = "arm64" ]; then
      PLATFORM="linux-arm64"
      NIX_SYSTEM="aarch64-linux"
    else
      PLATFORM="linux-amd64" 
      NIX_SYSTEM="x86_64-linux"
    fi
    ;;
  *)
    echo "Unsupported platform: $UNAME_S"
    exit 1
    ;;
esac

# Extract version from git if available
VERSION="$(git describe --tags 2>/dev/null || echo '1.0.0-dev')"
VERSION="${VERSION#v}"  # Remove 'v' prefix if present

# Build with Nix for reproducibility
echo "Building TimeWhisperer ${VERSION} for ${PLATFORM}..."

# Ensure Nix is installed
if ! command -v nix >/dev/null 2>&1; then
  echo "Error: Nix is not installed. Please install Nix: https://nixos.org/download.html"
  exit 1
fi

# Use Nix to build the binary
nix build ".#default" --system "$NIX_SYSTEM" --out-link "result-${PLATFORM}"

# Copy binary to dist directory
BINARY_NAME="timewhisperer-${PLATFORM}"
cp "result-${PLATFORM}/bin/timewhisperer" "${DIST_DIR}/${BINARY_NAME}"

# Generate SHA256 hash
echo "Generating SHA256 hash..."
if [[ "$UNAME_S" == "Darwin" ]]; then
  shasum -a 256 "${DIST_DIR}/${BINARY_NAME}" > "${DIST_DIR}/${BINARY_NAME}.sha256"
else
  sha256sum "${DIST_DIR}/${BINARY_NAME}" > "${DIST_DIR}/${BINARY_NAME}.sha256"
fi

echo "Build complete!"
echo "Binary: ${DIST_DIR}/${BINARY_NAME}"
echo "SHA256: ${DIST_DIR}/${BINARY_NAME}.sha256" 