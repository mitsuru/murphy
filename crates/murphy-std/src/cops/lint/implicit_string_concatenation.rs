//! `Lint/ImplicitStringConcatenation` — checks adjacent same-line string literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ImplicitStringConcatenation
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_dstr` / `each_bad_cons`: adjacent same-line
//!   `Str` / `Dstr` parts lowered as `Dstr` (including a nested interpolated
//!   `Dstr` part such as `"string#{x}"` in `"foo""string#{x}""bar"`),
//!   the same-line check, the closing-delimiter guard (single/double quotes
//!   only, so `%` literals are ignored), array/method hint suffixes, the
//!   multiline display form (inspected string content, e.g. `"ab\nc"`), and
//!   empty-string removal for triple-quote adjacency (`"""string"""` corrects
//!   to `"string"`). Regular autocorrect joins the gap with ` + `.
//!   Backslash line continuation stays a non-offense (the gap holds a
//!   newline). `Csend` parents also get the method hint although RuboCop
//!   checks `send` only; a harmless superset for `&.` calls.
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

            // RuboCop removes an empty adjacent part (`"""string"""` holds
            // `""` parts) instead of joining with ` + `.
            if is_empty_string(*lhs, cx) {
                cx.emit_edit(cx.range(*lhs), "");
            } else if is_empty_string(*rhs, cx) {
                cx.emit_edit(cx.range(*rhs), "");
            } else {
                let join_range = Range {
                    start: cx.range(*lhs).end,
                    end: cx.range(*rhs).start,
                };
                cx.emit_edit(join_range, " + ");
            }
        }
    }
}

fn is_string_literal_part(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_))
}

/// RuboCop's `lhs_node.value == ''` — only a plain empty `Str` part. A
/// `Dstr` part (possibly interpolated) never counts as empty.
fn is_empty_string(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Str(id) if cx.string_str(id).is_empty())
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
    let raw = cx.raw_source(cx.range(node));
    if raw.contains('\n') {
        // RuboCop's `display_str`: a part spanning lines is shown as the
        // inspected string content (`"ab\nc"`), not the raw multiline text.
        inspect_content(&str_content(node, cx))
    } else {
        raw.to_string()
    }
}

/// RuboCop's `str_content`: the plain `Str` values joined together.
/// Interpolation fragments (`Begin`, `Send`, ...) contribute `""`.
fn str_content(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        NodeKind::Str(id) => cx.string_str(id).to_string(),
        NodeKind::Dstr(parts) => cx.list(parts).iter().map(|child| str_content(*child, cx)).collect(),
        _ => String::new(),
    }
}

/// Ruby `String#inspect` for message display: double-quoted with `\`, `"`,
/// and control escapes. Matches for the newline-bearing contents this cop
/// displays; exotic codepoints may escape differently than CRuby.
fn inspect_content(content: &str) -> String {
    format!("{content:?}")
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
    use super::{inspect_content, same_line_gap, ImplicitStringConcatenation};
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
    fn flags_adjacent_interpolated_string_on_same_line() {
        test::<ImplicitStringConcatenation>().expect_correction(
            indoc! {r#"
                "string#{x}" "def"
                ^^^^^^^^^^^^^^^^^^ Combine "string#{x}" and "def" into a single string literal, rather than using implicit string concatenation.
            "#},
            "\"string#{x}\" + \"def\"\n",
        );
    }

    #[test]
    fn flags_multiple_concatenations_with_nested_interpolation() {
        test::<ImplicitStringConcatenation>().expect_correction(
            indoc! {r#"
                "foo""string#{x}""bar"
                ^^^^^^^^^^^^^^^^^ Combine "foo" and "string#{x}" into a single string literal, rather than using implicit string concatenation.
                     ^^^^^^^^^^^^^^^^^ Combine "string#{x}" and "bar" into a single string literal, rather than using implicit string concatenation.
            "#},
            "\"foo\" + \"string#{x}\" + \"bar\"\n",
        );
    }

    #[test]
    fn removes_empty_strings_in_triple_quoted_concatenation() {
        test::<ImplicitStringConcatenation>().expect_correction(
            indoc! {r#"
                """string"""
                ^^^^^^^^^^ Combine "" and "string" into a single string literal, rather than using implicit string concatenation.
                  ^^^^^^^^^^ Combine "string" and "" into a single string literal, rather than using implicit string concatenation.
            "#},
            "\"string\"\n",
        );
    }

    #[test]
    fn accepts_single_strings_and_line_continuation() {
        test::<ImplicitStringConcatenation>()
            .expect_no_offenses("\"abc\"\n")
            .expect_no_offenses("'abc\ndef'\n")
            .expect_no_offenses(indoc! {r#"
                array = [
                  'abc'\
                  'def'
                ]
            "#});
    }

    #[test]
    fn accepts_single_string_with_interpolations() {
        test::<ImplicitStringConcatenation>()
            .expect_no_offenses("array = [\"abc#{something}def#{something_else}\"]\n");
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
    fn multiline_display_inspects_string_content() {
        assert_eq!(inspect_content("ab\nc"), "\"ab\\nc\"");
        assert_eq!(inspect_content("string\n"), "\"string\\n\"");
        assert_eq!(inspect_content("plain"), "\"plain\"");
    }

    #[test]
    fn same_line_gap_checks_only_gap_text() {
        let source = "prefix\n\"abc\" \"def\"\n\"ghi\"\n\"jkl\"\n";

        assert!(same_line_gap(12, 13, source));
        assert!(!same_line_gap(24, 25, source));
    }
}
