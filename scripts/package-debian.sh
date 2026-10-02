#!/bin/bash
set -euo pipefail

# Build the app and package it for Debian and Ubuntu as
# dist/request-eagle_<version>_<architecture>.deb.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(sed -n '/^name = "request-eagle"$/{n;s/version = "\([^"]*\)"/\1/p;q;}' "$ROOT/crates/request-eagle/Cargo.toml")}"
DIST_DIR="${DIST_DIR:-$ROOT/dist}"
PROFILE="${PROFILE:-dist}"
ARCHITECTURE="$(dpkg --print-architecture)"
PACKAGE="$DIST_DIR/request-eagle_${VERSION}_${ARCHITECTURE}.deb"
APP_ID=com.egortokarev.requesteagle

cd "$ROOT"
cargo build --locked --profile "$PROFILE" -p request-eagle
BINARY="target/$PROFILE/request-eagle"
[[ "$PROFILE" != dev ]] || BINARY=target/debug/request-eagle

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
root="$staging/root"

# The updater recognizes a packaged install by this path.
install -Dm755 "$BINARY" "$root/usr/bin/request-eagle"
strip --strip-unneeded "$root/usr/bin/request-eagle"
install -Dm644 "packaging/debian/$APP_ID.desktop" "$root/usr/share/applications/$APP_ID.desktop"
install -Dm644 "packaging/debian/$APP_ID.png" "$root/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"

# dpkg-shlibdeps reads the package name from a source control file.
mkdir -p "$staging/debian"
printf 'Source: request-eagle\n\nPackage: request-eagle\nArchitecture: any\n' > "$staging/debian/control"
libraries="$(cd "$staging" && dpkg-shlibdeps -O -e"$root/usr/bin/request-eagle" | sed -n 's/^shlibs:Depends=//p')"

# Vulkan, the GPU drivers and the Wayland client library are loaded at run
# time, so dpkg-shlibdeps cannot see them.
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CONTROL
Package: request-eagle
Version: $VERSION
Architecture: $ARCHITECTURE
Maintainer: Gregor Tokarev <gregor-tokarev@users.noreply.github.com>
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Depends: $libraries, libvulkan1, libwayland-client0
Recommends: mesa-vulkan-drivers
Section: devel
Priority: optional
Homepage: https://requesteagle.tokarev.work
Description: Fast, native API client
 Request Eagle is a Postman alternative written in Rust and rendered on the
 GPU. It keeps requests as TOML files on disk, with scripts, Vim mode and a
 CLI for agents.
CONTROL

mkdir -p "$DIST_DIR"
dpkg-deb --root-owner-group --build "$root" "$PACKAGE"
echo "Packaged $PACKAGE"
