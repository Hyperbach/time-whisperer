# Verifying Time Whisperer Releases

This document explains how to verify that a Time Whisperer binary you downloaded is authentic and unmodified.

## Why Verify?

Verifying software ensures that the binary you're running:
1. Was built from the exact source code in the repository
2. Hasn't been tampered with or modified
3. Is signed by the official Time Whisperer developers

## Quick Verification

Each release includes three files for each binary:
- The binary itself (e.g., `timewhisperer-macos-amd64`)
- A SHA256 hash file (e.g., `timewhisperer-macos-amd64.sha256`)
- A GPG signature of the hash file (e.g., `timewhisperer-macos-amd64.sha256.asc`)

### Using the Included Verification Script

The easiest way to verify your download is to use the included verification script:

**On macOS:**
```bash
/Applications/TimeWhisperer.app/Contents/MacOS/verify-binary
```

**On Linux:**
```bash
/usr/lib/time-whisperer/verify-binary /usr/bin/time-whisperer
```

## Manual Verification (Step by Step)

If you prefer to verify manually:

### 1. Verify the SHA256 hash

**On macOS:**
```bash
shasum -a 256 -c timewhisperer-macos-amd64.sha256
```

**On Linux:**
```bash
sha256sum -c timewhisperer-linux-amd64.sha256
```

You should see: `timewhisperer-*: OK`

### 2. Import the GPG public key

Choose one of these methods to import the Time Whisperer public key:

#### Method A: Import from GitHub (Recommended)
```bash
# Download the public key directly from GitHub
curl -s https://github.com/hyperbach-git.gpg | gpg --import
```

#### Method B: Import from release assets
```bash
# Download and import the public key from release assets
gpg --import hyperbach-public-key.asc
```

### 3. Verify the GPG signature

Then verify the signature:

```bash
gpg --verify timewhisperer-*.sha256.asc timewhisperer-*.sha256
```

You should see: `Good signature from "Hyperbach (Time Whisperer Release Signing Key) <root@hyperbach.com>"`

## Advanced Verification: Reproducible Build

For maximum confidence, you can rebuild from source and verify that the binary matches:

1. Install Nix (if not already installed):
   ```bash
   curl -L https://nixos.org/nix/install | sh
   ```

2. Clone the repository:
   ```bash
   git clone https://github.com/Hyperbach/time-whisperer.git
   cd time-whisperer
   ```

3. Check out the exact tag for the version you want to verify:
   ```bash
   git checkout v1.0.0  # Replace with your version
   ```

4. Build using Nix:
   ```bash
   nix build
   ```

5. Compare the result with your downloaded binary:
   ```bash
   # Using cmp for a binary comparison:
   cmp ./result/bin/timewhisperer /path/to/downloaded/timewhisperer
   
   # No output means the files are identical
   ```

## What If Verification Fails?

If verification fails:

1. Make sure you downloaded files from the official GitHub releases page
2. Check if you're using the correct files for verification
3. Ensure your GPG keyring has the correct public key
4. Try importing the key using a different method (see Method A or B above)
5. If problems persist, please open an issue on GitHub 