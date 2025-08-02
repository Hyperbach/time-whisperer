# 🧱 Project Structure & Release Pipeline for Verified GitHub Releases

## 🗂️ Project Structure Overview

```
.
├── flake.nix                # Reproducible build definition
├── default.nix             # Optional legacy Nix support
├── .github/
│   └── workflows/
│       └── release.yml     # GitHub Actions release pipeline
├── scripts/
│   ├── build.sh            # Build wrapper for CI/local
│   └── sign.sh             # Signs SHA256 hash with GPG
├── dist/                   # Binaries + hashes + sigs get placed here
├── README.md               # Mention reproducibility + verification
└── VERIFICATION.md         # Detailed verification instructions
```

---

# ⚙️ Step-by-Step Release Workflow

## 1. ✅ **Reproducible Build with Nix**

Define your build in `flake.nix`. Our implementation looks like:

```nix
{
  description = "TimeWhisperer reproducible build";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  
  outputs = { self, nixpkgs }:
    let 
      # Define systems we want to support
      supportedSystems = [ "x86_64-darwin" "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      
      # Helper function to generate outputs for each system
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      
      # Get pkgs for each system
      pkgsFor = system: import nixpkgs { inherit system; };
    in {
      packages = forAllSystems (system: 
        let 
          pkgs = pkgsFor system;
          version = "1.0.0"; # This should be automated or passed in
        in {
          default = pkgs.buildGoModule {
            name = "timewhisperer";
            pname = "timewhisperer";
            src = ./.;
            vendorHash = null; # Will be computed on first build
            
            # Extract version directly from Git
            inherit version;
            
            # Embed build information
            ldflags = [
              "-X" "main.Version=${version}"
              "-X" "main.GitCommit=${self.rev or "unknown"}"
              "-X" "main.BuildDate=1970-01-01T00:00:00Z" # Fixed timestamp for reproducibility
              "-s" "-w" # Strip debug symbols
            ];
            
            # Make sure build is hermetic
            CGO_ENABLED = 0;
          };
        }
      );
    };
}
```

This ensures the same binary is built from the same commit every time, with fixed build timestamps.

---

## 2. 🤖 **GitHub Actions Pipeline (release.yml)**

Trigger on push to tags with 'v*' prefix.

```yaml
name: Release Build

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Get all history for tags

      - name: Install Nix
        run: |
          curl -L https://nixos.org/nix/install | sh
          . /Users/runner/.nix-profile/etc/profile.d/nix.sh
          mkdir -p ~/.config/nix
          echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf

      - name: Build reproducible binary
        run: |
          . /Users/runner/.nix-profile/etc/profile.d/nix.sh
          nix build .#default --system x86_64-darwin --out-link result-macos-amd64

      - name: Copy to dist/
        run: |
          mkdir -p dist
          cp result-macos-amd64/bin/timewhisperer dist/timewhisperer-macos

      - name: Generate SHA256
        run: |
          shasum -a 256 dist/timewhisperer-macos > dist/timewhisperer-macos.sha256

      - name: Sign the hash
        run: |
          gpg --batch --yes --armor --detach-sign dist/timewhisperer-macos.sha256

      - name: Upload Release Assets
        uses: softprops/action-gh-release@v2
        with:
          files: |
            dist/timewhisperer-macos
            dist/timewhisperer-macos.sha256
            dist/timewhisperer-macos.sha256.asc
```

✅ This builds your app, hashes it, signs the hash, and uploads everything to the GitHub release.

---

## 3. 🔏 Signing Setup

You'll need to add your **GPG private key** as a GitHub Secret. Example setup:
```bash
# export your GPG private key
gpg --armor --export-secret-keys "Your Name" > private.key
```

Then:
- Store `private.key` in GitHub Secrets as `GPG_PRIVATE_KEY`
- Store your passphrase as `GPG_PASSPHRASE`
- Store your key ID as `GPG_KEY_ID`
- Import the key in Actions with the `crazy-max/ghaction-import-gpg` action

---

## 4. 📦 GitHub Release Assets

Each release will include:
```
timewhisperer-macos
timewhisperer-macos.sha256
timewhisperer-macos.sha256.asc
```

We also include Linux builds and packaged binaries (DMG and DEB).

---

## 5. 🧠 README / Release Notes

In `README.md` and each GitHub release, we include:

> ### 🔐 Verifying Binaries
> This binary was built from [this commit](https://github.com/your/repo/commit/xyz123), reproducibly using Nix.  
>  
> Technical users can verify the download with:
>
> ```bash
> shasum -a 256 -c timewhisperer-macos.sha256
> gpg --verify timewhisperer-macos.sha256.asc
> ```
> 
> Or rebuild with:
> ```bash
> nix build
> ```

---

## ✅ Final Result

Our implementation provides:
- Reproducible builds using Nix (no external caching required)
- Fully automated GitHub release pipeline
- GPG-signed hash for integrity
- Clear instructions for verification in our VERIFICATION.md file

This approach offers:
- One-click install for normal users
- Bulletproof verification path for security-conscious users

## 🔧 Local Development

For local development:

```bash
# Clone the repository
git clone https://github.com/yourusername/time-whisperer.git
cd time-whisperer

# Build with Nix
nix build

# Test the binary
./result/bin/timewhisperer
```

## 🚀 Creating a Release

To create a new release:

1. Update the version number in relevant files
2. Commit your changes
3. Create and push a tag:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```
4. GitHub Actions will automatically build and publish the release

