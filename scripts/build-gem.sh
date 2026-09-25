#!/bin/sh
# Build the `murphy` Ruby gem for one platform (C2; ADR 0048).
#
# Usage:
#   scripts/build-gem.sh [--platform <tag>] [--release]
#
#   --platform <tag>  one of x86_64-linux, aarch64-linux, x86_64-darwin,
#                     arm64-darwin. Default: auto-detected from uname.
#   --release         `cargo build --release` (default) vs debug build.
#                     Debug builds are for local `gem build` smoke tests only;
#                     published platform gems always use --release.
#
# The script compiles `murphy-cli`, stages the binary as
# `libexec/murphy-<tag>`, then runs `gem build` with
# `MURPHY_GEM_PLATFORM=<tag>` so the resulting
# `murphy-<version>-<tag>.gem` carries the matching payload.
set -eu

PLATFORM=""
PROFILE="release"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform) PLATFORM="$2"; shift 2 ;;
    --release) PROFILE="release"; shift ;;
    --debug) PROFILE="debug"; shift ;;
    *) echo "usage: $0 [--platform <tag>] [--release|--debug]" >&2; exit 2 ;;
  esac
done

if [ -z "$PLATFORM" ]; then
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "$ARCH" in
    x86_64|amd64) CPU="x86_64" ;;
    aarch64|arm64)
      case "$OS" in
        darwin*) CPU="arm64" ;;
        *) CPU="aarch64" ;;
      esac
      ;;
    *) echo "unsupported arch: $ARCH" >&2; exit 2 ;;
  esac
  case "$OS" in
    linux*) OS="linux" ;;
    darwin*) OS="darwin" ;;
    *) echo "unsupported os: $OS" >&2; exit 2 ;;
  esac
  PLATFORM="$CPU-$OS"
fi

case "$PLATFORM" in
  x86_64-linux|aarch64-linux|x86_64-darwin|arm64-darwin) ;;
  *) echo "unsupported platform: $PLATFORM" >&2; exit 2 ;;
esac

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

if [ "$PROFILE" = "release" ]; then
  cargo build --release -p murphy-cli
  BIN="$ROOT/target/release/murphy"
else
  cargo build -p murphy-cli
  BIN="$ROOT/target/debug/murphy"
fi

mkdir -p "$ROOT/libexec"
cp -f "$BIN" "$ROOT/libexec/murphy-$PLATFORM"
chmod 755 "$ROOT/libexec/murphy-$PLATFORM"

MURPHY_GEM_PLATFORM="$PLATFORM" gem build murphy.gemspec
