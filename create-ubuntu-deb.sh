#!/bin/bash

set -e

echo "Creating Ubuntu .deb package for SneakTime..."
echo "================================================="

# Configuration
APP_NAME="time-whisperer"
VERSION="${VERSION:-1.0.0}"  # Use env var if set, otherwise default to 1.0.0
PACKAGE_NAME="${APP_NAME}_${VERSION}_amd64"
BINARY_NAME="time-whisperer"
BINARY_PATH="${BINARY_PATH:-${BINARY_NAME}}"  # Allow override for Nix-built binaries

echo "Building version ${VERSION}"
echo "Using binary from: ${BINARY_PATH}"

# Create necessary directories
mkdir -p build/deb-package/${PACKAGE_NAME}/DEBIAN
mkdir -p build/deb-package/${PACKAGE_NAME}/usr/bin
mkdir -p build/deb-package/${PACKAGE_NAME}/etc/systemd/user
mkdir -p build/deb-package/${PACKAGE_NAME}/usr/share/doc/${APP_NAME}
mkdir -p build/deb-package/${PACKAGE_NAME}/usr/lib/${APP_NAME}

# Build the binary (if not exists and BINARY_PATH not specified)
if [ ! -f "${BINARY_PATH}" ] && [ "${BINARY_PATH}" = "${BINARY_NAME}" ]; then
    echo "Building ${BINARY_NAME}..."
    go build -o ${BINARY_NAME} main.go
fi

# Copy binary to package location
cp "${BINARY_PATH}" "build/deb-package/${PACKAGE_NAME}/usr/bin/${BINARY_NAME}"
chmod 755 "build/deb-package/${PACKAGE_NAME}/usr/bin/${BINARY_NAME}"

# Copy verification script
if [ -f "scripts/release/verify.sh" ]; then
    cp "scripts/release/verify.sh" "build/deb-package/${PACKAGE_NAME}/usr/lib/${APP_NAME}/verify-binary"
    chmod +x "build/deb-package/${PACKAGE_NAME}/usr/lib/${APP_NAME}/verify-binary"
fi

# Create service unit file
cat > build/deb-package/${PACKAGE_NAME}/etc/systemd/user/time-whisperer.service << EOF
[Unit]
Description=SneakTime - Upwork Screenshot Monitor
After=network.target

[Service]
ExecStart=/usr/bin/time-whisperer
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
EOF

# Create control file
cat > build/deb-package/${PACKAGE_NAME}/DEBIAN/control << EOF
Package: ${APP_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: amd64
Depends: libc6
Maintainer: HyperBach <contact@hyperbach.com>
Description: SneakTime - Upwork Screenshot Monitor
 A utility that monitors and alerts you about Upwork screenshots.
 Helps freelancers be aware when screenshots are taken.
 .
 This package was built reproducibly with Nix for verification.
EOF

# Create postinst script
cat > build/deb-package/${PACKAGE_NAME}/DEBIAN/postinst << EOF
#!/bin/bash

# Set permissions
chmod 755 /usr/bin/time-whisperer
chmod 755 /usr/lib/${APP_NAME}/verify-binary

# Create config directory in user home directories
for user_home in /home/*; do
  if [ -d "\${user_home}" ]; then
    username=\$(basename "\${user_home}")
    user_config_dir="\${user_home}/.config/time-whisperer"
    
    # Create config directory
    mkdir -p "\${user_config_dir}"
    chown "\${username}:\${username}" "\${user_config_dir}"
    
    # Inform the user about the service
    echo "To enable SneakTime for user \${username}, run:"
    echo "  systemctl --user enable time-whisperer.service"
    echo "  systemctl --user start time-whisperer.service"
  fi
done

echo "SneakTime has been installed!"
echo "To start the service, each user should run:"
echo "  systemctl --user enable time-whisperer.service"
echo "  systemctl --user start time-whisperer.service"
echo ""
echo "To verify the binary's authenticity, run:"
echo "  /usr/lib/${APP_NAME}/verify-binary /usr/bin/time-whisperer"

exit 0
EOF

# Make scripts executable
chmod +x build/deb-package/${PACKAGE_NAME}/DEBIAN/postinst

# Create README file
cat > build/deb-package/${PACKAGE_NAME}/usr/share/doc/${APP_NAME}/README << EOF
SneakTime - Upwork Screenshot Monitor
=========================================

Version: ${VERSION}

Usage:
1. Run '${APP_NAME}' from your terminal or application menu

To start SneakTime at login:
1. Run: systemctl --user enable time-whisperer.service
2. Run: systemctl --user start time-whisperer.service

To verify the binary's authenticity:
1. Run: /usr/lib/${APP_NAME}/verify-binary /usr/bin/${APP_NAME}

Environment Variables:
- UPWORK_LOG_DIR: Override the default Upwork log directory path

For more information, visit: https://github.com/yourusername/time-whisperer
EOF

# Create VERIFICATION.txt file
cat > build/deb-package/${PACKAGE_NAME}/usr/share/doc/${APP_NAME}/VERIFICATION << EOF
VERIFICATION INSTRUCTIONS
========================

The binary in this package was reproducibly built using Nix.

To verify the authenticity of the binary:

1. Run the included verification script:
   /usr/lib/${APP_NAME}/verify-binary /usr/bin/${APP_NAME}

2. Or manually verify with:
   - Download SHA256 hash and GPG signature from GitHub releases
   - Verify hash: sha256sum -c /path/to/timewhisperer-*.sha256
   - Verify signature: gpg --verify /path/to/timewhisperer-*.sha256.asc /path/to/timewhisperer-*.sha256

3. Advanced verification by building yourself:
   - Clone the repository: git clone https://github.com/yourusername/time-whisperer.git
   - Check out tag: git checkout v${VERSION}
   - Install Nix: curl -L https://nixos.org/nix/install | sh
   - Build: nix build
   - Compare binaries: cmp ./result/bin/timewhisperer /usr/bin/${APP_NAME}
EOF

# Create copyright file (required for Debian packages)
cat > build/deb-package/${PACKAGE_NAME}/usr/share/doc/${APP_NAME}/copyright << EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: ${APP_NAME}
Source: https://github.com/yourusername/time-whisperer

Files: *
Copyright: $(date +%Y) HyperBach
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in all
 copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 SOFTWARE.
EOF

# Build the package
echo "Building .deb package..."
dpkg-deb --build --root-owner-group build/deb-package/${PACKAGE_NAME}

# Move the package to the current directory
mv build/deb-package/${PACKAGE_NAME}.deb .

echo ".deb package created: ${PACKAGE_NAME}.deb"
echo "Install with: sudo dpkg -i ${PACKAGE_NAME}.deb" 