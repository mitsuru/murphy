#!/bin/sh
# Create a git worktree and trust its mise config in one step.
#
# Fresh worktrees get a new path, so mise treats mise.toml as untrusted
# until acknowledged. Without trust, `mise exec` / `mise activate` provide
# no toolchain (rust/ruby) and fail with `Config files ... are not trusted`;
# in non-interactive shells there is no prompt, so the failure looks silent
# and downstream builds fail with `-lmruby` not found. This helper runs
# `mise trust` right after `git worktree add` so the step is never skipped.
#
# Usage:
#   scripts/worktree-add.sh <path> [<git-worktree-add-args>...]
#
# Example:
#   scripts/worktree-add.sh .claude/worktrees/mywork -b fix/mywork
set -eu

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <path> [<git-worktree-add-args>...]" >&2
  exit 2
fi

WORKTREE_PATH=$1
shift

git worktree add "$WORKTREE_PATH" "$@"
mise trust "$WORKTREE_PATH"
