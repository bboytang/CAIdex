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
    # Ping can activate a second, locked daemon before our unlocked fixture
    # owns the name. Ask the bus first; this never activates Secret Service.
    if [ "$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.freedesktop.secrets)" = "(true,)" ] &&
       [ "$(gdbus call --session --dest org.freedesktop.secrets --object-path /org/freedesktop/secrets/aliases/default --method org.freedesktop.DBus.Properties.Get org.freedesktop.Secret.Collection Locked 2>/dev/null)" = "(<false>,)" ]; then
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
