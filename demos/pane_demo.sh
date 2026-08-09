#!/bin/bash
# A pane on node B drives an agent on node A, across a real Iroh connection.
# Node B is the "laptop"; node A is the "home node" running the agent.
set -u
cd "$(dirname "$0")/.."

DEMO="${TMPDIR:-/tmp}/kamiroh-pane"
rm -rf "$DEMO"; mkdir -p "$DEMO"
BIN=./target/debug/kamiroh

# This session runs inside a Herdr pane, so HERDR_* would be inherited by every
# node started here and they would report their agent state into the *real*
# pane you are looking at. Unset it: a demo must not touch a live session.
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID


PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done; }
trap cleanup EXIT

nap() { perl -e 'select(undef, undef, undef, shift)' "$1"; }

wait_for() { # file, marker
  for _ in $(seq 1 100); do
    grep -q "$2" "$1" 2>/dev/null && return 0
    nap 0.1
  done
  echo "TIMEOUT waiting for '$2' in $1"; cat "$1"; return 1
}

endpoint_id() { grep '^endpoint id:' "$1" | head -1 | awk '{print $3}'; }
port_of() {
  grep '^listening:' "$1" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+' \
    | head -1 | cut -d: -f2
}

# What the person at the pane types.
cat > "$DEMO/typed.txt" <<'KEYS'
hello from the pane
/status

/nonsense
/quit
KEYS

# --- learn B's id ------------------------------------------------------------
KAMIROH_KEY_FILE="$DEMO/b.key" $BIN < /dev/null > "$DEMO/b0.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b0.txt" '^listening:' || exit 1
B_ID=$(endpoint_id "$DEMO/b0.txt")
kill "$B_PID" 2>/dev/null; wait "$B_PID" 2>/dev/null

# --- node A: the home node, admitting B --------------------------------------
KAMIROH_KEY_FILE="$DEMO/a.key" KAMIROH_ALLOW="$B_ID" \
  $BIN < /dev/null > "$DEMO/a.txt" 2>&1 &
A_PID=$!; PIDS+=("$A_PID")
wait_for "$DEMO/a.txt" '^serving' || exit 1
A_ID=$(endpoint_id "$DEMO/a.txt")
A_PORT=$(port_of "$DEMO/a.txt")

echo "node A (home):   ${A_ID:0:16}…  port $A_PORT   agent 'agent'"
echo "node B (laptop): ${B_ID:0:16}…"
echo

# --- node B: a pane driving A's agent ----------------------------------------
echo "=== what the pane shows (B), typing at A's agent ==="
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_PEER="$A_ID@127.0.0.1:$A_PORT" \
  $BIN < "$DEMO/typed.txt" > "$DEMO/b1.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b1.txt" '^serving' || exit 1
nap 1.5
sed -n '/^pane:/,$p' "$DEMO/b1.txt"
kill "$B_PID" 2>/dev/null; wait "$B_PID" 2>/dev/null
kill "$A_PID" 2>/dev/null; wait "$A_PID" 2>/dev/null

# --- same pane, but A admits nobody ------------------------------------------
KAMIROH_KEY_FILE="$DEMO/a.key" $BIN < /dev/null > "$DEMO/a2.txt" 2>&1 &
A_PID=$!; PIDS+=("$A_PID")
wait_for "$DEMO/a2.txt" '^serving' || exit 1
A_PORT=$(port_of "$DEMO/a2.txt")

echo
echo "=== same pane, after A empties its allowlist ==="
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_PEER="$A_ID@127.0.0.1:$A_PORT" \
  $BIN < "$DEMO/typed.txt" > "$DEMO/b2.txt" 2>&1 &
B_PID=$!; PIDS+=("$B_PID")
wait_for "$DEMO/b2.txt" '^serving' || exit 1
nap 1.5
sed -n '/^talking to/,$p' "$DEMO/b2.txt"
