#!/bin/bash
set -euo pipefail

# CLI artifacts are release attachments, never files inside the desktop bundle.
: "${VERSION:?VERSION is required}"
: "${CLI_TARGET:?CLI_TARGET is required}"
CLI_DIST="${CLI_DIST:-dist/cli}"

mkdir -p "$CLI_DIST"
REQUEST_EAGLE_RELEASE_VERSION="$VERSION" cargo build --locked --release -p request-eagle-cli --target "$CLI_TARGET"
CLI_BINARY="$CLI_DIST/request-eagle-cli-$CLI_TARGET"
cp "target/$CLI_TARGET/release/request-eagle-cli" "$CLI_BINARY"
chmod 755 "$CLI_BINARY"

if [[ "$CLI_TARGET" == aarch64-apple-darwin ]]; then
  : "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"
  : "${APPLE_ID:?APPLE_ID is required}"
  : "${APPLE_APP_SPECIFIC_PASSWORD:?APPLE_APP_SPECIFIC_PASSWORD is required}"
  : "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required}"

  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$CLI_BINARY"
  codesign --verify --strict "$CLI_BINARY"

  CLI_NOTARY="$CLI_DIST/cli-notarization.zip"
  ditto -c -k "$CLI_BINARY" "$CLI_NOTARY"
  xcrun notarytool submit "$CLI_NOTARY" \
    --apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait
  rm "$CLI_NOTARY"
fi
