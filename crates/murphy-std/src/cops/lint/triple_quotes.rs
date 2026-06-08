//! `Lint/TripleQuotes` — Checks for strings delimited by multiple quotes.
//!
//! Ruby allows adjacent string literals to be implicitly concatenated. A string
//! starting and ending with multiple quotes (3, 5, 7, etc.) is actually just
//! empty strings concatenated with the real content, producing the same result.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/TripleQuotes
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `dstr` nodes (interpolated strings formed by adjacent string
//!   concatenation) whose source starts with 3+ quote characters. Both
//!   double-quoted (`"""`) and single-quoted (`'''`) forms are detected.
//!   Nested triple quotes inside interpolation are also handled.
//! ```
//!
//! ## Matched shapes
//!
//! - `"""a string"""` — triple-double-quoted string — offense
//! - `'''a string'''` — triple-single-quoted string — offense
//! - `"""""` — quintuple quotes — offense
//! - `"a string"` — normal single-quoted — no offense
//! - `"#{interpolation}"` — normal interpolation — no offense
//! - `"a""b"` — implicit concatenation of non-empty strings — no offense
//!
//! ## No autocorrect
//!
//! Removing the extra quotes requires human judgement about formatting intent.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Delimiting a string with multiple quotes has no effect, use a single quote instead.";

fn has_empty_str_child(children: &[NodeId], cx: &Cx<'_>) -> bool {
    children.iter().any(|&child| {
        matches!(cx.kind(child), NodeKind::Str(sid) if cx.string_str(*sid).is_empty())
    })
}

#[derive(Default)]
pub struct TripleQuotes;

#[cop(
    name = "Lint/TripleQuotes",
    description = "Checks for strings delimited by multiple quotes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TripleQuotes {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dstr(children) = *cx.kind(node) else {
            unreachable!()
        };
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        let num_quotes = src.chars().take_while(|&c| c == '"' || c == '\'').count();
        if num_quotes < 3 {
            return;
        }
        let child_nodes = cx.list(children);
        if !has_empty_str_child(child_nodes, cx) {
            return;
        }
        cx.emit_offense(
            Range {
                start: node_range.start,
                end: node_range.start + num_quotes as u32,
            },
            MSG,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::TripleQuotes;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_triple_double_quotes_one_line() {
        test::<TripleQuotes>().expect_offense(indoc! {r#"
            """a string"""
            ^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
        "#});
    }

    #[test]
    fn flags_triple_single_quotes_one_line() {
        test::<TripleQuotes>().expect_offense(indoc! {r#"
            '''a string'''
            ^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
        "#});
    }

    #[test]
    fn flags_triple_double_quotes_multi_line() {
        test::<TripleQuotes>().expect_offense(indoc! {r#"
            """
            ^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
              a string
            """
        "#});
    }

    #[test]
    fn flags_only_quotes() {
        test::<TripleQuotes>().expect_offense(indoc! {r#"
            """"""
            ^^^^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
        "#});
    }

    #[test]
    fn flags_quintuple_quotes() {
        test::<TripleQuotes>().expect_offense(indoc! {r#"
            """""
            ^^^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
              a string
            """""
        "#});
    }

    #[test]
    fn flags_nested_triple_quotes_in_interpolation() {
        test::<TripleQuotes>().expect_offense(indoc! {r##"
            str = "#{'''abc'''}"
                     ^^^ Delimiting a string with multiple quotes has no effect, use a single quote instead.
        "##});
    }

    #[test]
    fn accepts_normal_string() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r#"
            "a string"
        "#});
    }

    #[test]
    fn accepts_interpolation() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r##"
            str = "#{abc}"
        "##});
    }

    #[test]
    fn accepts_whitespace_separated_quotes() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r#"
            " " " " " "
        "#});
    }

    #[test]
    fn accepts_heredoc() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r#"
            str = <<~STRING
              a string
              #{interpolation}
            STRING
        "#});
    }

    #[test]
    fn accepts_implicit_concatenation() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r#"
            '' ''
        "#});
    }

    #[test]
    fn accepts_implicit_concatenation_non_empty() {
        test::<TripleQuotes>().expect_no_offenses(indoc! {r#"
            'a''b''c'
        "#});
    }
}

murphy_plugin_api::submit_cop!(TripleQuotes);
