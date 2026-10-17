#!/bin/bash
# Run xcrun stapler, trying again when it fails. Stapling and validating look
# the notarization ticket up in Apple's CloudKit, which sometimes drops the
# connection ("The network connection was lost", error 68).
set -euo pipefail

attempts=4
for ((attempt = 1; ; attempt++)); do
  if xcrun stapler "$@"; then
    exit 0
  fi
  if ((attempt == attempts)); then
    echo "stapler $1 failed $attempts times" >&2
    exit 1
  fi
  echo "stapler $1 failed, trying again in 15 seconds" >&2
  sleep 15
done
