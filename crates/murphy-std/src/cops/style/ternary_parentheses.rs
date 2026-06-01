//! `Style/TernaryParentheses` — checks for use of parentheses around ternary conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TernaryParentheses
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Three EnforcedStyle values are implemented:
//!     require_no_parentheses (default): flag parenthesized conditions.
//!     require_parentheses: flag unparenthesized conditions.
//!     require_parentheses_when_complex: flag complex without parens or simple with parens.
//!   AllowSafeAssignment (default: true): parenthesized conditions containing an assignment
//!   token `=` are allowed (detected via token scan — covers common cases).
//!   Parenthesized conditions map to `Unknown` in Murphy's AST (prism's ParenthesesNode
//!   is not yet translated). Detection: `Unknown` node whose source starts with `(`.
//!   Complexity classification:
//!     Non-complex: Lvar/Ivar/Cvar/Gvar/Const/Defined/Yield/Send/Csend that are NOT
//!     operator methods (or are the `[]` operator, which is non-complex per RuboCop).
//!     Complex: And/Or/Not, operator Send nodes.
//!   When condition is Unknown (parenthesized), complexity cannot be determined —
//!   `require_parentheses_when_complex` never flags Unknown conditions as "too many parens"
//!   (gap: false negatives for simple parenthesized conditions).
//!   Autocorrect safety:
//!     For removing parens, skipped if source contains English `and`/`or`/`not` operators
//!     (below ternary precedence), detected via raw source scan.
//!     When removing `)`, a space is inserted before `?` if the next char is `?`
//!     (prevents `bar?? a` invalid syntax).
//!   Multi-line ternary conditions (closing `)` on its own line) are skipped entirely —
//!   RuboCop's `only_closing_parenthesis_is_last_line?` guard.
//! ```
//!
//! ## Configuration
//!
//! - `EnforcedStyle`: `require_no_parentheses` (default), `require_parentheses`,
//!   `require_parentheses_when_complex`
//! - `AllowSafeAssignment` (bool, default `true`)
//!
//! ## Matched shapes
//!
//! Ternary `if` nodes (`cond ? a : b`).

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

const MSG: &str = "%<command>s parentheses for ternary conditions.";
const MSG_COMPLEX: &str =
    "%<command>s parentheses for ternary expressions with complex conditions.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TernaryParentheses;

#[derive(CopOptions)]
pub struct TernaryParenthesesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "require_no_parentheses",
        description = "Parentheses enforcement style for ternary conditions."
    )]
    pub enforced_style: TernaryStyle,

    #[option(
        name = "AllowSafeAssignment",
        default = true,
        description = "Allow parenthesized assignments in ternary conditions."
    )]
    pub allow_safe_assignment: bool,
}

#[derive(Default, CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TernaryStyle {
    #[option(value = "require_no_parentheses")]
    #[default]
    RequireNoParentheses,
    #[option(value = "require_parentheses")]
    RequireParentheses,
    #[option(value = "require_parentheses_when_complex")]
    RequireParenthesesWhenComplex,
}

#[cop(
    name = "Style/TernaryParentheses",
    description = "Check for parentheses around ternary conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = TernaryParenthesesOptions,
)]
impl TernaryParentheses {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<TernaryParenthesesOptions>();
        check(node, cx, &opts);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, opts: &TernaryParenthesesOptions) {
    if !cx.is_ternary(node) {
        return;
    }

    let NodeKind::If { cond, .. } = *cx.kind(node) else {
        return;
    };

    let is_paren = is_parenthesized(cond, cx);
    let cond_src = cx.raw_source(cx.range(cond));

    // Guard: skip if closing paren is on its own line (RuboCop's
    // `only_closing_parenthesis_is_last_line?`).
    if is_paren && only_closing_paren_is_last_line(cond_src) {
        return;
    }

    // Safe assignment check: `(bar = baz) ? a : b`.
    if is_paren && opts.allow_safe_assignment && is_safe_assignment(cond, cx) {
        return;
    }

    match opts.enforced_style {
        TernaryStyle::RequireNoParentheses => {
            if is_paren {
                let msg = MSG.replace("%<command>s", "Omit");
                cx.emit_offense(cx.range(node), &msg, None);
                // Autocorrect: remove parens if safe to do so.
                if !has_below_ternary_precedence(cond_src) {
                    emit_remove_parens(cond, cx);
                }
            }
        }
        TernaryStyle::RequireParentheses => {
            if !is_paren {
                let msg = MSG.replace("%<command>s", "Use");
                cx.emit_offense(cx.range(node), &msg, None);
                // Autocorrect: wrap condition in parens.
                cx.emit_edit(cx.range(cond), &format!("({cond_src})"));
            }
        }
        TernaryStyle::RequireParenthesesWhenComplex => {
            if !is_paren && is_complex_condition(cond, cx) {
                // Complex condition without parens — add them.
                let msg = MSG_COMPLEX.replace("%<command>s", "Use");
                cx.emit_offense(cx.range(node), &msg, None);
                cx.emit_edit(cx.range(cond), &format!("({cond_src})"));
            }
            // If is_paren: Unknown (parenthesized) — can't introspect inside,
            // so we conservatively skip flagging. See parity notes.
        }
    }
}

