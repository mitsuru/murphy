# Final-review fix report

## Finding addressed

In `crates/murphy-std/src/cops/style/invertible_unless_condition.rs`, moved
`cx.options_or_default::<InvertibleUnlessConditionOptions>()` and
`effective_inverse_methods` after the cheap `is_unless`, ternary, and
`NodeKind::If` guards. Unless nodes still decode options exactly once before
the first use in `invertible`; ordinary `if` nodes now return without option
decoding, cloning, or map construction.

## Scope review

- No ABI changes.
- No Rails YAML changes.
- No `.beads/.gitignore` changes.
- No changes to the closed Beads issue.
- No unrelated files changed by this fix.

## Verification

- `cargo test -p murphy-std --lib invertible_unless_condition`: 16 passed, 0 failed.
- `cargo clippy -p murphy-std --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.
- Self-review confirmed the diff only relocates the two option-processing statements within `check`.
