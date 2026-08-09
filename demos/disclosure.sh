#!/bin/bash
# Demonstrates ARCHITECTURE.md 5a: a node that is NOT on the allowlist can still
# FIND a published node. Reachable is not admitted -- and the two failure modes
# are distinguishable, which is what makes the claim checkable.
set -u
cd "$(dirname "$0")/.."
D="${TMPDIR:-/tmp}/kamiroh-disclose"; rm -rf "$D"; mkdir -p "$D"
BIN=./target/debug/kamiroh
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID
nap() { sleep "$1"; }
wait_for() { for _ in $(seq 1 "${3:-100}"); do grep -q "$2" "$1" 2>/dev/null && return 0; nap 0.5; done; return 1; }
PIDS=(); trap 'for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done' EXIT

# A publishes, and admits NOBODY (empty allowlist).
KAMIROH_KEY_FILE="$D/a.key" KAMIROH_ALLOW_FILE="$D/none" KAMIROH_REACH=anywhere \
  $BIN < /dev/null > "$D/a.txt" 2>&1 &
A=$!; PIDS+=("$A")
wait_for "$D/a.txt" '^serving' 60 || { cat "$D/a.txt"; exit 1; }
A_ID=$(grep '^endpoint id:' "$D/a.txt" | head -1 | awk '{print $3}')
echo "node A published, admitting nobody:"
grep -E '^allowing:' "$D/a.txt" | sed 's/^/  /'
nap 12.0

# C is a stranger: not on A's allowlist, and has only A's id.
echo
echo "=== stranger C, given only A's endpoint id ==="
KAMIROH_KEY_FILE="$D/c.key" KAMIROH_ALLOW_FILE="$D/none-c" \
KAMIROH_PEER="$A_ID" KAMIROH_REACH=anywhere \
  $BIN < /dev/null > "$D/c.txt" 2>&1 &
C=$!; PIDS+=("$C")
wait_for "$D/c.txt" '^peer ' 120
grep -E '^peer ' "$D/c.txt" | sed 's/^/  /'
echo
echo "  refused  = C FOUND A and was rejected  -> disclosure is real, allowlist holds"
echo "  unreachable = C never found A          -> no disclosure"
