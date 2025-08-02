#!/bin/bash
set -e

# Time Whisperer Binary Verification Script
# ========================================
# This script helps verify the authenticity of a TimeWhisperer binary.
# It checks both the SHA256 hash and the GPG signature.

echo "Time Whisperer Binary Verification Script"
echo "========================================"

# Function to show help
show_help() {
  echo "Usage: $0 <binary_file>"
  echo ""
  echo "This script verifies:"
  echo "1. The SHA256 hash matches the provided .sha256 file"
  echo "2. The GPG signature on the .sha256 file is valid"
  echo ""
  echo "Required files in the same directory:"
  echo "- Binary file (e.g., timewhisperer-macos-amd64)"
  echo "- Hash file (e.g., timewhisperer-macos-amd64.sha256)"
  echo "- Signature file (e.g., timewhisperer-macos-amd64.sha256.asc)"
  echo ""
  echo "Example:"
  echo "  $0 timewhisperer-macos-amd64"
  exit 1
}

# Check if binary file argument was provided
if [ $# -ne 1 ]; then
  show_help
fi

BINARY_FILE="$1"
HASH_FILE="${BINARY_FILE}.sha256"
SIG_FILE="${HASH_FILE}.asc"

# Check if files exist
if [ ! -f "$BINARY_FILE" ]; then
  echo "❌ Error: Binary file not found: $BINARY_FILE"
  exit 1
fi

if [ ! -f "$HASH_FILE" ]; then
  echo "❌ Error: Hash file not found: $HASH_FILE"
  exit 1
fi

if [ ! -f "$SIG_FILE" ]; then
  echo "❌ Error: Signature file not found: $SIG_FILE"
  exit 1
fi

echo "Found all required files:"
echo "- Binary: $BINARY_FILE"
echo "- Hash: $HASH_FILE"
echo "- Signature: $SIG_FILE"
echo ""

# Verify SHA256 hash
echo "Step 1: Verifying SHA256 hash..."
HASH_CMD=""
if command -v shasum >/dev/null 2>&1; then
  HASH_CMD="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  HASH_CMD="sha256sum"
else
  echo "❌ Error: Neither shasum nor sha256sum found"
  exit 1
fi

# Check if hash verification succeeds
if $HASH_CMD -c "$HASH_FILE"; then
  echo "✅ Hash verification successful!"
else
  echo "❌ Hash verification failed!"
  exit 1
fi

echo ""

# Verify GPG signature
echo "Step 2: Verifying GPG signature..."

# Check if gpg is installed
if ! command -v gpg >/dev/null 2>&1; then
  echo "❌ Error: GPG is not installed. Please install GPG and try again."
  exit 1
fi

# Try to verify the signature
if gpg --verify "$SIG_FILE" "$HASH_FILE" 2>/dev/null; then
  echo "✅ Signature verification successful!"
else
  echo "⚠️  Signature verification failed. You need to import the public key first."
  
  # Extract the key ID from the signature error message
  KEY_ID=$(gpg --verify "$SIG_FILE" "$HASH_FILE" 2>&1 | grep -o "[0-9A-F]\{8,40\}" | head -1)
  
  if [ -n "$KEY_ID" ]; then
    echo ""
    echo "Detected key ID: $KEY_ID"
    echo "Would you like to try importing this key? (y/N)"
    read -r response
    
    if [[ "$response" =~ ^[Yy]$ ]]; then
      echo "Trying to import the key..."
      
      # Method 1: Try to find public key file in current directory
      if [ -f "hyperbach-public-key.asc" ]; then
        echo "Method 1: Found local public key file, importing..."
        if gpg --import hyperbach-public-key.asc; then
          echo "✅ Successfully imported key from local file"
          if gpg --verify "$SIG_FILE" "$HASH_FILE"; then
            echo "✅ Signature verification successful!"
            exit 0
          fi
        fi
      fi
      
      # Method 2: Try GitHub
      echo "Method 2: Trying to import from GitHub..."
      if command -v curl >/dev/null 2>&1; then
        if curl -s https://github.com/hyperbach-git.gpg | gpg --import 2>/dev/null; then
          echo "✅ Successfully imported key from GitHub"
          if gpg --verify "$SIG_FILE" "$HASH_FILE"; then
            echo "✅ Signature verification successful!"
            exit 0
          fi
        else
          echo "❌ Failed to import from GitHub"
        fi
      else
        echo "❌ curl not available, cannot import from GitHub"
      fi
      
      echo "❌ Could not import the key using available methods."
      echo ""
      echo "Manual steps to verify:"
      echo "1. Download the public key from https://github.com/Hyperbach/time-whisperer/releases"
      echo "2. Import it: gpg --import hyperbach-public-key.asc"
      echo "3. Re-run this script"
      echo ""
      echo "Or import directly from GitHub:"
      echo "curl -s https://github.com/hyperbach-git.gpg | gpg --import"
      exit 1
    else
      echo "Skipping key import. Signature remains unverified."
      exit 1
    fi
  else
    echo "❌ Could not determine the key ID to import."
    exit 1
  fi
fi

echo ""
echo "🎉 Verification complete! This binary appears to be authentic."
echo ""
echo "You can now use this binary with confidence that it was built"
echo "from the official Time Whisperer source code." 