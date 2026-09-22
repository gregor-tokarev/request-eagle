#!/bin/bash
set -euo pipefail

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"

if [[ "$(git branch --show-current)" != main ]]; then
  echo 'Daily releases must run on main' >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo 'Daily releases require a clean checkout' >&2
  exit 1
fi

latest_tag="$(git tag --merged HEAD --sort=-version:refname |
  awk '/^v[0-9]+\.[0-9]+\.[0-9]+$/ && !found { print; found = 1 }')"

if [[ -n "$latest_tag" && "$(git rev-parse "$latest_tag^{commit}")" == "$(git rev-parse HEAD)" ]]; then
  published_tags="$(gh api --paginate "repos/$GITHUB_REPOSITORY/releases" \
    --jq '.[] | select(.draft == false and .prerelease == false) | .tag_name')"

  if grep -Fxq "$latest_tag" <<< "$published_tags"; then
    echo 'No new commits on main; skipping release'
    echo 'tag=' >> "$GITHUB_OUTPUT"
  else
    echo "Retrying unpublished release $latest_tag"
    echo "tag=$latest_tag" >> "$GITHUB_OUTPUT"
  fi

  exit 0
fi

manifest=crates/request-eagle/Cargo.toml
current_version="$(sed -n '/^name = "request-eagle"$/{n;s/version = "\([^"]*\)"/\1/p;}' "$manifest")"

if [[ ! "$current_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Expected a stable Cargo version, got: $current_version" >&2
  exit 1
fi

base_version="$(printf '%s\n' "$current_version" "${latest_tag#v}" |
  sort -t . -k1,1n -k2,2n -k3,3n | tail -n 1)"
IFS=. read -r major minor patch <<< "$base_version"
version="$major.$minor.$((10#$patch + 1))"
tag="v$version"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

# Validate both files before changing either, preserving dependency versions.
for file in "$manifest" Cargo.lock; do
  awk -v current="$current_version" -v version="$version" '
    /^name = "request-eagle"$/ {
      print
      getline

      if ($0 == "version = \"" current "\"") {
        $0 = "version = \"" version "\""
        count++
      }
    }
    { print }
    END { if (count != 1) exit 1 }
  ' "$file" > "$temporary/$(basename "$file")" || {
    echo "Expected one request-eagle $current_version entry in $file" >&2
    exit 1
  }
done

cp "$temporary/Cargo.toml" "$manifest"
cp "$temporary/Cargo.lock" Cargo.lock
git add "$manifest" Cargo.lock
git commit -m "Release $tag"
git tag "$tag"

# Reject concurrent main updates without leaving a remote tag behind.
git push --atomic origin HEAD:refs/heads/main "refs/tags/$tag"
echo "Prepared $tag"
echo "tag=$tag" >> "$GITHUB_OUTPUT"
