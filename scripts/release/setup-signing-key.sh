#!/bin/bash
set -e

# Script to generate and configure a GPG key for signing Time Whisperer releases
# =============================================================================

echo "Time Whisperer Release Signing Key Setup"
echo "========================================"
echo

# Check if GPG is installed
if ! command -v gpg &> /dev/null; then
    echo "Error: GPG is not installed."
    echo "Please install GPG before continuing:"
    echo "  - macOS: brew install gnupg"
    echo "  - Ubuntu: sudo apt install gnupg"
    exit 1
fi

echo "This script will help you create a GPG key for signing Time Whisperer releases."
echo "The key will be used to cryptographically sign release artifacts."
echo
echo "NOTE: If you already have a signing key you want to use, you can skip this process."
echo

read -p "Do you want to continue with creating a new GPG key? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Operation cancelled."
    exit 0
fi

# Generate a new key
echo
echo "Generating a new GPG key..."
echo "When prompted:"
echo "  1. Select 'RSA and RSA' (default)"
echo "  2. Choose 4096 bits"
echo "  3. Set an expiration date (recommended: 2y)"
echo "  4. Provide your real name and email address"
echo "  5. Use a strong passphrase and store it securely"
echo

# Start key generation
gpg --full-generate-key

# Get the key ID of the newly created key - improved logic
echo
echo "Getting key ID of the newly created key..."
# Get the full fingerprint of the most recently created key
FULL_FINGERPRINT=$(gpg --list-secret-keys --with-colons | grep "^fpr" | tail -1 | cut -d: -f10)
KEY_ID=${FULL_FINGERPRINT: -16}  # Last 16 characters

if [ -z "$KEY_ID" ]; then
    echo "Error: Could not find the key ID. Key generation may have failed."
    exit 1
fi

echo "Your GPG key ID is: $KEY_ID"
echo "Full fingerprint: $FULL_FINGERPRINT"
echo

# Export public key for GitHub
echo "Exporting public key..."
gpg --armor --export $KEY_ID > "${KEY_ID}.pub.asc"
echo "Public key exported to ${KEY_ID}.pub.asc"
echo

# Export private key for backup and GitHub Actions
echo "Exporting private key (KEEP THIS SECURE!)..."
echo "You will need to enter your key passphrase."
gpg --armor --export-secret-keys $KEY_ID > "${KEY_ID}.private.asc"
echo "Private key exported to ${KEY_ID}.private.asc"
echo
echo "⚠️  WARNING: This file contains your private key. Keep it secure! ⚠️"
echo

# Instructions for GitHub
echo "=== NEXT STEPS ==="
echo
echo "1. Add your public key to GitHub:"
echo "   - Visit https://github.com/settings/keys"
echo "   - Click 'New GPG key'"
echo "   - Copy the contents of ${KEY_ID}.pub.asc"
echo
echo "2. Set up GitHub repository secrets for GitHub Actions:"
echo "   - Go to your repository settings → Secrets → Actions"
echo "   - Add a new secret named 'GPG_PRIVATE_KEY' with contents of ${KEY_ID}.private.asc"
echo "   - Add a new secret named 'GPG_PASSPHRASE' with your key passphrase"
echo "   - Add a new secret named 'GPG_KEY_ID' with value: $KEY_ID"
echo
echo "3. Configure Git to sign commits/tags with this key:"
echo "   git config --global user.signingkey $KEY_ID"
echo "   git config --global commit.gpgsign true"
echo "   git config --global tag.gpgsign true"
echo
echo "Now you're ready to sign releases of Time Whisperer!"
echo
echo "Users will be able to verify your signatures by importing your key from:"
echo "  - GitHub: curl -s https://github.com/hyperbach-git.gpg | gpg --import"
echo "  - Release assets: gpg --import hyperbach-public-key.asc" 