/// Returns `true` if `cond` is a parenthesized expression.
/// In Murphy's AST, prism's `ParenthesesNode` translates to `Unknown`.
/// We additionally confirm the source starts with `(` to avoid false positives
/// from other `Unknown` translations.
fn is_parenthesized(cond: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(cond), NodeKind::Unknown) {
        return false;
    }
    let src = cx.raw_source(cx.range(cond)).as_bytes();
    src.first() == Some(&b'(')
}

/// Returns `true` if the ternary condition is a safe assignment (contains `=`).
/// Detected via token scan of the condition range looking for an `Other` token
/// whose text is exactly `=` (distinguishes from `==`, `<=`, `>=`, `=>`).
fn is_safe_assignment(cond: NodeId, cx: &Cx<'_>) -> bool {
    let cond_range = cx.range(cond);
    let source = cx.source().as_bytes();
    for tok in cx.tokens_in(cond_range) {
        if tok.kind == SourceTokenKind::Other {
            let text = &source[tok.range.start as usize..tok.range.end as usize];
            if text == b"=" {
                return true;
            }
        }
    }
    false
}

/// Returns `true` if the parenthesized condition source has a closing `)` on
/// its own last line — RuboCop's `only_closing_parenthesis_is_last_line?` guard.
fn only_closing_paren_is_last_line(src: &str) -> bool {
    src.lines().last() == Some(")")
}

/// Returns `true` if the condition source contains English-language operators
/// that are below ternary precedence: `and`, `or`, `not`.
/// These make autocorrect unsafe (removing parens changes precedence).
fn has_below_ternary_precedence(src: &str) -> bool {
    for word in [b"and" as &[u8], b"or", b"not"] {
        let bytes = src.as_bytes();
        let mut start = 0usize;
        while start + word.len() <= bytes.len() {
            let rest = &bytes[start..];
            if let Some(pos) = rest.windows(word.len()).position(|w| w == word) {
                let abs = start + pos;
                let before_ok =
                    abs == 0 || (!bytes[abs - 1].is_ascii_alphabetic() && bytes[abs - 1] != b'_');
                let after_end = abs + word.len();
                let after_ok = after_end >= bytes.len()
                    || (!bytes[after_end].is_ascii_alphanumeric() && bytes[after_end] != b'_');
                if before_ok && after_ok {
                    return true;
                }
                start = abs + 1;
            } else {
                break;
            }
        }
    }
    false
}

/// Returns `true` if the condition is "complex" (requires parens for clarity).
/// Complex: And, Or, Not, operator Send (not `[]`).
/// Non-complex: Lvar/Ivar/Cvar/Gvar, Const, Defined, Yield, Send/Csend that
/// are non-operator calls or the `[]` operator.
fn is_complex_condition(cond: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(cond) {
        // Variable / constant / special literals — never complex.
        NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Const { .. }
        | NodeKind::Defined(_)
        | NodeKind::Yield(_) => false,

        // Send/Csend — complex only if operator method (excluding `[]`).
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            is_operator_method(cx.symbol_str(*method))
        }

        // Logical / negation — always complex.
        NodeKind::And { .. } | NodeKind::Or { .. } | NodeKind::Not(_) => true,

        // Unknown (parenthesized) — treated as non-complex. Used in
        // RequireParenthesesWhenComplex for the "add parens" check:
        // if we can't see inside, skip (don't add unnecessary parens).
        NodeKind::Unknown => false,

        // Everything else — not operator-like, treat as non-complex.
        _ => false,
    }
}

