#!/bin/bash
# Two agents on one node, driven by name from a peer.
set -u
cd "$(dirname "$0")/.."
D="${TMPDIR:-/tmp}/kamiroh-m3"; rm -rf "$D"; mkdir -p "$D"
BIN=./target/debug/kamiroh
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID
nap() { sleep "$1"; }
wait_for() { for _ in $(seq 1 "${3:-100}"); do grep -q "$2" "$1" 2>/dev/null && return 0; nap 0.5; done; return 1; }
eid() { grep '^endpoint id:' "$1" | head -1 | awk '{print $3}'; }
port() { grep '^listening:' "$1" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+' | head -1 | cut -d: -f2; }
PIDS=(); trap 'for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done' EXIT

# B learns its id
KAMIROH_KEY_FILE="$D/b.key" KAMIROH_ALLOW_FILE="$D/none" $BIN </dev/null >"$D/b0.txt" 2>&1 &
B0=$!; PIDS+=("$B0"); wait_for "$D/b0.txt" '^endpoint id:' 40; B_ID=$(eid "$D/b0.txt")
kill "$B0" 2>/dev/null; wait "$B0" 2>/dev/null

# A hosts TWO agents, from a file.
cat > "$D/agents-a" <<'AGENTS'
# name      target
agent       echo
reviewer    echo
AGENTS
KAMIROH_KEY_FILE="$D/a.key" KAMIROH_ALLOW_FILE="$D/none-a" KAMIROH_ALLOW="$B_ID" \
KAMIROH_AGENTS_FILE="$D/agents-a" $BIN </dev/null >"$D/a.txt" 2>&1 &
A=$!; PIDS+=("$A"); wait_for "$D/a.txt" '^serving' 60 || { cat "$D/a.txt"; exit 1; }
A_ID=$(eid "$D/a.txt"); A_PORT=$(port "$D/a.txt")
echo "=== node A ==="; grep -E '^(agents:|pane agent:)' "$D/a.txt" | sed 's/^/  /'

for TARGET in agent reviewer nonesuch; do
  echo
  echo "=== node B driving A's \"$TARGET\" by name ==="
  printf 'hello %s\n/quit\n' "$TARGET" > "$D/typed.txt"
  KAMIROH_KEY_FILE="$D/b.key" KAMIROH_ALLOW_FILE="$D/none" \
  KAMIROH_PEER="$A_ID@127.0.0.1:$A_PORT" KAMIROH_PEER_AGENT="$TARGET" \
    $BIN < "$D/typed.txt" > "$D/b-$TARGET.txt" 2>&1 &
  BP=$!; PIDS+=("$BP")
  wait_for "$D/b-$TARGET.txt" '^talking to' 60; nap 2.0
  grep -E '^(pane:)' "$D/b-$TARGET.txt" | sed 's/^/  /'
  sed -n '/^talking to/,$p' "$D/b-$TARGET.txt" | grep -vE '^\s*$|^talking to' | head -2 | sed 's/^/  reply: /'
  kill "$BP" 2>/dev/null; wait "$BP" 2>/dev/null
done
