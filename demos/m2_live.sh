#!/bin/bash
# M2 live: can a node be reached by endpoint id alone, with no address anywhere?
#
# Both nodes run on this machine, so this cannot prove NAT traversal. What it
# proves is the thing that actually blocked the product: that B needs only A's
# identity, never its address.
#
# Includes the negative control, because without it a success proves nothing --
# two nodes on one host might have found each other some other way.
#
# Uses throwaway node keys, so the records published to n0 are keyed to
# ephemeral ids and expire on their own.
set -u
cd "$(dirname "$0")/.."

DEMO="${TMPDIR:-/tmp}/kamiroh-m2"; rm -rf "$DEMO"; mkdir -p "$DEMO"
BIN=./target/debug/kamiroh
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID

PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done; }
trap cleanup EXIT

nap() { perl -e 'select(undef, undef, undef, shift)' "$1"; }
wait_for() { for _ in $(seq 1 "${3:-100}"); do grep -q "$2" "$1" 2>/dev/null && return 0; nap 0.5; done; return 1; }
endpoint_id() { grep '^endpoint id:' "$1" | head -1 | awk '{print $3}'; }

# --- learn B's id, so A can admit it -----------------------------------------
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
  $BIN < /dev/null > "$DEMO/b0.txt" 2>&1 &
B0=$!; PIDS+=("$B0")
wait_for "$DEMO/b0.txt" '^endpoint id:' 40 || { cat "$DEMO/b0.txt"; exit 1; }
B_ID=$(endpoint_id "$DEMO/b0.txt"); kill "$B0" 2>/dev/null; wait "$B0" 2>/dev/null

# --- node A: reachable from anywhere, admitting B ----------------------------
echo "=== node A: KAMIROH_REACH=anywhere (publishes its addresses) ==="
KAMIROH_KEY_FILE="$DEMO/a.key" KAMIROH_ALLOW_FILE="$DEMO/allow-a" \
KAMIROH_ALLOW="$B_ID" KAMIROH_REACH=anywhere \
  $BIN < /dev/null > "$DEMO/a.txt" 2>&1 &
A_PID=$!; PIDS+=("$A_PID")
wait_for "$DEMO/a.txt" '^serving' 60 || { cat "$DEMO/a.txt"; exit 1; }
A_ID=$(endpoint_id "$DEMO/a.txt")
grep -E '^(endpoint id|reach):' "$DEMO/a.txt" | sed 's/^/  /'
echo "  (no port is recorded or used below)"

echo
echo "--- giving the address record time to publish ---"
nap 12.0

# --- control: reach=direct, bare id. Must fail, and say why ------------------
echo "=== CONTROL: node B with KAMIROH_REACH=direct and only A's id ==="
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
KAMIROH_PEER="$A_ID" \
  $BIN < /dev/null > "$DEMO/b-direct.txt" 2>&1 &
BD=$!; PIDS+=("$BD")
wait_for "$DEMO/b-direct.txt" '^peer ' 60
grep -E '^(reach:|peer )' "$DEMO/b-direct.txt" | sed 's/^/  /'
kill "$BD" 2>/dev/null; wait "$BD" 2>/dev/null

echo
echo "=== node B with KAMIROH_REACH=anywhere and only A's id ==="
KAMIROH_KEY_FILE="$DEMO/b.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
KAMIROH_PEER="$A_ID" KAMIROH_REACH=anywhere \
  $BIN < /dev/null > "$DEMO/b-anywhere.txt" 2>&1 &
BA=$!; PIDS+=("$BA")
wait_for "$DEMO/b-anywhere.txt" '^peer ' 120
grep -E '^(reach:|peer )' "$DEMO/b-anywhere.txt" | sed 's/^/  /'

echo
echo "--- B was given this, and nothing else: ---"
echo "    KAMIROH_PEER=$A_ID"
