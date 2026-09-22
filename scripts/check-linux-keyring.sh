#!/usr/bin/env bash
# Native Linux storage check on a private bus, never the user's desktop keyring.
set -euo pipefail

if [[ ${1:-} == --session ]]; then
  fixture=$2

  # This bus has no service activation directories, so no installed keyring can
  # be started implicitly. Check the missing-provider path before starting ours.
  timeout 30s cargo test --locked -p preferences --test native_keyring -- --unavailable

  printf '%s\n' request-eagle-test-password | \
    keepassxc --pw-stdin --config "$fixture/keepassxc.ini" \
      --localconfig "$fixture/keepassxc-local.ini" "$fixture/test.kdbx" \
      >"$fixture/keepassxc.log" 2>&1 &
  provider_pid=$!
  # --profile records method names and timings, without secret message bodies.
  dbus-monitor --session --profile >"$fixture/dbus.log" 2>&1 &
  monitor_pid=$!
  # KeePassXC can wait on an exit dialog; this disposable process owns only the
  # synthetic database, so cleanup must not wait for desktop interaction.
  trap 'kill -KILL "$provider_pid" "$monitor_pid" 2>/dev/null || true; wait "$provider_pid" "$monitor_pid" 2>/dev/null || true' EXIT

  ready=false
  for ((attempt = 0; attempt < 100; attempt++)); do
    collection=$(dbus-send --session --print-reply --dest=org.freedesktop.secrets \
      /org/freedesktop/secrets org.freedesktop.Secret.Service.ReadAlias \
      string:default 2>/dev/null | sed -n 's/.*object path "\(\/org\/[^" ]*\)"/\1/p') || true
    if [[ -n $collection ]] && \
      dbus-send --session --print-reply --dest=org.freedesktop.secrets \
        "$collection" org.freedesktop.DBus.Properties.Get \
        string:org.freedesktop.Secret.Collection string:Locked 2>/dev/null | grep -q 'boolean false'; then
      ready=true
      break
    fi
    sleep 0.2
  done

  if [[ $ready != true ]]; then
    cat "$fixture/keepassxc.log" "$fixture/dbus.log" >&2
    echo 'KeePassXC did not expose its test database.' >&2
    exit 1
  fi

  if timeout 60s cargo test --locked -p preferences --test native_keyring -- --round-trip; then
    echo 'KeePassXC round trip and deletion passed.'
  else
    status=$?
    cat "$fixture/keepassxc.log" "$fixture/dbus.log" >&2
    xdotool search --onlyvisible --name . getwindowname %@ >&2 || true
    exit "$status"
  fi
  exit
fi

# Compile before opening the isolated desktop session so its timeouts cover
# native storage operations rather than a cold build.
cargo test --locked -p preferences --test native_keyring --no-run
script=$(realpath "$0")
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT

cat >"$fixture/bus.conf" <<'EOF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF

cat >"$fixture/database.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<KeePassFile>
  <Meta>
    <DatabaseName>Request Eagle test</DatabaseName>
    <CustomData><Item>
      <Key>FDO_SECRETS_EXPOSED_GROUP</Key>
      <Value>{00000000-0000-0000-0000-000000000001}</Value>
    </Item></CustomData>
  </Meta>
  <Root><Group>
    <UUID>AAAAAAAAAAAAAAAAAAAAAQ==</UUID>
    <Name>Request Eagle test</Name>
  </Group></Root>
</KeePassFile>
EOF

cat >"$fixture/keepassxc.ini" <<'EOF'
[General]
ConfigVersion=2
SingleInstance=false
OpenPreviousDatabasesOnStartup=false
[FdoSecrets]
Enabled=true
ConfirmAccessItem=false
ConfirmDeleteItem=false
ShowNotification=false
[GUI]
ShowTrayIcon=false
EOF

printf '%s\n' request-eagle-test-password request-eagle-test-password | \
  keepassxc-cli import --set-password --decryption-time 100 \
    "$fixture/database.xml" "$fixture/test.kdbx"

# Isolate Qt/KeePassXC configuration, caches, runtime files, and any D-Bus files.
mkdir -m 700 "$fixture/config" "$fixture/cache" "$fixture/data" "$fixture/runtime"
env XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache" \
  XDG_DATA_HOME="$fixture/data" XDG_RUNTIME_DIR="$fixture/runtime" \
  QT_QPA_PLATFORM=xcb WAYLAND_DISPLAY= \
  dbus-run-session --config-file "$fixture/bus.conf" -- \
    xvfb-run -a bash "$script" --session "$fixture"
