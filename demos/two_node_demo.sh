#!/bin/bash
# Drives two real kamiroh node processes: B greets an agent on A.
# A admits B; the second run flips to an empty allowlist to show the refusal.
set -u
cd "$(dirname "$0")/.."

DEMO="${TMPDIR:-/tmp}/kamiroh-two-node"
rm -rf "$DEMO"; mkdir -p "$DEMO"
BIN=./target/debug/kamiroh

# This session runs inside a Herdr pane, so HERDR_* would be inherited by every
# node started here and they would report their agent state into the *real*
# pane you are looking at. Unset it: a demo must not touch a live session.
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID


PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done; }
trap cleanup EXIT

# Sub-second pause without `sleep`.
nap() { sleep "$1"; }

# Wait up to ~10s for a marker to appear in a file, failing loudly if the
# process died instead (which is what an unreadable log usually means).
wait_for() { # file, marker
  for _ in $(seq 1 100); do
    grep -q "$2" "$1" 2>/dev/null && return 0
    nap 0.1
  done
  echo "TIMEOUT waiting for '$2' in $1"; echo "--- $1 ---"; cat "$1"; return 1
}

# `endpoint id: <hex>` — the id is the third field, after the colon.
endpoint_id() { grep '^endpoint id:' "$1" | head -1 | awk '{print $3}'; }

# `listening:   [0.0.0.0:PORT, [::]:PORT]` — take the port off the first IPv4
# socket. The bind is a wildcard, so we reach it on loopback.
port_of() {
  grep '^listening:' "$1" \
    | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+' | head -1 | cut -d: -f2
}

# --- pass 1: learn B's endpoint id -----------------------------------------
KAMIROH_KEY_FILE="$DEMO/b.key" $BIN > "$DEMO/b0.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b0.txt" '^listening:' || exit 1
B_ID=$(endpoint_id "$DEMO/b0.txt")
kill "$B_PID" 2>/dev/null; wait "$B_PID" 2>/dev/null

[ "${#B_ID}" = 64 ] || { echo "bad B id: '$B_ID'"; exit 1; }

# --- pass 2: start A, admitting B ------------------------------------------
KAMIROH_KEY_FILE="$DEMO/a.key" KAMIROH_ALLOW="$B_ID" $BIN > "$DEMO/a.txt" 2>&1 &
A_PID=$!; PIDS+=("$A_PID")
wait_for "$DEMO/a.txt" '^serving' || exit 1
A_ID=$(endpoint_id "$DEMO/a.txt")
A_PORT=$(port_of "$DEMO/a.txt")

echo "node A: ${A_ID:0:16}…  port $A_PORT  (allows B)"
echo "node B: ${B_ID:0:16}…"
echo

# --- pass 3: B greets A -----------------------------------------------------
echo "--- B -> A, B is allowlisted ---"
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_PEER="$A_ID@127.0.0.1:$A_PORT" $BIN > "$DEMO/b1.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b1.txt" '^peer ' || exit 1
grep '^peer ' "$DEMO/b1.txt"
kill "$B_PID" 2>/dev/null; wait "$B_PID" 2>/dev/null
kill "$A_PID" 2>/dev/null; wait "$A_PID" 2>/dev/null

# --- pass 4: same, but A admits nobody --------------------------------------
KAMIROH_KEY_FILE="$DEMO/a.key" $BIN > "$DEMO/a2.txt" 2>&1 &
A_PID=$!; PIDS+=("$A_PID")
wait_for "$DEMO/a2.txt" '^serving' || exit 1
A_PORT=$(port_of "$DEMO/a2.txt")

echo
echo "--- B -> A, allowlist now empty ---"
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_PEER="$A_ID@127.0.0.1:$A_PORT" $BIN > "$DEMO/b2.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b2.txt" '^peer ' || exit 1
grep '^peer ' "$DEMO/b2.txt"
kill "$B_PID" 2>/dev/null; wait "$B_PID" 2>/dev/null
kill "$A_PID" 2>/dev/null; wait "$A_PID" 2>/dev/null

echo
echo "--- node A identity stable across its two runs ---"
grep '^endpoint id:' "$DEMO/a.txt" "$DEMO/a2.txt"
