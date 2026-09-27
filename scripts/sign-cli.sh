#!/bin/bash
set -euo pipefail

# Sign only after platform signing has finalized the executable bytes.
: "${CLI_SIGNING_KEY:?CLI_SIGNING_KEY is required}"
CLI_DIST="${CLI_DIST:-dist/cli}"
CLI_KEY_DIR="$(mktemp -d)"
trap 'rm -rf "$CLI_KEY_DIR"' EXIT
umask 077
printf '%s' "$CLI_SIGNING_KEY" > "$CLI_KEY_DIR/private.pem"
unset CLI_SIGNING_KEY
openssl rsa -in "$CLI_KEY_DIR/private.pem" -RSAPublicKey_out -outform DER \
  -out "$CLI_KEY_DIR/public.der" 2>/dev/null
cmp "$CLI_KEY_DIR/public.der" crates/updater/src/cli/signing-key.der
for manifest in "$CLI_DIST"/request-eagle-cli-*.json; do
  openssl dgst -sha256 -sign "$CLI_KEY_DIR/private.pem" \
    -out "${manifest%.json}.sig" "$manifest"
done
