#!/bin/bash
# Verifies kamiroh's Herdr reporting SUCCESS path against the real daemon, in a
# scratch pane created for the purpose and closed again afterwards.
set -u
cd "$(dirname "$0")/.."

SOCK="${HERDR_SOCKET_PATH:-$HOME/.config/herdr/herdr.sock}"
DEMO="${TMPDIR:-/tmp}/kamiroh-scratch"; rm -rf "$DEMO"; mkdir -p "$DEMO"
BIN=./target/debug/kamiroh

nap() { sleep "$1"; }

# One request per connection — that is how Herdr answers.
call() { printf '%s\n' "$1" | nc -U "$SOCK" 2>/dev/null; }

SCRATCH=""
cleanup() {
  if [ -n "$SCRATCH" ]; then
    echo
    echo "--- closing scratch pane $SCRATCH ---"
    call "{\"id\":\"z\",\"method\":\"pane.close\",\"params\":{\"pane_id\":\"$SCRATCH\"}}" \
      | head -c 200
    echo
  fi
}
trap cleanup EXIT

# --- create the scratch pane, unfocused so it does not steal the cursor ------
echo "--- creating a scratch pane ---"
SPLIT=$(call '{"id":"1","method":"pane.split","params":{"direction":"down","focus":false,"ratio":0.2}}')
SCRATCH=$(printf '%s' "$SPLIT" | jq -r '.result.pane.pane_id // empty')
if [ -z "$SCRATCH" ]; then
  echo "could not create a pane; response was:"; printf '%s\n' "$SPLIT" | head -c 400; exit 1
fi
echo "scratch pane: $SCRATCH"

show() { # label
  local info
  info=$(call "{\"id\":\"g\",\"method\":\"pane.get\",\"params\":{\"pane_id\":\"$SCRATCH\"}}")
  printf '  %-22s agent=%s  agent_status=%s\n' "$1" \
    "$(printf '%s' "$info" | jq -c '.result.pane.agent // "null"')" \
    "$(printf '%s' "$info" | jq -c '.result.pane.agent_status // "null"')"
}

echo
echo "--- before kamiroh reports ---"
show "fresh pane:"

# --- kamiroh reports into that pane -----------------------------------------
echo
echo "--- kamiroh, bound to a local agent, reporting to $SCRATCH ---"
HERDR_PANE_ID="$SCRATCH" HERDR_SOCKET_PATH="$SOCK" \
KAMIROH_KEY_FILE="$DEMO/node.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
  $BIN < /dev/null > "$DEMO/a.txt" 2>&1 &
PID=$!
nap 1.5
grep -E '^(herdr:|kamiroh:)' "$DEMO/a.txt" | sed 's/^/  /'
show "after opening report:"
kill $PID 2>/dev/null; wait $PID 2>/dev/null

# --- catch `working` in flight ----------------------------------------------
# A prompt to an unreachable peer reports `working`, then stays there for as
# long as the dial takes — which is the window this polls for.
echo
echo "--- kamiroh prompting an UNREACHABLE peer (working should appear) ---"
UNREACHABLE=$(printf 'ff%.0s' $(seq 1 32))
printf 'this will not arrive\n' > "$DEMO/typed.txt"
HERDR_PANE_ID="$SCRATCH" HERDR_SOCKET_PATH="$SOCK" \
KAMIROH_KEY_FILE="$DEMO/node.key" KAMIROH_ALLOW_FILE="$DEMO/allow" \
KAMIROH_PEER="$UNREACHABLE@127.0.0.1:1" \
  $BIN < "$DEMO/typed.txt" > "$DEMO/b.txt" 2>&1 &
PID=$!

SEEN=""
for _ in $(seq 1 40); do
  STATE=$(call "{\"id\":\"g\",\"method\":\"pane.get\",\"params\":{\"pane_id\":\"$SCRATCH\"}}" \
          | jq -r '.result.pane.agent_status // "null"')
  case " $SEEN " in *" $STATE "*) ;; *) SEEN="$SEEN $STATE"; echo "  observed: $STATE" ;; esac
  nap 0.25
done
kill $PID 2>/dev/null; wait $PID 2>/dev/null

echo
echo "states observed while prompting an unreachable peer:$SEEN"
