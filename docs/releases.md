# Releases

Request Eagle ships on two update tracks. The app follows the one chosen in
Settings > General > Update track, which is stable unless changed.

| Track | What it gets | How a release gets there |
| --- | --- | --- |
| Nightly | A build of `main` every day it changed | The Nightly release workflow, on its own |
| Stable | Nightly builds that worked out | Promote to stable, run by hand |

Both tracks share one version sequence. A nightly release is a GitHub
prerelease; a stable one is GitHub's latest release. Switching from nightly to
stable keeps the installed build until a newer one is promoted.

## Nightly releases

`.github/workflows/daily-release.yml` runs every morning at 06:17 UTC. When
`main` has changed since the last release, `scripts/prepare-daily-release.sh`
bumps the patch version, commits and tags it, and the Release workflow builds
and publishes it as a prerelease. Run the workflow by hand to release sooner.

## Stable releases

Run **Promote to stable** (`.github/workflows/promote.yml`) with a nightly tag,
or with no tag for the newest nightly. It marks that release as the latest
release, without rebuilding it, and rewrites its notes to list everything
merged since the previous stable release. It refuses a release older than the
current stable one.

Pushing a `vX.Y.Z` tag whose version matches `crates/request-eagle/Cargo.toml`
also publishes a stable release.

## What a release contains

`.github/workflows/release.yml` builds every platform from the tag:

| Platform | Files | Updates |
| --- | --- | --- |
| macOS (Apple Silicon) | `RequestEagle-X.Y.Z-arm64.dmg` and `.zip`, signed and notarized | In the app, from the ZIP |
| Windows (x64) | `RequestEagle-X.Y.Z-x64-setup.exe`, a per-user installer | The browser downloads the installer |
| Debian and Ubuntu (x64) | `request-eagle_X.Y.Z_amd64.deb`, built on Debian 12 | The browser downloads the package |
| CLI | `request-eagle-cli-<target>` for macOS and Linux | Downloaded again by hand |

Each platform also gets an update manifest, which the app reads to find its
update: `request-eagle-update.json` for macOS (the name older versions read),
`request-eagle-update-windows.json` and `request-eagle-update-debian.json`.
`SHA256SUMS` lists every file.

A stable install reads its manifest from GitHub's latest release. A nightly
install lists the releases through the GitHub API and takes the newest one
that has its manifest.

Windows builds are not code-signed yet, so SmartScreen asks to confirm the
first run.

Run the Release workflow by hand with **dry run** to build and check every
platform from a branch without publishing. **Skip macOS** leaves out signing
and notarization.

## Release notes

`scripts/release-notes.py` lists the pull requests merged on `main` between
version tags, from merge commits ("Merge pull request #N", titled by their
message) and squashed commits ("Title (#N)").

- `release-notes.py json vX.Y.Z` writes every release up to that tag. Release
  builds embed it through `REQUEST_EAGLE_RELEASE_NOTES`, and the app reads it
  with `updater::release_notes()`. Other builds embed an empty list.
- `release-notes.py markdown vX.Y.Z [vA.B.C]` writes the notes of a GitHub
  release: the changes since the previous release, or since the given one.
