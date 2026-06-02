//! `Style/MultilineMethodSignature` — flags multi-line method signatures.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineMethodSignature
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-y3h2
//! notes: >
//!   Detects `def`/`defs` nodes whose parameter list spans multiple lines.
//!   The offense is highlighted at the `def` keyword.
//!
//!   Guard: fires only when the method has at least one argument and the
//!   argument list is multi-line (the range from `def` through the closing `)`)
//!   contains a newline.
//!
//!   Autocorrect: not implemented (v1 gap); the cop is detect-only.
//!   Full autocorrect requires joining parameters onto a single line and
//!   checking the resulting line length against Layout/LineLength.
//! ```
//!
//! ## Matched shapes
//!
//! `def`/`defs` nodes that:
//! - Have at least one argument
//! - The header (from `def` to the closing `)` of the parameter list) spans
//!   more than one line
//!
//! ## No autocorrect
//!
//! Joining parameters into a single line while respecting line-length limits
//! requires Layout/LineLength config integration. Deferred to a follow-up.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid multi-line method signatures.";

#[derive(Default)]
pub struct MultilineMethodSignature;

#[cop(
    name = "Style/MultilineMethodSignature",
    description = "Avoid multi-line method signatures.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineMethodSignature {
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
    // Method must have at least one argument.
    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };

    // Check if the args node contains any arguments.
    let has_args = match cx.kind(args_node) {
        NodeKind::Args(list) => !cx.list(*list).is_empty(),
        _ => false,
    };
    if !has_args {
        return;
    }

    // Find the signature end: the closing `)` after the method name.
    // We look for the closing paren of the parameter list.
    let signature_end = match find_params_close_paren(node, args_node, cx) {
        Some(end) => end,
        None => {
            // No parentheses — method defined without parens, e.g. `def foo arg`
            // This cannot be multi-line in the RuboCop sense.
            return;
        }
    };

    // Check if the header (from node start to signature_end) is multi-line.
    let node_start = cx.range(node).start;
    let header_src = &cx.source()[node_start as usize..signature_end as usize];
    if !header_src.contains('\n') {
        return;
    }

    // Emit offense at the `def`/`defs` keyword (always single-line).
    let offense_range = {
        let kw = cx.loc(node).keyword();
        if kw != Range::ZERO {
            kw
        } else {
            cx.range(node)
        }
    };

    cx.emit_offense(offense_range, MSG, None);
}

/// Find the byte offset past the closing `)` of the parameter list.
///
/// Locates the `(` that opens the parameter list by searching backward from
/// the first argument's start position. This avoids mistaking a receiver
/// paren in `def (obj).foo(arg)` for the parameter-list paren.
fn find_params_close_paren(node: NodeId, args_node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let toks = cx.sorted_tokens();
    let node_end = cx.range(node).end;

    // Find the first argument's start to anchor the backward search.
    // The parameter-list `(` is the last LeftParen before first_arg_start.
    let first_arg_start = match cx.kind(args_node) {
        NodeKind::Args(list) => {
            let params = cx.list(*list);
            if params.is_empty() {
                return None;
            }
            cx.range(params[0]).start
        }
        _ => return None,
    };

    // Search backward: find the last LeftParen strictly before first_arg_start
    // and at or after node_start (skips receiver parens in `def (obj).foo`).
    let node_start = cx.range(node).start;
    let idx = toks.partition_point(|t| t.range.start < first_arg_start);
    let open_paren_pos = toks[..idx]
        .iter()
        .rev()
        .take_while(|t| t.range.start >= node_start)
        .find(|t| t.kind == SourceTokenKind::LeftParen)
        .map(|t| t.range.start)?;

    // Now find the matching `)` by counting nesting depth.
    let search_start = open_paren_pos + 1;
    let idx2 = toks.partition_point(|t| t.range.start < search_start);
    let mut depth: i32 = 1;
    for tok in &toks[idx2..] {
        if tok.range.start >= node_end {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::MultilineMethodSignature;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiline_signature() {
        test::<MultilineMethodSignature>().expect_offense(indoc! {"
            def foo(
            ^^^ Avoid multi-line method signatures.
              arg1,
              arg2
            )
              body
            end
        "});
    }

    #[test]
    fn flags_multiline_singleton_method_signature() {
        test::<MultilineMethodSignature>().expect_offense(indoc! {"
            def self.foo(
            ^^^ Avoid multi-line method signatures.
              arg1,
              arg2
            )
              body
            end
        "});
    }

    #[test]
    fn flags_multiline_signature_with_defaults() {
        test::<MultilineMethodSignature>().expect_offense(indoc! {"
            def foo(
            ^^^ Avoid multi-line method signatures.
              arg1,
              arg2 = 1
            )
              body
            end
        "});
    }

    #[test]
    fn accepts_single_line_signature() {
        test::<MultilineMethodSignature>().expect_no_offenses(indoc! {"
            def foo(arg1, arg2)
              body
            end
        "});
    }

    #[test]
    fn accepts_no_args() {
        test::<MultilineMethodSignature>().expect_no_offenses(indoc! {"
            def foo
              body
            end
        "});
    }

    #[test]
    fn accepts_empty_parens() {
        test::<MultilineMethodSignature>().expect_no_offenses(indoc! {"
            def foo()
              body
            end
        "});
    }

    #[test]
    fn accepts_multiline_body_single_line_signature() {
        test::<MultilineMethodSignature>().expect_no_offenses(indoc! {"
            def foo(arg1, arg2)
              x = 1
              y = 2
            end
        "});
    }

    // Regression: for `def (obj).foo(...)`, the `(` of the receiver must be
    // skipped. The param-list `(` comes after the method name `foo`.
    #[test]
    fn flags_multiline_singleton_with_parenthesized_receiver() {
        test::<MultilineMethodSignature>().expect_offense(indoc! {"
            def (obj).foo(
            ^^^  Avoid multi-line method signatures.
              arg1,
              arg2
            )
              body
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodSignature);
