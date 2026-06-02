//! `Style/EmptyMethod` — enforces consistent formatting of empty method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyMethod
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle:
//!   - `compact` (default) — empty methods must be on a single line
//!     (`def foo; end`). Multiline empty methods are flagged and corrected.
//!   - `expanded` — empty methods must be on multiple lines (`def foo\nend`).
//!     Single-line empty methods are flagged and corrected.
//!
//!   A method with a comment body is NOT considered empty and is never flagged.
//!   Guard: `cx.comments_in_range(cx.range(node))` must be empty.
//!
//!   Endless methods (`def foo = expr`) have no `end` keyword and are always
//!   skipped (`cx.loc(node).end_keyword() == Range::ZERO`).
//!
//!   Autocorrect:
//!   - compact: replaces the whole node with `def <signature>; end`.
//!   - expanded: replaces the whole node with `def <signature>\n<indent>end`.
//!   The signature is extracted verbatim from the source (between `def ` keyword
//!   end and the start of the body/end-keyword).
//!
//!   Gap: RuboCop's compact style skips autocorrect when the result would exceed
//!   `Layout/LineLength Max`. Murphy always corrects (cross-cop config gap).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

const MSG_COMPACT: &str = "Put empty method definitions on a single line.";
const MSG_EXPANDED: &str = "Put the `end` of empty method definitions on the next line.";

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "compact")]
    Compact,
    #[option(value = "expanded")]
    Expanded,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "compact",
        description = "Whether empty methods should be compact (`def foo; end`) or expanded."
    )]
    pub enforced_style: EnforcedStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyMethod;

#[cop(
    name = "Style/EmptyMethod",
    description = "Checks the formatting of empty method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl EmptyMethod {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must not have a body.
    if cx.def_body(node).get().is_some() {
        return;
    }

    // Skip endless methods (no `end` keyword).
    if cx.loc(node).end_keyword() == Range::ZERO {
        return;
    }

    // Skip methods that contain comments (not truly empty).
    if !cx.comments_in_range(cx.range(node)).is_empty() {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    let (offense, msg) = match opts.enforced_style {
        EnforcedStyle::Compact => {
            // compact: multiline is an offense
            (!cx.is_single_line(node), MSG_COMPACT)
        }
        EnforcedStyle::Expanded => {
            // expanded: single-line is an offense
            (cx.is_single_line(node), MSG_EXPANDED)
        }
    };

    if !offense {
        return;
    }

    // Offense range: first line of the node.
    cx.emit_offense(first_line_range(node, cx), msg, None);

    // Autocorrect.
    autocorrect(node, opts.enforced_style, cx);
}

/// Returns the range of the first source line of the node.
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}

fn autocorrect(node: NodeId, style: EnforcedStyle, cx: &Cx<'_>) {
    let signature = extract_signature(node, cx);

    let replacement = match style {
        EnforcedStyle::Compact => {
            format!("def {}; end", signature)
        }
        EnforcedStyle::Expanded => {
            let indent = " ".repeat(def_column(node, cx));
            format!("def {}\n{}end", signature, indent)
        }
    };

    cx.emit_edit(cx.range(node), &replacement);
}

/// Extract the method signature verbatim from source.
/// For `def foo(bar)\nend`, returns `foo(bar)`.
/// For `def foo(bar); end`, returns `foo(bar)`.
/// Uses `cx.loc(node).name` as the start of the name, then extends to the
/// end of the args (scanning for `)` or newline/semicolon after the name).
fn extract_signature<'a>(node: NodeId, cx: &'a Cx<'a>) -> &'a str {
    let src = cx.source();
    let src_bytes = src.as_bytes();
    let node_loc = cx.loc(node);

    // Name range: the method name (and possibly receiver for `defs`).
    // For `def foo(bar)`, name is `foo`. For `def self.foo(bar)`, name is `foo`.
    // We want from after `def ` keyword end to end of signature.
    let keyword_range = node_loc.keyword();
    // signature start = keyword end + any spaces
    let mut sig_start = keyword_range.end as usize;
    while sig_start < src_bytes.len() && src_bytes[sig_start] == b' ' {
        sig_start += 1;
    }

    // Find end of signature: walk from sig_start to first `\n` or `;` (excluding parens).
    let sig_end = find_signature_end(src_bytes, sig_start, cx.range(node).end as usize);

    &src[sig_start..sig_end]
}

