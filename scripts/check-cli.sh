#!/bin/bash
set -euo pipefail

CLI_TEST_DIR="$(mktemp -d)"
CLI_TEST_TOKEN="$(openssl rand -hex 32)"
CLI_APP_PID=
cleanup() {
  if [[ -n "$CLI_APP_PID" ]]; then
    kill "$CLI_APP_PID" 2>/dev/null || true
    wait "$CLI_APP_PID" 2>/dev/null || true
  fi
  rm -rf "$CLI_TEST_DIR"
}
trap cleanup EXIT
mkdir -p "$CLI_TEST_DIR/home" "$CLI_TEST_DIR/runtime"
chmod 700 "$CLI_TEST_DIR/runtime"
env HOME="$CLI_TEST_DIR/home" XDG_RUNTIME_DIR="$CLI_TEST_DIR/runtime" WAYLAND_DISPLAY= \
  REQUEST_EAGLE_COLLECTIONS_DIR="$CLI_TEST_DIR/collections" \
  REQUEST_EAGLE_AUTOMATION_TOKEN="$CLI_TEST_TOKEN" \
  REQUEST_EAGLE_AUTOMATION_DIR="$CLI_TEST_DIR/automation" \
  REQUEST_EAGLE_WINDOW_TITLE='Request Eagle (CLI integration)' \
  target/debug/request-eagle >"$CLI_TEST_DIR/app.log" 2>&1 &
CLI_APP_PID=$!
CLI_SOCKET="$CLI_TEST_DIR/automation/$CLI_APP_PID.sock"
for attempt in {1..100}; do
  if [[ -S "$CLI_SOCKET" ]]; then break; fi
  if ! kill -0 "$CLI_APP_PID" 2>/dev/null; then cat "$CLI_TEST_DIR/app.log"; exit 1; fi
  sleep 0.1
done
if ! REQUEST_EAGLE_CLI_TOKEN="$CLI_TEST_TOKEN" python3 scripts/test-cli.py --socket "$CLI_SOCKET"; then
  cat "$CLI_TEST_DIR/app.log"
  exit 1
fi
