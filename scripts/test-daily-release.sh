#!/bin/bash
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/prepare-daily-release.sh"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

# Keep all pushes local and replace only the GitHub release-list API.
mkdir "$temporary/bin"
cat > "$temporary/bin/gh" <<'EOF'
#!/bin/bash
set -euo pipefail
[[ "${TEST_API_FAILURE:-false}" == false ]] || exit 1
printf '%s\n' "$TEST_PUBLISHED_TAGS"
EOF
chmod +x "$temporary/bin/gh"
export PATH="$temporary/bin:$PATH"
export GITHUB_REPOSITORY=example/request-eagle
export TEST_PUBLISHED_TAGS=v0.1.9
export TEST_API_FAILURE=false

setup() {
  case_dir="$temporary/$1"
  mkdir "$case_dir"
  git init --bare "$case_dir/remote.git"
  git init -b main "$case_dir/checkout"
  cd "$case_dir/checkout"
  git config user.name 'Release test'
  git config user.email release@example.com
  git config commit.gpgsign false
  git config tag.gpgsign false
  git remote add origin "$case_dir/remote.git"
  mkdir -p crates/request-eagle
  printf '[package]\nname = "request-eagle"\nversion = "0.1.9"\n' > crates/request-eagle/Cargo.toml
  cat > Cargo.lock <<'EOF'
version = 4

[[package]]
name = "dependency"
version = "0.1.9"

[[package]]
name = "request-eagle"
version = "0.1.9"
EOF
  git add .
  git commit -m 'Initial release'
  git tag v0.1.9
  git push origin main --tags
  export GITHUB_OUTPUT="$case_dir/output"
  export TEST_PUBLISHED_TAGS=v0.1.9
  export TEST_API_FAILURE=false
}

add_change() {
  echo 'New feature' > feature.txt
  git add feature.txt
  git commit -m 'Add feature'
  git push origin main
}

expect_failure() {
  if bash "$script"; then
    echo 'Expected release preparation to fail' >&2
    exit 1
  fi

  test ! -s "$GITHUB_OUTPUT"
}

setup unchanged
before="$(git rev-parse HEAD)"
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = 'tag='
test "$(git rev-parse HEAD)" = "$before"

setup changed
add_change
git tag v0.1.100-rc.1
git tag v0.1.8
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = tag=v0.1.10
grep -Fxq 'version = "0.1.10"' crates/request-eagle/Cargo.toml
test "$(sed -n '/^name = "request-eagle"$/{n;p;}' Cargo.lock)" = 'version = "0.1.10"'
test "$(sed -n '/^name = "dependency"$/{n;p;}' Cargo.lock)" = 'version = "0.1.9"'
test "$(git --git-dir="$case_dir/remote.git" rev-parse main)" = "$(git rev-parse HEAD)"
test "$(git --git-dir="$case_dir/remote.git" rev-parse v0.1.10)" = "$(git rev-parse HEAD)"
export TEST_PUBLISHED_TAGS=$'v0.1.9\nv0.1.10'
: > "$GITHUB_OUTPUT"
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = 'tag='

setup retry
export TEST_PUBLISHED_TAGS=''
before="$(git rev-parse HEAD)"
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = tag=v0.1.9
test "$(git rev-parse HEAD)" = "$before"

setup changed_after_failure
add_change
export TEST_PUBLISHED_TAGS=''
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = tag=v0.1.10

setup first_release
git tag -d v0.1.9
git push origin :refs/tags/v0.1.9
bash "$script"
test "$(cat "$GITHUB_OUTPUT")" = tag=v0.1.10

setup mismatched_lockfile
add_change
sed 's/0.1.9/0.1.8/g' Cargo.lock > "$case_dir/lockfile"
cp "$case_dir/lockfile" Cargo.lock
git add Cargo.lock
git commit -m 'Mismatched lockfile'
expect_failure
test -z "$(git status --porcelain)"

setup concurrent_push
add_change
git clone --branch main "$case_dir/remote.git" "$case_dir/other"
git -C "$case_dir/other" -c user.name=Other -c user.email=other@example.com \
  -c commit.gpgsign=false commit --allow-empty -m 'Concurrent change'
git -C "$case_dir/other" push origin main
expect_failure
test -z "$(git --git-dir="$case_dir/remote.git" tag --list v0.1.10)"
test "$(git --git-dir="$case_dir/remote.git" rev-parse main)" = "$(git -C "$case_dir/other" rev-parse HEAD)"

setup wrong_branch
git checkout -b feature
expect_failure

setup dirty_checkout
echo 'Uncommitted change' > feature.txt
expect_failure

setup api_failure
export TEST_API_FAILURE=true
before="$(git rev-parse HEAD)"
expect_failure
test "$(git rev-parse HEAD)" = "$before"

echo 'All 10 daily release tests passed'
