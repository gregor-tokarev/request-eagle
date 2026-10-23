#!/bin/bash
set -euo pipefail

# Build the app once and package it for Linux into dist/:
#   RequestEagle-<version>-x86_64.AppImage        any distribution
#   request-eagle_<version>_amd64.deb             Debian and Ubuntu
#   request-eagle-<version>-1.x86_64.rpm          Fedora, openSUSE and other RPM distributions
#   request-eagle-<version>-x86_64-linux.tar.gz   the AUR package and other distributions
#
# Needs dpkg-dev, rpm, file and curl. The AppImage tools are downloaded on
# first use.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(sed -n '/^name = "request-eagle"$/{n;s/version = "\([^"]*\)"/\1/p;q;}' "$ROOT/crates/request-eagle/Cargo.toml")}"
DIST_DIR="${DIST_DIR:-$ROOT/dist}"
PROFILE="${PROFILE:-dist}"
APP_ID=com.egortokarev.requesteagle

# Pinned from https://github.com/AppImage/appimagetool and
# https://github.com/AppImage/type2-runtime. The runtime needs no FUSE 2.
APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
APPIMAGE_RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
APPIMAGE_RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

test "$(uname -m)" = x86_64

cd "$ROOT"
cargo build --locked --profile "$PROFILE" -p request-eagle
BINARY="target/$PROFILE/request-eagle"
[[ "$PROFILE" != dev ]] || BINARY=target/debug/request-eagle

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
root="$staging/root"
mkdir -p "$DIST_DIR"

# The updater recognizes a packaged install by this path.
install -Dm755 "$BINARY" "$root/usr/bin/request-eagle"
strip --strip-unneeded "$root/usr/bin/request-eagle"
install -Dm644 "packaging/linux/$APP_ID.desktop" "$root/usr/share/applications/$APP_ID.desktop"
install -Dm644 "packaging/linux/$APP_ID.png" "$root/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
install -Dm644 "packaging/linux/$APP_ID.metainfo.xml" "$root/usr/share/metainfo/$APP_ID.metainfo.xml"

# Download a pinned tool once, into the build directory.
fetch_tool() {
  local url="$1" sha256="$2" file
  file="target/appimage-tools/$(basename "$url")"

  if [[ ! -f "$file" ]]; then
    mkdir -p "$(dirname "$file")"
    curl -sSfL --retry 3 -o "$file" "$url"
    chmod 755 "$file"
  fi

  if [[ "$(sha256sum "$file" | cut -d ' ' -f 1)" != "$sha256" ]]; then
    echo "$file does not match its pinned checksum; delete it to download it again" >&2
    return 1
  fi

  echo "$file"
}

# The AppImage carries the same files. Its runtime tells the app where the
# image is, so the updater needs no marker.
appimagetool="$(fetch_tool "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256")"
runtime="$(fetch_tool "$APPIMAGE_RUNTIME_URL" "$APPIMAGE_RUNTIME_SHA256")"
appdir="$staging/AppDir"
cp -a "$root" "$appdir"
ln -s usr/bin/request-eagle "$appdir/AppRun"
ln -s "usr/share/applications/$APP_ID.desktop" "$appdir/$APP_ID.desktop"
ln -s "usr/share/icons/hicolor/512x512/apps/$APP_ID.png" "$appdir/$APP_ID.png"
ln -s "$APP_ID.png" "$appdir/.DirIcon"
APPIMAGE="$DIST_DIR/RequestEagle-$VERSION-x86_64.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$appimagetool" --runtime-file "$runtime" "$appdir" "$APPIMAGE" > /dev/null
echo "Packaged $APPIMAGE"

# The tarball is what the AUR package installs from.
tarball_root="$staging/request-eagle-$VERSION"
mkdir -p "$tarball_root"
cp "$root/usr/bin/request-eagle" "packaging/linux/$APP_ID."{desktop,png,metainfo.xml} LICENSE-MIT LICENSE-APACHE "$tarball_root/"
chmod -R go-w "$tarball_root"
TARBALL="$DIST_DIR/request-eagle-$VERSION-x86_64-linux.tar.gz"
tar -czf "$TARBALL" -C "$staging" --owner=0 --group=0 "request-eagle-$VERSION"
echo "Packaged $TARBALL"

# The Debian and RPM packages name themselves, so the updater offers the
# right one. Read in crates/updater/src/package.rs.
marker="$root/usr/share/request-eagle/package"
install -Dm644 /dev/null "$marker"
echo debian > "$marker"

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
Architecture: $(dpkg --print-architecture)
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

DEB="$DIST_DIR/request-eagle_${VERSION}_$(dpkg --print-architecture).deb"
dpkg-deb --root-owner-group --build "$root" "$DEB" > /dev/null
rm -r "$root/DEBIAN"
echo "Packaged $DEB"

echo rpm > "$marker"
rpmbuild -bb --quiet \
  --define "_topdir $staging/rpm" \
  --define "_dbpath $staging/rpm/db" \
  --define "version $VERSION" \
  --define "staged_root $root" \
  --buildroot "$staging/rpm/buildroot" \
  packaging/linux/request-eagle.spec
RPM="$DIST_DIR/request-eagle-$VERSION-1.x86_64.rpm"
mv "$staging/rpm/RPMS/x86_64/request-eagle-$VERSION-1.x86_64.rpm" "$RPM"
echo "Packaged $RPM"
