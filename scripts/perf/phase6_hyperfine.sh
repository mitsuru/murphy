#!/usr/bin/env bash
set -euo pipefail

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine is required" >&2
  exit 2
fi

if ! command -v rubocop >/dev/null 2>&1; then
  echo "rubocop is required" >&2
  exit 2
fi

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
corpus_dir="$root_dir/crates/murphy-cli/tests/fixtures/builtin_only_project"
export_dir=""

#   phase6_hyperfine.sh [<corpus-dir>] [--export-dir <dir>]  (flags any order)
while [ "$#" -gt 0 ]; do
  case "$1" in
    --export-dir)
      export_dir=${2:-}
      if [ -z "$export_dir" ]; then
        echo "--export-dir requires a directory argument" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      corpus_dir=$1
      shift
      ;;
  esac
done

if [ ! -d "$corpus_dir" ]; then
  echo "phase6 corpus not found: $corpus_dir" >&2
  exit 2
fi

cd "$root_dir"

cargo build --release

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

if [ -n "$export_dir" ]; then
  mkdir -p "$export_dir"
fi

for n in 1 20 100; do
  run_dir="$tmp_dir/n$n"
  mkdir -p "$run_dir"
  for i in $(seq 1 "$n"); do
    cp -R "$corpus_dir" "$run_dir/project_$i"
  done

  hyperfine \
    --warmup 2 \
    --ignore-failure \
    --show-output \
    --export-json "$tmp_dir/phase6-n$n.json" \
    "./target/release/murphy lint $run_dir" \
    "rubocop --format json $run_dir"

  if [ -n "$export_dir" ]; then
    cp -f "$tmp_dir/phase6-n$n.json" "$export_dir/phase6-n$n.json"
  fi
done

if [ -n "$export_dir" ]; then
  echo "perf results written to $export_dir"
else
  echo "perf results written under $tmp_dir"
fi
