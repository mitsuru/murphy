//! `Lint/ImplicitStringConcatenation` — checks adjacent same-line string literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ImplicitStringConcatenation
//! upstream_version_checked: master
//! status: partial
//! gap_issues: [murphy-irhu]
//! notes: >
//!   Covers adjacent same-line `Str` parts lowered as `Dstr`, array/method
//!   argument hint messages, line-continuation non-offenses, and ` + `
//!   autocorrection between adjacent parts. Known v1 limitation: RuboCop's full
//!   formatting parity for nested interpolated strings, multiline display text,
//!   and triple-quote empty-string removal needs more string-literal delimiter /
//!   component metadata than the current cop uses from the plugin surface.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Combine %<lhs>s and %<rhs>s into a single string literal, rather than using implicit string concatenation.";
const FOR_ARRAY: &str = " Or, if they were intended to be separate array elements, separate them with a comma.";
const FOR_METHOD: &str = " Or, if they were intended to be separate method arguments, separate them with a comma.";

#[derive(Default)]
pub struct ImplicitStringConcatenation;

#[cop(
    name = "Lint/ImplicitStringConcatenation",
    description = "Checks adjacent same-line string literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ImplicitStringConcatenation {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dstr(parts) = *cx.kind(node) else {
            return;
        };
        for window in cx.list(parts).windows(2) {
            let [lhs, rhs] = window else {
                continue;
            };
            if !is_string_literal_part(*lhs, cx) || !is_string_literal_part(*rhs, cx) {
                continue;
            }
            if !same_line_gap(cx.range(*lhs).end, cx.range(*rhs).start, cx.source()) {
                continue;
            }
            if !ends_with_string_delimiter(*lhs, cx) {
                continue;
            }

            let range = Range {
                start: cx.range(*lhs).start,
                end: cx.range(*rhs).end,
            };
            let mut message = MSG
                .replace("%<lhs>s", display_string(*lhs, cx).as_str())
                .replace("%<rhs>s", display_string(*rhs, cx).as_str());
            if parent_is_array(node, cx) {
                message.push_str(FOR_ARRAY);
            } else if parent_is_send(node, cx) {
                message.push_str(FOR_METHOD);
            }
            cx.emit_offense(range, &message, None);

            let join_range = Range {
                start: cx.range(*lhs).end,
                end: cx.range(*rhs).start,
            };
            cx.emit_edit(join_range, " + ");
        }
    }
}

fn is_string_literal_part(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_))
}

fn ends_with_string_delimiter(node: NodeId, cx: &Cx<'_>) -> bool {
    // Test the UNtrimmed last byte against the opening delimiter. Trimming
    // trailing whitespace would mis-read a heredoc per-line `Str` part whose
    // raw source is `'\n` (literal apostrophe + newline) as ending in a `'`
    // delimiter, producing a false positive.
    let bytes = cx.raw_source(cx.range(node)).as_bytes();
    match (bytes.first().copied(), bytes.last().copied()) {
        (Some(b'\''), Some(last)) => last == b'\'',
        (Some(b'"'), Some(last)) => last == b'"',
        _ => false,
    }
}

fn display_string(node: NodeId, cx: &Cx<'_>) -> String {
    cx.raw_source(cx.range(node)).to_string()
}

fn parent_is_array(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node)
        .get()
        .is_some_and(|parent| matches!(cx.kind(parent), NodeKind::Array(_)))
}

fn parent_is_send(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node).get().is_some_and(|parent| {
        matches!(cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. })
    })
}

fn same_line_gap(lhs_end: u32, rhs_start: u32, source: &str) -> bool {
    !source.as_bytes()[lhs_end as usize..rhs_start as usize].contains(&b'\n')
}

murphy_plugin_api::submit_cop!(ImplicitStringConcatenation);

#[cfg(test)]
mod tests {
    use super::{same_line_gap, ImplicitStringConcatenation};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_adjacent_string_literals_on_same_line() {
        test::<ImplicitStringConcatenation>().expect_correction(
            indoc! {r#"
                class A; "abc" "def"; end
                         ^^^^^^^^^^^ Combine "abc" and "def" into a single string literal, rather than using implicit string concatenation.
            "#},
            "class A; \"abc\" + \"def\"; end\n",
        );
    }

    #[test]
    fn adds_array_and_method_argument_hints() {
        test::<ImplicitStringConcatenation>()
            .expect_offense(indoc! {r#"
                array = ["abc" "def"]
                         ^^^^^^^^^^^ Combine "abc" and "def" into a single string literal, rather than using implicit string concatenation. Or, if they were intended to be separate array elements, separate them with a comma.
            "#})
            .expect_offense(indoc! {r#"
                method("abc" "def")
                       ^^^^^^^^^^^ Combine "abc" and "def" into a single string literal, rather than using implicit string concatenation. Or, if they were intended to be separate method arguments, separate them with a comma.
            "#});
    }

    #[test]
    fn accepts_single_strings_and_line_continuation() {
        test::<ImplicitStringConcatenation>()
            .expect_no_offenses("\"abc\"\n")
            .expect_no_offenses(indoc! {r#"
                array = [
                  'abc'\
                  'def'
                ]
            "#});
    }

    #[test]
    fn accepts_squiggly_heredoc_with_interpolation() {
        // Mastodon FP: a squiggly heredoc with interpolations lowers to a Dstr
        // with adjacent per-line Str parts. At a line boundary the lhs raw
        // source is `'\n` — trimming the trailing newline made it look like it
        // ended with a `'` delimiter. RuboCop checks the UNtrimmed last char
        // (`\n` ≠ `'`), so there is no implicit concatenation. Clean.
        test::<ImplicitStringConcatenation>().expect_no_offenses(indoc! {r#"
            x = <<~SQL
              SELECT '#{name}'
              WHERE name = '#{name}'
            SQL
        "#});
    }

    #[test]
    fn same_line_gap_checks_only_gap_text() {
        let source = "prefix\n\"abc\" \"def\"\n\"ghi\"\n\"jkl\"\n";

        assert!(same_line_gap(12, 13, source));
        assert!(!same_line_gap(24, 25, source));
    }
}
