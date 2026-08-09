#!/bin/bash
# Can a human clear a BLOCKED agent through kamiroh, from a distance?
#
# Uses a directory nobody has approved, so Claude Code raises its workspace
# trust dialog. The point is the order: see the question, then answer it. That
# is remote consent, not a bypass -- the script never approves anything on its
# own initiative, it relays one specific answer to one visible question.
set -u
cd "$(dirname "$0")/.."

SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
STAMP=$(date +%s)
DEMO="${TMPDIR:-/tmp}/kamiroh-unblock-$STAMP"
mkdir -p "$DEMO/work"
BIN=./target/debug/kamiroh

nap() { sleep "$1"; }
call() { printf '%s\n' "$1" | nc -U "$SOCK" 2>/dev/null; }
state() { call "{\"id\":\"s\",\"method\":\"agent.get\",\"params\":{\"target\":\"$1\"}}" | jq -r '.result.agent.agent_status // "?"'; }
screen() { call "{\"id\":\"r\",\"method\":\"agent.read\",\"params\":{\"target\":\"$1\",\"source\":\"recent\",\"lines\":30,\"strip_ansi\":true}}" | jq -r '.result.read.text // ""'; }

P=""
cleanup() {
  [ -n "$P" ] && { echo; echo "--- closing $P ---"; \
    call "{\"id\":\"z\",\"method\":\"pane.close\",\"params\":{\"pane_id\":\"$P\"}}" | head -c 80; echo; }
  echo "note: $DEMO/work is now recorded in ~/.claude.json if trust was granted."
}
trap cleanup EXIT

echo "=== a directory nobody has approved: $DEMO/work ==="
P=$(call "{\"id\":\"1\",\"method\":\"pane.split\",\"params\":{\"direction\":\"down\",\"focus\":false,\"ratio\":0.4,\"cwd\":\"$DEMO/work\"}}" | jq -r '.result.pane.pane_id')
echo "pane: $P"; nap 3.0
call "{\"id\":\"2\",\"method\":\"agent.start\",\"params\":{\"name\":\"unblock-$STAMP\",\"kind\":\"claude\",\"pane_id\":\"$P\",\"timeout_ms\":120000}}" \
  | jq -c 'if .error then . else {started: .result.agent.name} end'

echo
echo "=== waiting for it to block on the trust dialog (NOT answering it) ==="
for i in $(seq 1 40); do
  S=$(state "$P")
  [ "$S" = "blocked" ] && { echo "blocked after ~${i}s"; break; }
  nap 1.0
done
echo "agent state: $(state "$P")"
echo
echo "--- the question, as the agent is showing it ---"
screen "$P" | grep -viE '^\s*$' | tail -14

echo
echo "=== now answer it THROUGH kamiroh ==="
# Line 1 answers the dialog. Line 2 is a real prompt, to show the agent is
# usable afterwards. /status first, to see what kamiroh thinks before any run.
printf '/status\n1\n/status\nReply with exactly this and nothing else: KAMIROH-UNBLOCKED\n/quit\n' > "$DEMO/typed.txt"

env -u HERDR_PANE_ID -u HERDR_ENV -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID \
  HERDR_SOCKET_PATH="$SOCK" \
  KAMIROH_KEY_FILE="$DEMO/node.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
  KAMIROH_AGENT_TARGET="$P" \
  $BIN < "$DEMO/typed.txt" > "$DEMO/out.txt" 2>&1 &
PID=$!
for _ in $(seq 1 60); do grep -q '^talking to' "$DEMO/out.txt" 2>/dev/null && break; nap 0.5; done
nap 45
kill $PID 2>/dev/null; wait $PID 2>/dev/null

echo "--- what kamiroh's console showed (status lines only) ---"
sed -n '/^talking to/,$p' "$DEMO/out.txt" | grep -nE '^(idle|blocked|busy|stopped|starting)$'
echo "  ^ first = before answering the dialog, second = after"
echo
echo "--- agent state afterwards: $(state "$P") ---"
echo "--- did it answer? ---"
screen "$P" | grep -iE 'kamiroh-unblocked|trust|❯' | tail -6
