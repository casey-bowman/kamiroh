#!/bin/bash
# M1 end to end against a real Herdr-managed agent.
#
# Starts a claude agent in a scratch pane, prompts it *through kamiroh*, and
# closes everything again. The agent runs in an empty temp directory, not the
# repo: a coding agent should have nothing of the user's within reach.
set -u
cd "$(dirname "$0")/.."

SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
DEMO="${TMPDIR:-/tmp}/kamiroh-m1-${1:-claude}"; rm -rf "$DEMO"; mkdir -p "$DEMO/work"
BIN=./target/debug/kamiroh
KIND="${1:-claude}"
AGENT_NAME="kamiroh-$KIND"

nap() { sleep "$1"; }
call() { printf '%s\n' "$1" | nc -U "$SOCK" 2>/dev/null; }

SCRATCH=""
cleanup() {
  if [ -n "$SCRATCH" ]; then
    echo
    echo "--- closing scratch pane $SCRATCH (stops the agent with it) ---"
    call "{\"id\":\"z\",\"method\":\"pane.close\",\"params\":{\"pane_id\":\"$SCRATCH\"}}" | head -c 120
    echo
  fi
}
trap cleanup EXIT

# --- a pane for the agent, rooted in an empty directory ----------------------
echo "--- creating a scratch pane in $DEMO/work ---"
SPLIT=$(call "{\"id\":\"1\",\"method\":\"pane.split\",\"params\":{\"direction\":\"down\",\"focus\":false,\"ratio\":0.35,\"cwd\":\"$DEMO/work\"}}")
SCRATCH=$(printf '%s' "$SPLIT" | jq -r '.result.pane.pane_id // empty')
[ -n "$SCRATCH" ] || { echo "no pane; $SPLIT" | head -c 300; exit 1; }
echo "scratch pane: $SCRATCH"
# Herdr refuses agent.start until the pane's shell has reached its prompt.
nap 3.0

# --- start a real claude agent in it -----------------------------------------
echo "--- agent.start kind=$KIND name=$AGENT_NAME ---"
call "{\"id\":\"2\",\"method\":\"agent.start\",\"params\":{\"name\":\"$AGENT_NAME\",\"kind\":\"$KIND\",\"pane_id\":\"$SCRATCH\",\"timeout_ms\":120000}}" \
  | jq -c 'if .error then . else {started: .result.type, agent: .result.agent.name, status: .result.agent.agent_status} end'

echo "--- waiting for it to become interactive ---"
for i in $(seq 1 60); do
  INFO=$(call "{\"id\":\"3\",\"method\":\"agent.get\",\"params\":{\"target\":\"$SCRATCH\"}}")
  READY=$(printf '%s' "$INFO" | jq -r '.result.agent.interactive_ready // false')
  STATE=$(printf '%s' "$INFO" | jq -r '.result.agent.agent_status // "?"')
  [ "$READY" = "true" ] && { echo "ready after ~$((i))s (status=$STATE)"; break; }
  nap 1.0
done
[ "${READY:-false}" = "true" ] || { echo "agent never became interactive"; exit 1; }

# Claude Code asks for workspace trust the first time an agent runs in a given
# directory, and records the answer per path in ~/.claude.json. That makes this
# a one-time human step per scratch dir -- and an invisible dependency of this
# test, since a re-run of the same path is silent while a fresh path is not.
# Say so out loud rather than letting it look like a kamiroh failure.
STATE=$(call "{\"id\":\"3b\",\"method\":\"agent.get\",\"params\":{\"target\":\"$SCRATCH\"}}" | jq -r '.result.agent.agent_status // "?"')
if [ "$STATE" = "blocked" ]; then
  echo
  echo "!! The agent is BLOCKED before kamiroh has sent anything."
  echo "!! Almost certainly Claude Code's workspace-trust dialog for"
  echo "!!   $DEMO/work"
  echo "!! Approve it in pane $SCRATCH, then re-run. This is a deliberate"
  echo "!! human step: approving it from a script would be scripting around a"
  echo "!! consent prompt. It is asked once per directory path."
  echo
fi

# --- prompt it THROUGH kamiroh ------------------------------------------------
# HERDR_PANE_ID is unset so kamiroh does not report state into the pane this
# session is running in; HERDR_SOCKET_PATH stays so it can reach the agent.
echo
echo "=== prompting through kamiroh (KAMIROH_AGENT_TARGET=$SCRATCH) ==="
printf 'Reply with exactly this and nothing else: KAMIROH-OK\n/quit\n' > "$DEMO/typed.txt"

env -u HERDR_PANE_ID -u HERDR_ENV -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID \
  HERDR_SOCKET_PATH="$SOCK" \
  KAMIROH_KEY_FILE="$DEMO/node.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
  KAMIROH_AGENT_TARGET="$SCRATCH" \
  $BIN < "$DEMO/typed.txt" > "$DEMO/out.txt" 2>&1 &
PID=$!

for _ in $(seq 1 60); do
  grep -q '^talking to' "$DEMO/out.txt" 2>/dev/null && break
  nap 0.5
done
nap 25
kill $PID 2>/dev/null; wait $PID 2>/dev/null

echo "--- kamiroh's startup lines ---"
grep -E '^(agent:|herdr:|pane:)' "$DEMO/out.txt"
echo
echo "--- what the console showed ---"
sed -n '/^talking to/,$p' "$DEMO/out.txt" | head -40
