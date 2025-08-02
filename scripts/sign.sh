#!/bin/bash
set -euo pipefail

# Determine script directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="${ROOT_DIR}/dist"

# Ensure GPG is installed
if ! command -v gpg >/dev/null 2>&1; then
  echo "Error: GPG is not installed. Please install GPG."
  exit 1
fi

# Check that we have files to sign
if [ -z "$(ls -A ${DIST_DIR}/*.sha256 2>/dev/null)" ]; then
  echo "Error: No .sha256 files found in ${DIST_DIR}"
  echo "Run build.sh first to generate binaries and hash files."
  exit 1
fi

# Sign all SHA256 files
echo "Signing SHA256 hash files with GPG..."

# Check if CI environment - use imported key if available
if [ -n "${GPG_KEY_ID:-}" ]; then
  echo "Using CI environment signing key: ${GPG_KEY_ID}"
  
  # Sign each hash file
  for hashfile in ${DIST_DIR}/*.sha256; do
    echo "Signing ${hashfile}..."
    gpg --batch --yes --armor --detach-sign --local-user "${GPG_KEY_ID}" "${hashfile}"
    
    # Verify the signature
    if gpg --verify "${hashfile}.asc" "${hashfile}"; then
      echo "✓ Signature verified for ${hashfile}"
    else
      echo "× Signature verification failed for ${hashfile}"
      exit 1
    fi
  done
  
else
  # Interactive mode for development
  echo "Signing in interactive mode..."
  
  # Sign each hash file
  for hashfile in ${DIST_DIR}/*.sha256; do
    echo "Signing ${hashfile}..."
    gpg --armor --detach-sign "${hashfile}"
    
    # Verify the signature
    if gpg --verify "${hashfile}.asc" "${hashfile}"; then
      echo "✓ Signature verified for ${hashfile}"
    else
      echo "× Signature verification failed for ${hashfile}"
      exit 1
    fi
  done
fi

echo "All files signed successfully!"
echo "Signed files:"
for sigfile in ${DIST_DIR}/*.sha256.asc; do
  echo " - ${sigfile}"
done 