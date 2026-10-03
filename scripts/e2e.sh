#!/bin/sh
# Runs inside the e2e job's emulator: install and start the sample app, then replay its flows.
# Failures are also reported as annotations, which are readable without access to the job log.
set -u
cd "$(dirname "$0")/../examples/android-sample"
mdh=../../target/release/mdh

annotate() {
  # One annotation per failing command, newlines encoded the way workflow commands need them.
  printf '::error title=%s::%s\n' "$1" "$(awk '{ printf "%s%%0A", $0 }' "$2" | cut -c1-60000)"
}

run() {
  title=$1
  shift
  out=$(mktemp)
  if "$@" >"$out" 2>&1; then
    cat "$out"
  else
    status=$?
    cat "$out"
    annotate "$title (exit $status)" "$out"
    exit "$status"
  fi
}

run devices "$mdh" devices
run "mdh run" "$mdh" run
run flows "$mdh" flow run login-success login-wrong-password settings-bluetooth compose-greeting messages-scroll troubles-layout \
  --junit flows.xml --step-timeout 30 --timeout 10
