# Verifying Worklog / Time Whisperer releases

This explains how to confirm that a download from the
[Releases](https://github.com/Hyperbach/time-whisperer/releases) page is
authentic and unmodified.

## What's published

Each release includes, with a `.sha256` sidecar for every file:

- `Worklog-<version>-macos-arm64.pkg` — macOS installer (Developer ID signed + notarized)
- `Worklog-<version>-macos-arm64.dmg` — macOS drag-to-Applications image (signed + notarized)
- `time-whisperer-linux-amd64` — Linux daemon binary

## 1. Verify the SHA-256 hash (all platforms)

**macOS**
```bash
shasum -a 256 -c Worklog-1.0.3-macos-arm64.pkg.sha256
```

**Linux**
```bash
sha256sum -c time-whisperer-linux-amd64.sha256
```

You should see `… OK`.

## 2. Verify the macOS signature & notarization

The macOS artifacts are signed with Apple **Developer ID** and **notarized** by
Apple, under the team **Hyperbach Services OÜ (Team ID `9QNDK63AV5`)**. You don't
need any key — Gatekeeper checks this automatically on first open — but you can
verify explicitly:

**The installer `.pkg`**
```bash
pkgutil --check-signature Worklog-1.0.3-macos-arm64.pkg
# Expect a valid chain ending in:
#   Developer ID Installer: Hyperbach Services OU (9QNDK63AV5)

xcrun stapler validate Worklog-1.0.3-macos-arm64.pkg
# Expect: The validate action worked!
```

**The `.dmg`**
```bash
xcrun stapler validate Worklog-1.0.3-macos-arm64.dmg
spctl -a -t open --context context:primary-signature -vv Worklog-1.0.3-macos-arm64.dmg
# Expect: accepted / source=Notarized Developer ID
```

**The installed app**
```bash
codesign -dv --verbose=4 /Applications/Worklog.app
#   Authority=Developer ID Application: Hyperbach Services OU (9QNDK63AV5)
#   TeamIdentifier=9QNDK63AV5

spctl -a -vvv /Applications/Worklog.app
#   accepted, source=Notarized Developer ID
```

If those checks pass, the build came from this repository's signing identity and
was notarized by Apple — it has not been tampered with.

## 3. The Linux binary

The Linux binary is verified by its SHA-256 hash (step 1). It is not code-signed
(Linux has no equivalent system trust mechanism); rebuild from source if you want
end-to-end assurance — see below.

## Rebuild from source

For maximum confidence, build the same tag yourself with a stable Rust toolchain:

```bash
git clone https://github.com/Hyperbach/time-whisperer.git
cd time-whisperer
git checkout v1.0.3            # the version you're verifying
cargo build --release --locked
# target/release/time-whisperer
```

`--locked` builds against the committed `Cargo.lock`, so dependency versions match
exactly what CI used. (Note: the CI release artifacts are signed/stapled, so they
are not byte-identical to a local build — compare behavior and provenance, not raw
bytes.)

## If verification fails

1. Confirm you downloaded from the official Releases page above.
2. Re-download — a partial/corrupted download fails the hash check.
3. For macOS signature checks, make sure you're on the actual file (not a copy
   stripped of extended attributes by some transfer tools).
4. If problems persist, open an issue on GitHub.
