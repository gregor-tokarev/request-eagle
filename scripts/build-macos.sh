#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(sed -n '/^name = "request-eagle"$/{n;s/version = "\([^"]*\)"/\1/p;q;}' "$ROOT/crates/request-eagle/Cargo.toml")}"
BUILD_VERSION="${BUILD_VERSION:-${VERSION//[^0-9]/}}"
BUILD_VERSION="${BUILD_VERSION:-1}"
DIST_DIR="${DIST_DIR:-$ROOT/dist}"
APP="$DIST_DIR/Request Eagle.app"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
TARGET="${TARGET:-aarch64-apple-darwin}"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

rustup target add "$TARGET"
cargo build --locked --profile dist -p request-eagle --target "$TARGET"
cp "$ROOT/target/$TARGET/dist/request-eagle" "$APP/Contents/MacOS/request-eagle"

chmod 755 "$APP/Contents/MacOS/request-eagle"
cp "$ROOT/packaging/macos/AppIconEagleV2.icns" "$APP/Contents/Resources/AppIconEagleV2.icns"
sed \
  -e "s/__VERSION__/$VERSION/g" \
  -e "s/__BUILD_VERSION__/$BUILD_VERSION/g" \
  "$ROOT/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

/usr/bin/codesign \
  --force \
  --options runtime \
  --timestamp \
  --entitlements "$ROOT/packaging/macos/entitlements.plist" \
  --sign "$SIGN_IDENTITY" \
  "$APP"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$APP"

echo "Built $APP"
