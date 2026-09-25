#!/usr/bin/env bash
# A5 persistent-cache gate (murphy-fmw.1.1, Phase 8 gate #3):
# second consecutive `murphy lint` run is 5x+ faster on a dispatch-bound corpus.
#
# Usage: scripts/perf/a5_cache_gate.sh [--keep]
#   Builds release, generates 50 large Ruby files (~750 KB), times two
#   consecutive runs with a fresh XDG_CACHE_HOME, and asserts speedup >= 5.
#   `--keep` preserves the temp dirs for inspection.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo build --release -p murphy-cli

corpus=$(mktemp -d -t murphy-a5-gate-corpus-XXXXXX)
cache=$(mktemp -d -t murphy-a5-gate-cache-XXXXXX)
keep=0
if [ "${1:-}" = "--keep" ]; then keep=1; fi
trap 'if [ "$keep" -eq 0 ]; then rm -rf "$corpus" "$cache"; else echo "kept: corpus=$corpus cache=$cache"; fi' EXIT

for i in $(seq 1 50); do
  {
    echo "# frozen_string_literal: true"
    echo ""
    for j in $(seq 1 200); do
      echo "def method_${i}_${j}(arg)"
      echo "  x = arg + ${j} + ${i}"
      echo "  y = x * 2"
      echo "  puts y"
      echo "  y"
      echo "end"
      if [ $((j % 7)) -eq 0 ]; then echo "debugger"; fi
      if [ $((j % 11)) -eq 0 ]; then echo "foo rescue nil"; fi
    done
  } > "$corpus/big_$i.rb"
done

export XDG_CACHE_HOME="$cache"
unset MURPHY_NO_CACHE || true

set +e
t0=$(date +%s%N)
./target/release/murphy lint --format json "$corpus" > /tmp/murphy-a5-gate-run1.json
t1=$(date +%s%N)
./target/release/murphy lint --format json "$corpus" > /tmp/murphy-a5-gate-run2.json
t2=$(date +%s%N)
set -e

if ! cmp -s /tmp/murphy-a5-gate-run1.json /tmp/murphy-a5-gate-run2.json; then
  echo "FAIL: consecutive runs differ" >&2
  exit 1
fi

ms1=$(( (t1 - t0) / 1000000 ))
ms2=$(( (t2 - t1) / 1000000 ))
echo "run1: ${ms1}ms run2: ${ms2}ms"
speedup_x100=$(( ms1 * 100 / (ms2 == 0 ? 1 : ms2) ))
echo "speedup: $((speedup_x100 / 100)).$(( (speedup_x100 % 100) / 10 ))$((speedup_x100 % 10))x"
./target/release/murphy cache stat

if [ "$ms2" -eq 0 ]; then echo "PASS (run2 under 1ms)"; exit 0; fi
if [ $(( ms1 )) -ge $(( ms2 * 5 )) ]; then
  echo "PASS: 5x gate met"
else
  echo "FAIL: need run1 >= 5*run2 (got ${ms1}ms vs ${ms2}ms)" >&2
  exit 1
fi
