//! `RSpec/ExampleLength` — caps the line count of an example block's
//! body. Mirrors RuboCop-RSpec's cop of the same name.
//!
//! ## Matched shapes
//!
//! Dispatched on `NodeKind::Block` and gates on
//! [`is_example_call`](crate::helpers::is_example_call) — the block's
//! call must be a bare `it` / `specify` / `example`. Other blocks
//! (`describe`, `context`, `before`, …) are skipped: this rule
//! specifically polices example bodies, not surrounding scaffolding.
//!
//! ## Line counting
//!
//! Counts source lines inside the body (between `do` and `end`, not
//! including them). Implementation: take the body's range, slice
//! `cx.raw_source(range)`, count `'\n'` bytes + 1. Examples:
//!
//! - `it { foo }` — body is `foo`, 1 line.
//! - `it do; a; b; c; end` — body covers `a; b; c`, 1 line (semicolons,
//!   not newlines).
//! - `it do\n  a\n  b\nend` — body covers `a\n  b`, 2 lines.
//!
//! An `it do ... end` with an empty body (no body node) is treated as
//! 0 lines and never emits.
//!
//! ## Option
//!
//! `max` (default `5`, matching RuboCop) — bodies whose line count
//! exceeds `max` are flagged. Runtime option wiring (murphy-9cr.9) is
//! not yet plumbed through `Cx`; v1 honours the `Default` (same
//! staging as `Style/StringLiterals` and `Example/TodoFormat`).
//!
//! ## No autocorrect
//!
//! Splitting an oversized example is a refactor that needs human
//! judgement (which assertions move, which setup belongs in
//! `before`); the cop reports and leaves the fix to the user.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

use super::helpers::is_example_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExampleLength;

/// Cop options for [`ExampleLength`]. The schema is exported via
/// `#[derive(CopOptions)]` for the host's validation gate; runtime
/// option access (murphy-9cr.9) is not yet wired through `Cx`, so the
/// `Default` (Max = 5) is what fires at runtime today.
#[derive(CopOptions)]
pub struct ExampleLengthOptions {
    #[option(
        default = 5,
        description = "Maximum number of lines in an example body."
    )]
    pub max: i64,
}

#[cop(
    name = "RSpec/ExampleLength",
    description = "Caps the line count of an example body (it / specify / example).",
    default_severity = "warning",
    default_enabled = true,
    options = ExampleLengthOptions
)]
impl ExampleLength {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return; // empty body — never long enough to exceed any max.
        };

        let opts = ExampleLengthOptions::default();
        let line_count = count_lines(cx.raw_source(cx.range(body_id)));
        if line_count <= opts.max as usize {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            &format!(
                "Example has too many lines ({line_count}/{max})",
                max = opts.max
            ),
            None,
        );
    }
}

/// Count source lines spanned by `text` — `'\n'` count + 1 unless `text`
/// is empty. A body like `"a\n  b"` is 2 lines; `"a; b"` is 1 line;
/// `""` is 0 lines (caller already short-circuits empty bodies, but
/// this keeps the helper total).
fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.bytes().filter(|&b| b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::{ExampleLength, count_lines};
    use murphy_plugin_api::test_support::{indoc, run_cop};

    /// `run_cop` only dispatches the one cop type so every emission is
    /// already a `RSpec/ExampleLength` offense — no per-name filter
    /// needed.
    fn hits(source: &str) -> usize {
        run_cop::<ExampleLength>(source).len()
    }

    #[test]
    fn count_lines_handles_basic_shapes() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("foo"), 1);
        assert_eq!(count_lines("a; b"), 1);
        assert_eq!(count_lines("a\nb"), 2);
        assert_eq!(count_lines("a\nb\nc"), 3);
        // Trailing newline counts: "a\n" spans the `a` line plus the
        // line after the newline — kept distinct so an extra blank
        // line in the body is accounted for.
        assert_eq!(count_lines("a\n"), 2);
    }

    #[test]
    fn flags_body_exceeding_default_max() {
        // 6-line body, default Max = 5 — must emit exactly once.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn does_not_flag_body_at_default_max() {
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn handles_specify_and_example_aliases() {
        let src = indoc! {r#"
            specify "x" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
            example "y" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 2);
    }

    #[test]
    fn ignores_non_example_blocks() {
        // `describe` is grouping scaffolding, not an example.
        let src = indoc! {r#"
            describe Widget do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_explicit_receiver_it_form() {
        // `Other.it "x" do ... end` — non-bare receiver belongs to
        // some other DSL.
        let src = indoc! {r#"
            Other.it "x" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_empty_body() {
        // `it "x" do end` — body is None, never long enough to flag.
        let src = indoc! {r#"
            it "x" do
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn flags_brace_form_block() {
        // RSpec accepts `it { ... }` as well as `it do ... end`; both
        // parse to `NodeKind::Block`. Newlines inside the braces feed
        // the line count the same way.
        let src = indoc! {r#"
            it "works" {
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            }
        "#};
        assert_eq!(hits(src), 1);
    }
}
