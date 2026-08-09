#!/bin/bash
# P2 "use it": drive a real coding agent through kamiroh on a task that takes
# minutes, then try to do the two things kamiroh-phase-3.md §2 says you cannot.
#
# Unlike kind_live.sh, which proves a prompt gets an answer, this one is about
# what happens *while* an agent is working. That is where the interesting
# failures are: it found that `agent.read` is refused whenever the agent is
# busy, so M1's Partial{Busy} path cannot execute against a real agent.
#
# Costs tokens: it starts a real claude agent and gives it a real task. The work
# happens in an empty temp directory, never the repo.
set -u
cd "$(dirname "$0")/.."

SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
DEMO="${TMPDIR:-/tmp}/kamiroh-use-it"
WORK="$DEMO/work"
BIN=./target/debug/kamiroh

rm -rf "$DEMO"; mkdir -p "$WORK"
call() { printf '%s\n' "$1" | nc -U "$SOCK" 2>/dev/null; }
state() { call "{\"id\":\"s\",\"method\":\"agent.get\",\"params\":{\"target\":\"$1\"}}" \
            | jq -r '.result.agent.agent_status // "?"'; }

# A real task with no dependencies, so the agent needs no network.
( cd "$WORK" && cargo init --name scratch --vcs none -q 2>/dev/null )

SCRATCH=""
cleanup() {
  [ -n "$SCRATCH" ] && call "{\"id\":\"z\",\"method\":\"pane.close\",\"params\":{\"pane_id\":\"$SCRATCH\"}}" >/dev/null
}
trap cleanup EXIT

echo "--- scratch pane in $WORK ---"
SPLIT=$(call "{\"id\":\"1\",\"method\":\"pane.split\",\"params\":{\"direction\":\"down\",\"focus\":false,\"ratio\":0.30,\"cwd\":\"$WORK\"}}")
SCRATCH=$(printf '%s' "$SPLIT" | jq -r '.result.pane.pane_id // empty')
[ -n "$SCRATCH" ] || { echo "no pane: $SPLIT" | head -c 300; exit 1; }
echo "pane: $SCRATCH"
sleep 3   # agent.start fails with agent_pane_busy until the shell has a prompt

echo "--- agent.start claude ---"
call "{\"id\":\"2\",\"method\":\"agent.start\",\"params\":{\"name\":\"kamiroh-use\",\"kind\":\"claude\",\"pane_id\":\"$SCRATCH\",\"timeout_ms\":120000}}" \
  | jq -c 'if .error then . else {started:.result.type} end'

for i in $(seq 1 60); do
  READY=$(call "{\"id\":\"3\",\"method\":\"agent.get\",\"params\":{\"target\":\"$SCRATCH\"}}" \
            | jq -r '.result.agent.interactive_ready // false')
  [ "$READY" = "true" ] && { echo "interactive after ~${i}s"; break; }
  sleep 1
done

# Claude Code asks for workspace trust once per directory path, and this script
# uses a fresh one every run. That is deliberate: approving a consent prompt
# from a script is one step from approving it in the product. Answer it in the
# pane, and expect more of them -- a real task stops for permission repeatedly,
# which is itself one of this script's findings.
if [ "$(state "$SCRATCH")" = "blocked" ]; then
  echo
  echo "!! blocked before kamiroh has sent anything -- the workspace-trust dialog"
  echo "!! for $WORK. Answer it in pane $SCRATCH, then re-run."
  exit 1
fi

# Herdr's own view, sampled alongside, so kamiroh's answers can be checked
# against the truth rather than believed.
( for i in $(seq 1 90); do printf '%3ds %s\n' "$((i*5))" "$(state "$SCRATCH")" >> "$DEMO/truth.txt"; sleep 5; done ) &
POLL=$!

TASK='In this crate, implement an RPN calculator in src/lib.rs: pub fn eval(&str) -> Result<f64, EvalError> supporting + - * / and negative numbers, with an error enum for empty input, unknown tokens, stack underflow and division by zero. Add at least 12 unit tests covering the error cases. Then run cargo test and fix anything that fails.'

echo
echo "=== driving it through kamiroh (KAMIROH_AGENT_TARGET=$SCRATCH) ==="
# HERDR_PANE_ID is unset so kamiroh does not report state into the pane this
# session is running in; HERDR_SOCKET_PATH stays so it can reach the agent.
{
  printf '%s\n' "$TASK"
  sleep 10; printf '/status\n'
  sleep 20; printf '/status\n'
  sleep 20; printf '/status\n'
  sleep 5;  printf '/shutdown\n'
} | env -u HERDR_PANE_ID -u HERDR_ENV -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID \
      HERDR_SOCKET_PATH="$SOCK" \
      KAMIROH_KEY_FILE="$DEMO/node.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
      KAMIROH_AGENT_TARGET="$SCRATCH" \
      "$BIN" > "$DEMO/console.txt" 2>"$DEMO/log.txt" &
NODE=$!

# `/quit` ends the *console*, not the node -- J1 decided that deliberately, since
# a serving node has nobody at its pane. So a piped driver must kill the process
# rather than wait for it; waiting hangs forever.
for _ in $(seq 1 40); do
  grep -q 'controller actor .* has stopped' "$DEMO/console.txt" 2>/dev/null && break
  sleep 2
done
SHUTDOWN_AT=$(date "+%H:%M:%S")
kill $NODE 2>/dev/null; wait $NODE 2>/dev/null

echo "--- what the console showed ---"
sed -n '/^talking to/,$p' "$DEMO/console.txt"
echo
echo "kamiroh reported the agent stopped at $SHUTDOWN_AT"

echo
echo "=== is the agent still alive and working after that? ==="
for i in $(seq 1 24); do
  printf '  +%3ds  herdr: %-8s' "$((i*10))" "$(state "$SCRATCH")"
  if [ -f "$WORK/src/lib.rs" ]; then
    printf 'lib.rs %s lines, written %s' \
      "$(wc -l < "$WORK/src/lib.rs" | tr -d ' ')" \
      "$(stat -f '%Sm' -t '%H:%M:%S' "$WORK/src/lib.rs" 2>/dev/null || stat -c '%y' "$WORK/src/lib.rs")"
  else
    printf 'no lib.rs yet'
  fi
  echo
  sleep 10
done

kill $POLL 2>/dev/null
echo
echo "--- herdr's view throughout ---"
cat "$DEMO/truth.txt" 2>/dev/null
echo
echo "A lib.rs written after $SHUTDOWN_AT is the whole point: kamiroh answered"
echo "/shutdown with 'ok' and the agent carried on working."
