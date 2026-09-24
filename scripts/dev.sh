#!/bin/bash

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
POLL_INTERVAL="${REQUEST_EAGLE_DEV_POLL_INTERVAL:-0.5}"
APP_PID=""

case "$(uname -s)" in
	Darwin)
		BUILD_TARGET=bundle
		APP="$ROOT/target/release/Request Eagle (Dev).app/Contents/MacOS/request-eagle"
		;;
	Linux)
		BUILD_TARGET=build
		APP="$ROOT/target/release/request-eagle"
		;;
	*)
		echo "Unsupported development platform: $(uname -s)" >&2
		exit 1
		;;
esac

snapshot() {
	{
		for file in Cargo.toml Cargo.lock Makefile scripts/dev.sh scripts/run.sh; do
			test ! -f "$ROOT/$file" || printf '%s\n' "$file"
		done
		find "$ROOT/crates" "$ROOT/packaging/macos" -type f -print |
			sed "s|^$ROOT/||"
	} |
		LC_ALL=C sort |
		while IFS= read -r file; do
			printf '%s  ' "$file"
			cksum "$ROOT/$file"
		done |
		cksum
}

stop_app() {
	if test -z "$APP_PID"; then
		return
	fi

	if kill -0 "$APP_PID" 2>/dev/null; then
		kill "$APP_PID" 2>/dev/null || true
	fi
	wait "$APP_PID" 2>/dev/null || true
	APP_PID=""
}

shutdown() {
	stop_app
	exit 0
}

trap shutdown INT TERM
trap stop_app EXIT

echo "Watching Cargo and application files. Release build with GPUI Kit FPS monitor. Press Ctrl-C to stop."

LAST_SNAPSHOT=""
while true; do
	CURRENT_SNAPSHOT="$(snapshot)"

	if test "$CURRENT_SNAPSHOT" = "$LAST_SNAPSHOT"; then
		sleep "$POLL_INTERVAL"
		continue
	fi

	LAST_SNAPSHOT="$CURRENT_SNAPSHOT"
	stop_app
	echo
	echo "Change detected; rebuilding Request Eagle..."

	if "${MAKE:-make}" --no-print-directory -C "$ROOT" "$BUILD_TARGET" \
		PROFILE=release FEATURES=dev-profiler \
		APP_NAME="Request Eagle (Dev)" BUNDLE_ID=com.egortokarev.requesteagle.dev; then
		# If another edit landed during the build, rebuild once more before launch.
		if test "$(snapshot)" != "$LAST_SNAPSHOT"; then
			LAST_SNAPSHOT=""
			continue
		fi

		echo "Launching Request Eagle..."
		"$ROOT/scripts/run.sh" "$APP" &
		APP_PID=$!
	else
		echo "Build failed; waiting for another change."
	fi
done
