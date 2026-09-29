#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${VERSION:?VERSION is required}"
: "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"
: "${APPLE_ID:?APPLE_ID is required}"
: "${APPLE_APP_SPECIFIC_PASSWORD:?APPLE_APP_SPECIFIC_PASSWORD is required}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required}"

DIST_DIR="${DIST_DIR:-$ROOT/dist}"
APP="$DIST_DIR/Request Eagle.app"
ZIP="$DIST_DIR/RequestEagle-$VERSION-arm64.zip"
DMG="$DIST_DIR/RequestEagle-$VERSION-arm64.dmg"
NOTARY_ZIP="$DIST_DIR/RequestEagle-notarization.zip"

test -d "$APP"

rm -f "$ZIP" "$DMG" "$NOTARY_ZIP"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP" "$NOTARY_ZIP"

xcrun notarytool submit "$NOTARY_ZIP" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"

DMG_SOURCE="$(mktemp -d)"
trap 'rm -rf "$DMG_SOURCE" "$NOTARY_ZIP"' EXIT
cp -R "$APP" "$DMG_SOURCE/Request Eagle.app"
ln -s /Applications "$DMG_SOURCE/Applications"
hdiutil create \
  -volname "Request Eagle $VERSION" \
  -srcfolder "$DMG_SOURCE" \
  -ov \
  -format UDZO \
  "$DMG"

/usr/bin/codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

echo "Packaged $ZIP and $DMG"
