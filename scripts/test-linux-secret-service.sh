#!/usr/bin/env bash
# Synthetic secrets, private data directories and a private D-Bus session only.
set -euo pipefail
CAIDEX_CREDENTIAL_TEST_ROOT="$(mktemp -d /tmp/caidex-keyring.XXXXXX)"
trap 'rm -rf -- "$CAIDEX_CREDENTIAL_TEST_ROOT"' EXIT
mkdir -m 700 "$CAIDEX_CREDENTIAL_TEST_ROOT/data" "$CAIDEX_CREDENTIAL_TEST_ROOT/config" "$CAIDEX_CREDENTIAL_TEST_ROOT/runtime"
dbus-run-session -- bash -euo pipefail -c '
  export XDG_DATA_HOME="$1/data" XDG_CONFIG_HOME="$1/config" XDG_RUNTIME_DIR="$1/runtime"
  printf "%s\n" "caidex-synthetic-keyring-password" | gnome-keyring-daemon --foreground --unlock --components=secrets >"$1/daemon.log" 2>&1 &
  CAIDEX_KEYRING_FIXTURE_PID=$!
  cleanup() { kill "$CAIDEX_KEYRING_FIXTURE_PID" 2>/dev/null || true; wait "$CAIDEX_KEYRING_FIXTURE_PID" 2>/dev/null || true; }
  trap cleanup EXIT
  CAIDEX_KEYRING_FIXTURE_READY=false
  for CAIDEX_KEYRING_FIXTURE_ATTEMPT in {1..50}; do
    if gdbus call --session --dest org.freedesktop.secrets --object-path /org/freedesktop/secrets --method org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
      CAIDEX_KEYRING_FIXTURE_READY=true
      break
    fi
    kill -0 "$CAIDEX_KEYRING_FIXTURE_PID"
    sleep 0.1
  done
  if [ "$CAIDEX_KEYRING_FIXTURE_READY" != true ]; then
    echo "Isolated Secret Service did not become ready" >&2
    exit 1
  fi
  cargo test -p caidex-credentials --test native --locked -- --ignored --exact native_store_round_trip_update_delete_and_missing_entry
' caidex-keyring-fixture "$CAIDEX_CREDENTIAL_TEST_ROOT"
