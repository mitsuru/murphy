//! Starter template — cop with autocorrect via `cx.emit_edit`.
//!
//! Use this when porting a RuboCop cop that ships `autocorrect` /
//! `extend AutoCorrector`. The cop emits one offense + one edit per
//! match, and the test pins both with `expect_correction!`.
//!
//! Mirrors the shape of `Style/StringLiterals` and
//! `Layout/SpaceInsideParens` (the canonical in-tree autocorrect cops).
//! See `references/autocorrect.md` for safety rules and
//! `references/testing.md` for the `expect_correction!` grammar.

//! `Pack/MyAutocorrectCop` — rewrites single-quoted plain strings to
//! double-quoted form, but only when the body is unambiguously safe to
//! swap (no backslashes, no `#`, no embedded double quotes).
//!
//! ## Matched shapes
//! `Str` literals whose source representation is `'…'` and whose body
//! has none of `\\`, `#`, or `"`.
//!
//! ## Why this shape
//! Pedagogical starter — demonstrates emit_offense + emit_edit pairing
//! with a real safety gate. A production port would mirror
//! `Style/StringLiterals` instead of inventing this rule.
//!
//! ## Autocorrect
//! Safe by construction: the body is byte-for-byte the same under both
//! quote styles when the gate passes, so the swap is idempotent.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct MyAutocorrectCop;

#[cop(
    name = "Pack/MyAutocorrectCop",
    description = "Prefer double-quoted strings when the body is safe to swap.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MyAutocorrectCop {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // House-style defensive destructure: statically guaranteed today
        // by the `kind = "str"` attribute, but the `let-else` is cheap
        // insurance against a future kind-aliasing accident.
        let NodeKind::Str(_) = *cx.kind(node) else {
            return;
        };

        let range = cx.range(node);
        let src = cx.raw_source(range);

        // Only act on a basic single-quoted literal — skip `%q[…]`,
        // heredocs, `?x` char literals, etc. that may surface here.
        let Some(body) = src.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) else {
            return;
        };

        // Safety gate: the body must mean the same thing under both
        // quote styles. Any of these characters would change semantics
        // when re-wrapped in double quotes.
        if body.contains('\\') || body.contains('#') || body.contains('"') {
            // Reportable style violation, but no safe rewrite — emit
            // only the offense. Tests for this branch use
            // `expect_no_corrections!`.
            cx.emit_offense(range, "Prefer double-quoted strings.", None);
            return;
        }

        cx.emit_offense(range, "Prefer double-quoted strings.", None);
        cx.emit_edit(range, &format!("\"{body}\""));
    }
}

#[cfg(test)]
mod tests {
    use super::MyAutocorrectCop;
    use murphy_plugin_api::test_support::{
        expect_correction, expect_no_corrections, expect_no_offenses, indoc,
    };

    #[test]
    fn rewrites_safe_single_quoted_literal() {
        // `expect_correction!` pins the offense set (via caret annotations)
        // and the corrected source (the third argument) in one assertion.
        expect_correction!(
            MyAutocorrectCop,
            indoc! {r#"
                x = 'hello'
                    ^^^^^^^ Prefer double-quoted strings.
            "#},
            "x = \"hello\"\n"
        );
    }

    #[test]
    fn does_not_rewrite_when_body_has_escapes() {
        // Offense fires but no edit — `expect_no_corrections!` asserts
        // the edit set is empty. Pair with the offense fixture above to
        // pin both halves of the behaviour.
        expect_no_corrections!(MyAutocorrectCop, r"x = 'line\n'");
    }

    #[test]
    fn does_not_flag_already_double_quoted() {
        expect_no_offenses!(
            MyAutocorrectCop,
            indoc! {r#"
                x = "hello"
            "#}
        );
    }
}
