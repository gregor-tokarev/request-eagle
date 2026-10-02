#!/bin/bash
set -euo pipefail

# Describe one platform's release file for the app's updater:
#   write-update-manifest.sh <release file> <manifest>

: "${VERSION:?VERSION is required}"
: "${TAG:?TAG is required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

file="$1"
manifest="$2"

if command -v sha256sum >/dev/null; then
  sha256="$(sha256sum "$file" | cut -d ' ' -f 1)"
else
  sha256="$(shasum -a 256 "$file" | cut -d ' ' -f 1)"
fi

printf '{"version":"%s","url":"https://github.com/%s/releases/download/%s/%s","sha256":"%s"}\n' \
  "$VERSION" "$GITHUB_REPOSITORY" "$TAG" "$(basename "$file")" "$sha256" > "$manifest"
