#!/usr/bin/env bash
set -euo pipefail

if ! command -v rubocop >/dev/null 2>&1; then
  echo "rubocop is required" >&2
  exit 2
fi

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_dir=${1:-"$root_dir/crates/murphy-cli/tests/fixtures/phase6_project"}

if [ ! -d "$source_dir" ]; then
  echo "phase6 corpus not found: $source_dir" >&2
  exit 2
fi

cd "$root_dir"

cargo build --release

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

cp -R "$source_dir" "$work_dir/murphy"
cp -R "$source_dir" "$work_dir/rubocop"

./target/release/murphy lint --fix "$work_dir/murphy" >/tmp/murphy-phase6-fix.json || true
rubocop -a "$work_dir/rubocop" >/tmp/rubocop-phase6-fix.txt || true

diff -ru "$work_dir/rubocop" "$work_dir/murphy" || true
