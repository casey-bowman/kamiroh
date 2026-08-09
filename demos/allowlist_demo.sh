#!/bin/bash
# Exercises every allowlist source the binary accepts, including the two that
# must refuse to start.
set -u
cd "$(dirname "$0")/.."

DEMO="${TMPDIR:-/tmp}/kamiroh-allowlist"
rm -rf "$DEMO"; mkdir -p "$DEMO"
BIN=./target/debug/kamiroh

# This session runs inside a Herdr pane, so HERDR_* would be inherited by every
# node started here and they would report their agent state into the *real*
# pane you are looking at. Unset it: a demo must not touch a live session.
unset HERDR_PANE_ID HERDR_ENV HERDR_SOCKET_PATH HERDR_WORKSPACE_ID HERDR_TAB_ID

KEY="$DEMO/node.key"
ID=cb1b755a7d4d6330665717449a886d58270b289746135c33d531038846dc9141

nap() { perl -e 'select(undef, undef, undef, shift)' "$1"; }

# Starts the node, prints its `allowing:` line, then stops it.
starts_with() { # label, env assignments applied by caller
  local label="$1"; shift
  local log="$DEMO/$label.txt"
  KAMIROH_KEY_FILE="$KEY" "$@" $BIN > "$log" 2>&1 &
  local pid=$!
  for _ in $(seq 1 100); do
    grep -q '^serving' "$log" 2>/dev/null && break
    nap 0.1
  done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  printf '  %s\n' "$(grep '^allowing:' "$log" || echo "DID NOT START:  $(head -1 "$log")")"
}

# Runs the node expecting it to exit non-zero before serving.
refuses() { # label, env assignments applied by caller
  local label="$1"; shift
  local log="$DEMO/$label.txt"
  KAMIROH_KEY_FILE="$KEY" "$@" $BIN > "$log" 2>&1
  local code=$?
  if [ "$code" -eq 0 ]; then
    printf '  UNEXPECTEDLY STARTED (exit 0)\n'
  else
    printf '  exit %s: %s\n' "$code" "$(head -1 "$log")"
  fi
}

echo "--- 1. allowlist file with one peer ---"
printf '# laptop\n%s\n' "$ID" > "$DEMO/allow"
starts_with file-ok env KAMIROH_ALLOW_FILE="$DEMO/allow"

echo "--- 2. allowlist file absent ---"
starts_with file-missing env KAMIROH_ALLOW_FILE="$DEMO/nonexistent"

echo "--- 3. allowlist file of only comments ---"
printf '# nobody yet\n\n' > "$DEMO/empty"
starts_with file-empty env KAMIROH_ALLOW_FILE="$DEMO/empty"

echo "--- 4. KAMIROH_ALLOW overrides the file ---"
printf '%s\n' "$ID" > "$DEMO/allow"
starts_with env-override env KAMIROH_ALLOW_FILE="$DEMO/allow" KAMIROH_ALLOW="$ID"

echo "--- 5. malformed file: must refuse to start ---"
printf '%s\ntruncated-id\n' "$ID" > "$DEMO/bad"
refuses file-malformed env KAMIROH_ALLOW_FILE="$DEMO/bad"

echo "--- 6. world-writable file: must refuse to start ---"
printf '%s\n' "$ID" > "$DEMO/writable"
chmod 666 "$DEMO/writable"
refuses file-writable env KAMIROH_ALLOW_FILE="$DEMO/writable"
