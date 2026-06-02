//! `Style/EmptyHeredoc` — flags empty heredocs and suggests `''` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyHeredoc
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `str` nodes that are (a) empty (`""`) and (b) sourced from a heredoc
//!   (raw source starts with `<<`). A heredoc with a blank-line body parses to
//!   `(str "\n")` — non-empty — so it is correctly not flagged.
//!
//!   Autocorrect:
//!   - Replaces the heredoc opener token (e.g. `<<~EOS`) with `''`.
//!   - Removes the heredoc body and terminator lines using HeredocStart /
//!     HeredocEnd token pairing (FIFO order, same as other heredoc-scanning cops).
//!
//!   Gap: quote style is always `''` regardless of `Style/StringLiterals`
//!   configuration (cross-cop config awareness not yet implemented).
//!
//!   Disabled by default (`Enabled: pending` in RuboCop's default config).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! <<~EOS
//! EOS
//!
//! <<-EOS
//! EOS
//!
//! # good
//! ''
//!
//! # good (body has content)
//! <<~EOS
//!   something
//! EOS
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Use an empty string literal instead of heredoc.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyHeredoc;

#[cop(
    name = "Style/EmptyHeredoc",
    description = "Checks for using empty heredoc to reduce redundancy.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl EmptyHeredoc {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only Str nodes.
    let NodeKind::Str(str_id) = *cx.kind(node) else {
        return;
    };

    // Must be empty string.
    if !cx.string_str(str_id).is_empty() {
        return;
    }

    // Must be a heredoc: raw source starts with `<<`.
    let raw = cx.raw_source(cx.range(node));
    if !raw.starts_with("<<") {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect: find the matching HeredocStart/HeredocEnd for this node
    // and rewrite opener → `''`, remove body+terminator lines.
    let _ = autocorrect(node, cx);
}

fn autocorrect(node: NodeId, cx: &Cx<'_>) -> Option<()> {
    let node_start = cx.range(node).start;
    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find the HeredocStart token that corresponds to this node.
    // The node's range.start points to the `<<` opener token start.
    let heredoc_start_tok = toks
        .iter()
        .find(|t| t.kind == SourceTokenKind::HeredocStart && t.range.start == node_start)?;

    // Find the matching HeredocEnd by scanning forward from heredoc_start_tok,
    // counting paired starts (FIFO).
    let mut depth = 0u32;
    let mut heredoc_end_tok = None;
    let start_idx = toks.partition_point(|t| t.range.start < heredoc_start_tok.range.start);
    for tok in &toks[start_idx..] {
        match tok.kind {
            SourceTokenKind::HeredocStart => depth += 1,
            SourceTokenKind::HeredocEnd => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                if depth == 0 {
                    heredoc_end_tok = Some(*tok);
                    break;
                }
            }
            _ => {}
        }
    }
    let heredoc_end_tok = heredoc_end_tok?;

    // Edit 1: Replace the HeredocStart opener with `''`.
    cx.emit_edit(heredoc_start_tok.range, "''");

    // Edit 2: Remove the heredoc body (line after opener) and terminator line.
    // The body starts on the line after the opener. We need to find the `\n`
    // that ends the opener's source line, then skip past it.
    let body_start = line_end_inclusive(src, heredoc_start_tok.range.end);

    // Terminator line: find start of the line containing heredoc_end_tok.
    // Find start/end of terminator line.
    let _ = line_start(src, heredoc_end_tok.range.start);
    let term_line_end = line_end_inclusive(src, heredoc_end_tok.range.end);

    cx.emit_edit(
        Range {
            start: body_start,
            end: term_line_end,
        },
        "",
    );
    Some(())
}

/// Returns the byte offset of the start of the line containing `offset`.
fn line_start(src: &[u8], offset: u32) -> u32 {
    let mut i = offset as usize;
    while i > 0 && src[i - 1] != b'\n' {
        i -= 1;
    }
    i as u32
}

/// Returns the byte offset just past the `\n` at or after `offset`, or
/// `src.len()` if there is no trailing newline.
fn line_end_inclusive(src: &[u8], offset: u32) -> u32 {
    let mut i = offset as usize;
    while i < src.len() && src[i] != b'\n' {
        i += 1;
    }
    // Include the newline if present.
    if i < src.len() && src[i] == b'\n' {
        i += 1;
    }
    i as u32
}

#[cfg(test)]
mod tests {
    use super::EmptyHeredoc;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_squiggly_empty_heredoc() {
        test::<EmptyHeredoc>().expect_offense(indoc! {"
            <<~EOS
            ^^^^^^ Use an empty string literal instead of heredoc.
            EOS
        "});
    }

    #[test]
    fn flags_dash_empty_heredoc() {
        test::<EmptyHeredoc>().expect_offense(indoc! {"
            <<-EOS
            ^^^^^^ Use an empty string literal instead of heredoc.
            EOS
        "});
    }

    #[test]
    fn flags_plain_empty_heredoc() {
        test::<EmptyHeredoc>().expect_offense(indoc! {"
            <<EOS
            ^^^^^ Use an empty string literal instead of heredoc.
            EOS
        "});
    }

    #[test]
    fn accepts_heredoc_with_content() {
        test::<EmptyHeredoc>().expect_no_offenses(indoc! {"
            <<~EOS
              something
            EOS
        "});
    }

    #[test]
    fn accepts_empty_string_literal() {
        test::<EmptyHeredoc>().expect_no_offenses("''\n");
    }

    #[test]
    fn corrects_squiggly_empty_heredoc() {
        test::<EmptyHeredoc>().expect_correction(
            indoc! {"
                <<~EOS
                ^^^^^^ Use an empty string literal instead of heredoc.
                EOS
            "},
            "''\n",
        );
    }

    #[test]
    fn corrects_in_method_call() {
        test::<EmptyHeredoc>().expect_correction(
            indoc! {"
                do_something(<<~EOS)
                             ^^^^^^ Use an empty string literal instead of heredoc.
                EOS
            "},
            "do_something('')\n",
        );
    }
}

murphy_plugin_api::submit_cop!(EmptyHeredoc);
