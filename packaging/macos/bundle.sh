#!/usr/bin/env bash
# IAP macOS-Bundle-Bauer (Konzept 16, arm64 + x86_64).
#
# Erzeugt ein doppelklick-fähiges .app-Bundle für beide Architekturen.
# **Kein Notarize-, kein Codesign-Schritt** — Konzept 16 sagt explizit,
# dass der Gatekeeper-Schritt in der LIESMICH-Datei dokumentiert wird,
# statt umgangen zu werden. Nutzer müssen beim ersten Start selbst
# freischalten.

set -euo pipefail

if [[ "${TRACE:-0}" == "1" ]]; then
  set -x
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST_ROOT="$REPO_ROOT/dist/iap-macos"
APP_DIR="$DIST_ROOT/IAP.app"

ARM_BINARY="${1:-$REPO_ROOT/target/aarch64-apple-darwin/release/iap}"
X64_BINARY="${2:-$REPO_ROOT/target/x86_64-apple-darwin/release/iap}"

if [[ ! -x "$ARM_BINARY" ]]; then
  echo "Fehler: arm64-Binary nicht gefunden ($ARM_BINARY)" >&2
  echo "Baue mit: cargo build --release --target aarch64-apple-darwin" >&2
  exit 2
fi
if [[ ! -x "$X64_BINARY" ]]; then
  echo "Fehler: x86_64-Binary nicht gefunden ($X64_BINARY)" >&2
  echo "Baue mit: cargo build --release --target x86_64-apple-darwin" >&2
  exit 2
fi

echo "==> Räume $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

echo "==> Baue Universal-Binary mit lipo"
lipo -create -output "$APP_DIR/Contents/MacOS/iap" "$ARM_BINARY" "$X64_BINARY"
chmod +x "$APP_DIR/Contents/MacOS/iap"

echo "==> Info.plist"
cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>IAP</string>
  <key>CFBundleDisplayName</key><string>IAP</string>
  <key>CFBundleIdentifier</key><string>local.iap</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleExecutable</key><string>iap</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

echo "==> Fertig: $APP_DIR"
echo
echo "Bitte $DIST_ROOT/LIESMICH.txt beilegen (Gatekeeper-Anleitung)."