/// Returns `true` if `name` is an operator method (excluding `[]`).
fn is_operator_method(name: &str) -> bool {
    if name == "[]" {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    matches!(
        bytes[0],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'~' | b'&' | b'|' | b'^'
    )
}

/// Emit two surgical edits to remove surrounding parentheses from `cond`.
///
/// The `cond` range includes the outer `(` and `)`.
/// Edit 1: delete the opening `(` (first byte of cond_range).
/// Edit 2: delete the closing `)` (last byte of cond_range), OR replace with
/// space if the byte immediately following is `?` (prevents `bar?? a`).
fn emit_remove_parens(cond: NodeId, cx: &Cx<'_>) {
    let cond_range = cx.range(cond);
    let source = cx.source().as_bytes();

    // Edit 1: remove opening `(`.
    let open_range = Range {
        start: cond_range.start,
        end: cond_range.start + 1,
    };
    cx.emit_edit(open_range, "");

    // Edit 2: remove closing `)`, inserting a space if next char is `?`.
    let close_range = Range {
        start: cond_range.end - 1,
        end: cond_range.end,
    };
    let after_byte = source.get(cond_range.end as usize).copied().unwrap_or(0);
    let replacement = if after_byte == b'?' { " " } else { "" };
    cx.emit_edit(close_range, replacement);
}

#[cfg(test)]
mod tests {
    use super::{TernaryParentheses, TernaryParenthesesOptions, TernaryStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- require_no_parentheses (default) -----

    #[test]
    fn no_parens_flags_parenthesized_simple() {
        // Offense range: the ternary node (starting at the condition).
        test::<TernaryParentheses>().expect_correction(
            indoc! {"
                foo = (bar?) ? a : b
                      ^^^^^^^^^^^^^^ Omit parentheses for ternary conditions.
            "},
            "foo = bar? ? a : b\n",
        );
    }

    #[test]
    fn no_parens_flags_parenthesized_complex() {
        test::<TernaryParentheses>().expect_correction(
            indoc! {"
                foo = (bar && baz) ? a : b
                      ^^^^^^^^^^^^^^^^^^^^ Omit parentheses for ternary conditions.
            "},
            "foo = bar && baz ? a : b\n",
        );
    }

    #[test]
    fn no_parens_removes_parens_with_space_before_question_mark() {
        // `(bar?)? a : b` — after removing `)`, next char is `?`.
        // A space must be inserted to prevent `bar?? a`.
        test::<TernaryParentheses>().expect_correction(
            indoc! {"
                (bar?)? a : b
                ^^^^^^^^^^^^^ Omit parentheses for ternary conditions.
            "},
            "bar? ? a : b\n",
        );
    }

    #[test]
    fn no_parens_accepts_simple_unparenthesized() {
        test::<TernaryParentheses>().expect_no_offenses("foo = bar? ? a : b\n");
    }

    #[test]
    fn no_parens_accepts_complex_unparenthesized() {
        test::<TernaryParentheses>().expect_no_offenses("foo = bar && baz ? a : b\n");
    }

    #[test]
    fn no_parens_allows_safe_assignment() {
        // AllowSafeAssignment: true (default)
        test::<TernaryParentheses>().expect_no_offenses("foo = (bar = baz) ? a : b\n");
    }

    #[test]
    fn no_parens_flags_when_safe_assignment_disabled() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireNoParentheses,
            allow_safe_assignment: false,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                foo = (bar = baz) ? a : b
                      ^^^^^^^^^^^^^^^^^^^ Omit parentheses for ternary conditions.
            "});
    }

    // ----- require_parentheses -----

    #[test]
    fn require_parens_flags_simple_unparenthesized() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParentheses,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_correction(
                indoc! {"
                    foo = bar? ? a : b
                          ^^^^^^^^^^^^ Use parentheses for ternary conditions.
                "},
                "foo = (bar?) ? a : b\n",
            );
    }

    #[test]
    fn require_parens_flags_complex_unparenthesized() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParentheses,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_correction(
                indoc! {"
                    foo = bar && baz ? a : b
                          ^^^^^^^^^^^^^^^^^^ Use parentheses for ternary conditions.
                "},
                "foo = (bar && baz) ? a : b\n",
            );
    }

    #[test]
    fn require_parens_accepts_parenthesized() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParentheses,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_no_offenses("foo = (bar?) ? a : b\n");
    }

    // ----- require_parentheses_when_complex -----

    #[test]
    fn complex_when_flags_and_without_parens() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParenthesesWhenComplex,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_correction(
                indoc! {"
                    foo = bar && baz ? a : b
                          ^^^^^^^^^^^^^^^^^^ Use parentheses for ternary expressions with complex conditions.
                "},
                "foo = (bar && baz) ? a : b\n",
            );
    }

    #[test]
    fn complex_when_flags_or_without_parens() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParenthesesWhenComplex,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_correction(
                indoc! {"
                    foo = bar || baz ? a : b
                          ^^^^^^^^^^^^^^^^^^ Use parentheses for ternary expressions with complex conditions.
                "},
                "foo = (bar || baz) ? a : b\n",
            );
    }

    #[test]
    fn complex_when_accepts_simple_without_parens() {
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParenthesesWhenComplex,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_no_offenses("foo = bar? ? a : b\n");
    }

    #[test]
    fn complex_when_accepts_parenthesized_unknown() {
        // `(bar && baz) ? a : b` — Unknown (parenthesized, opaque) — not flagged.
        let opts = TernaryParenthesesOptions {
            enforced_style: TernaryStyle::RequireParenthesesWhenComplex,
            allow_safe_assignment: true,
        };
        test::<TernaryParentheses>()
            .with_options(&opts)
            .expect_no_offenses("foo = (bar && baz) ? a : b\n");
    }

    // ----- Non-ternary (should not flag) -----

    #[test]
    fn accepts_non_ternary_if() {
        test::<TernaryParentheses>().expect_no_offenses(indoc! {"
            if (bar?)
              a
            end
        "});
    }

    // ----- English operators (unsafe autocorrect) -----

    #[test]
    fn no_parens_flags_but_no_autocorrect_for_english_or() {
        // `(foo or bar) ? a : b` has English `or` — offense but no autocorrect.
        test::<TernaryParentheses>().expect_offense(indoc! {"
            (foo or bar) ? a : b
            ^^^^^^^^^^^^^^^^^^^^ Omit parentheses for ternary conditions.
        "});
    }
}
murphy_plugin_api::submit_cop!(TernaryParentheses);
