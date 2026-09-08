#!/usr/bin/env bash
# Assemble DB Hive.app from the release binary.
#
# Tauri produced the bundle for us before; a plain cargo binary needs the
# .app laid out by hand. This is deliberately a script rather than a build
# dependency: it is a packaging step, not part of `cargo build`.
#
#   ./packaging/build-macos-app.sh [--sign "Developer ID Application: ..."]
#
# CODE SIGNING MATTERS HERE, and not only for distribution. Connection
# passwords live in the OS keychain, and macOS grants keychain items to a
# stable code identity. An unsigned binary is a different identity every
# time it is rebuilt, so it cannot read back what a previous build stored:
# the app will prompt for passwords it already saved. Signing with a stable
# identity, even an ad-hoc one for local use, fixes that.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="DB Hive"
BUNDLE_ID="com.kwamina.db-hive"
VERSION="$(grep -m1 '^version' "$ROOT/crates/dbhive-app/Cargo.toml" | cut -d'"' -f2)"

SIGN_IDENTITY="-"   # ad-hoc by default; stable enough for the keychain locally
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN_IDENTITY="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

echo "==> building release binary"
cargo build --release -p dbhive-app --manifest-path "$ROOT/Cargo.toml"

BIN="$ROOT/target/release/dbhive"
[[ -x "$BIN" ]] || { echo "binary not found at $BIN" >&2; exit 1; }

APP="$ROOT/target/release/bundle/$APP_NAME.app"
echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/dbhive"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key><string>dbhive</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>icon.icns</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

ICON_SRC="$ROOT/src-tauri/icons/icon.icns"
if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$APP/Contents/Resources/icon.icns"
else
  echo "    note: no icon.icns found, the bundle will use the generic icon"
fi

echo "==> signing with identity: $SIGN_IDENTITY"
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --verbose=1 "$APP" 2>&1 | sed 's/^/    /'

SIZE="$(du -sh "$APP" | cut -f1)"
echo "==> done: $APP ($SIZE)"
