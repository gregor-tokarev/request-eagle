#!/bin/bash

set -euo pipefail

if test "$(uname -s)" = Linux && test -z "${WAYLAND_DISPLAY:-}" && test -z "${DISPLAY:-}"; then
	# Remote terminals may not inherit the desktop's display environment.
	# Only select a Wayland socket automatically when there is no ambiguity.
	RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
	DISPLAYS=()

	for socket in "$RUNTIME_DIR"/wayland-*; do
		if test -S "$socket"; then
			DISPLAYS+=("$socket")
		fi
	done

	if test "${#DISPLAYS[@]}" -ne 1; then
		echo "Cannot select a desktop display. Set WAYLAND_DISPLAY (and XDG_RUNTIME_DIR) or DISPLAY to your desktop session before launching Request Eagle." >&2
		exit 1
	fi

	export XDG_RUNTIME_DIR="$RUNTIME_DIR"
	export WAYLAND_DISPLAY="${DISPLAYS[0]##*/}"
	echo "Using Wayland display $WAYLAND_DISPLAY."
fi

exec "$@"
