# Releases

Request Eagle ships on two update tracks. The app follows the one chosen in
Settings > General > Update track, which is stable unless changed.

| Track | What it gets | How a release gets there |
| --- | --- | --- |
| Nightly | Every build, most days one from `main` | The Nightly release workflow, on its own |
| Stable | Builds that worked out as nightly builds | Promote release, run by hand |

Both tracks share one version sequence. Every release is published as a GitHub
pre-release, which nightly installs update to. Promoting one makes it GitHub's
latest release, which stable installs update to. Switching from nightly to
stable keeps the installed build until a newer one is promoted.

## Nightly releases

`.github/workflows/daily-release.yml` runs every morning at 06:17 UTC. When
`main` has changed since the last release, `scripts/prepare-daily-release.sh`
bumps the patch version, commits and tags it, and the Release workflow builds
and publishes it as a pre-release. Run the workflow by hand to release sooner.
Pushing a `vX.Y.Z` tag whose version matches `crates/request-eagle/Cargo.toml`
also publishes a pre-release.

## Stable releases

Run **Promote release** (`.github/workflows/promote-release.yml`) with a
pre-release tag, or with no tag for the newest pre-release. It marks that
release as the latest release, without rebuilding it, and rewrites its notes to
list everything merged since the previous stable release. With no tag, it does
nothing when the stable release is already the newest.

## What a release contains

`.github/workflows/release.yml` builds every platform from the tag:

| Platform | Files | Updates |
| --- | --- | --- |
| macOS (Apple Silicon) | `RequestEagle-X.Y.Z-arm64.dmg` and `.zip`, signed and notarized | In the app, from the ZIP |
| Windows (x64) | `RequestEagle-X.Y.Z-x64-setup.exe`, a per-user installer | The browser downloads the installer |
| Linux (x64), built on Debian 12 | `RequestEagle-X.Y.Z-x86_64.AppImage` for any distribution, `request-eagle_X.Y.Z_amd64.deb` for Debian and Ubuntu, `request-eagle-X.Y.Z-1.x86_64.rpm` for Fedora and other RPM distributions, and `request-eagle-X.Y.Z-x86_64-linux.tar.gz` for the AUR package | The browser downloads the file; pacman updates the AUR package |
| CLI | `request-eagle-cli-<target>` for macOS and Linux | Downloaded again by hand |

Each package also gets an update manifest, which the app reads to find its
update: `request-eagle-update.json` for macOS (the name older versions read),
`request-eagle-update-windows.json`, `request-eagle-update-appimage.json`,
`request-eagle-update-debian.json` and `request-eagle-update-rpm.json`.
`SHA256SUMS` lists every file.

`scripts/package-linux.sh` builds the Linux packages from one build. The
Debian and RPM packages install the app at `/usr/bin/request-eagle` and name
themselves in `/usr/share/request-eagle/package`, so the app reads the right
manifest. The AppImage needs no marker: its runtime names the image in the
`APPIMAGE` environment variable. The AUR package writes `pacman` as its name,
and the app leaves updates to it.

A stable install reads its manifest from GitHub's latest release. A nightly
install lists the releases through the GitHub API and takes the newest one
that has its manifest.

Windows builds are not code-signed yet, so SmartScreen asks to confirm the
first run.

Run the Release workflow by hand with **dry run** to build and check every
platform from a branch without publishing. **Skip macOS** leaves out signing
and notarization.

## Arch Linux

The Promote release workflow publishes `request-eagle-bin` to the AUR for
each promoted release, from `packaging/linux/PKGBUILD` with the version and
the tarball's checksum filled in. It does so when the `AUR_SSH_PRIVATE_KEY`
repository secret is set:

1. Create an account on <https://aur.archlinux.org> and add an SSH public key
   to it under **My Account**.
2. Store the matching private key as the `AUR_SSH_PRIVATE_KEY` secret.

The first promotion creates the package; pushing to a package that does not
exist yet creates it on the AUR.

## Release notes

`scripts/release-notes.py` lists the pull requests merged on `main` between
version tags, from merge commits ("Merge pull request #N", titled by their
message) and squashed commits ("Title (#N)").

- `release-notes.py json vX.Y.Z` writes every release up to that tag. Release
  builds embed it through `REQUEST_EAGLE_RELEASE_NOTES`, and the app reads it
  with `updater::release_notes()`. Other builds embed an empty list.
- `release-notes.py markdown vX.Y.Z [vA.B.C]` writes the notes of a GitHub
  release: the changes since the previous release, or since the given one.