/// Find the end of the method signature: everything up to (but not including)
/// the first newline or `;` that is not inside parentheses.
fn find_signature_end(src: &[u8], start: usize, node_end: usize) -> usize {
    let mut i = start;
    let mut paren_depth = 0i32;
    while i < node_end {
        match src[i] {
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth -= 1;
                if paren_depth < 0 {
                    break;
                }
                if paren_depth == 0 {
                    // After closing paren — include it and stop.
                    return i + 1;
                }
            }
            b'\n' | b';' if paren_depth == 0 => {
                // End of signature at newline or semicolon (outside parens).
                return i;
            }
            b' ' | b'\t' if paren_depth == 0 => {
                // Trailing whitespace before \n or ; — stop here.
                let peek = src[i..].iter().position(|&b| b != b' ' && b != b'\t');
                let next_non_ws = peek.map_or(i, |p| i + p);
                if next_non_ws >= node_end
                    || src[next_non_ws] == b'\n'
                    || src[next_non_ws] == b';'
                {
                    return i;
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Returns the 0-based column of the `def` keyword (Unicode-aware).
fn def_column(node: NodeId, cx: &Cx<'_>) -> usize {
    let node_start = cx.range(node).start as usize;
    let src = cx.source();
    let line_start = src[..node_start].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..node_start].chars().count()
}

#[cfg(test)]
mod tests {
    use super::{EmptyMethod, EnforcedStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- compact style (default) ---

    #[test]
    fn flags_multiline_empty_def_compact() {
        test::<EmptyMethod>().expect_offense(indoc! {"
            def foo(bar)
            ^^^^^^^^^^^^ Put empty method definitions on a single line.
            end
        "});
    }

    #[test]
    fn flags_multiline_empty_singleton_def_compact() {
        test::<EmptyMethod>().expect_offense(indoc! {"
            def self.foo(bar)
            ^^^^^^^^^^^^^^^^^ Put empty method definitions on a single line.
            end
        "});
    }

    #[test]
    fn accepts_single_line_empty_def_compact() {
        test::<EmptyMethod>().expect_no_offenses("def foo(bar); end\n");
    }

    #[test]
    fn accepts_def_with_body() {
        test::<EmptyMethod>().expect_no_offenses(indoc! {"
            def foo(bar)
              baz
            end
        "});
    }

    #[test]
    fn accepts_def_with_comment() {
        test::<EmptyMethod>().expect_no_offenses(indoc! {"
            def foo(bar)
              # comment
            end
        "});
    }

    #[test]
    fn corrects_multiline_empty_def_to_compact() {
        test::<EmptyMethod>().expect_correction(
            indoc! {"
                def foo(bar)
                ^^^^^^^^^^^^ Put empty method definitions on a single line.
                end
            "},
            "def foo(bar); end\n",
        );
    }

    #[test]
    fn corrects_multiline_empty_no_args_to_compact() {
        test::<EmptyMethod>().expect_correction(
            indoc! {"
                def foo
                ^^^^^^^ Put empty method definitions on a single line.
                end
            "},
            "def foo; end\n",
        );
    }

    // --- expanded style ---

    #[test]
    fn flags_single_line_empty_def_expanded() {
        test::<EmptyMethod>()
            .with_options(&Options { enforced_style: EnforcedStyle::Expanded })
            .expect_offense(indoc! {"
                def foo(bar); end
                ^^^^^^^^^^^^^^^^^ Put the `end` of empty method definitions on the next line.
            "});
    }

    #[test]
    fn accepts_multiline_empty_def_expanded() {
        test::<EmptyMethod>()
            .with_options(&Options { enforced_style: EnforcedStyle::Expanded })
            .expect_no_offenses(indoc! {"
                def foo(bar)
                end
            "});
    }

    #[test]
    fn corrects_single_line_empty_def_to_expanded() {
        test::<EmptyMethod>()
            .with_options(&Options { enforced_style: EnforcedStyle::Expanded })
            .expect_correction(
                indoc! {"
                    def foo(bar); end
                    ^^^^^^^^^^^^^^^^^ Put the `end` of empty method definitions on the next line.
                "},
                "def foo(bar)\nend\n",
            );
    }
}

murphy_plugin_api::submit_cop!(EmptyMethod);
