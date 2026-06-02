//! `Style/LambdaCall` — enforces consistent lambda invocation syntax.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/LambdaCall
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Two EnforcedStyle values are implemented:
//!     call (default): require `lambda.call(args)` over `lambda.(args)`.
//!     braces: require `lambda.(args)` over `lambda.call(args)`.
//!   Discrimination between the two syntaxes uses the selector (loc.name):
//!   Prism provides no message_loc for implicit-call `f.()`, so the translator
//!   sets loc.name to Range::ZERO for implicit calls. For explicit `f.call(args)`,
//!   loc.name spans the `call` token.
//!   Also handles `csend` (`lambda&.call(args)` is flagged when `braces` style
//!   is enforced). Mirrors RuboCop's `alias on_csend on_send`.
//!   Autocorrect: whole-node replacement (receiver + dot + method + args).
//!   The cop does not check whether the receiver is actually a lambda -- it
//!   flags any `.call(...)` or `.(...)` call, matching RuboCop's behavior.
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # EnforcedStyle: call (default)
//! # bad
//! lambda.(x, y)
//!
//! # good
//! lambda.call(x, y)
//!
//! # EnforcedStyle: braces
//! # bad
//! lambda.call(x, y)
//!
//! # good
//! lambda.(x, y)
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, NodeList, cop};

const MSG: &str = "Prefer the use of `%PREFER%` over `%CURRENT%`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct LambdaCall;

/// Enforcement style for lambda call syntax.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Require `lambda.call(args)` (default).
    #[default]
    #[option(value = "call")]
    Call,
    /// Require `lambda.(args)`.
    #[option(value = "braces")]
    Braces,
}

/// Cop options for LambdaCall.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "call",
        description = "Whether to prefer `lambda.call(args)` or `lambda.(args)` style."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/LambdaCall",
    description = "Use lambda.call(...) instead of lambda.(...) (or vice versa).",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl LambdaCall {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Extract receiver, method, args from send or csend.
    let (recv_id, method, args) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            let Some(recv_id) = receiver.get() else { return };
            (recv_id, method, args)
        }
        NodeKind::Csend { receiver, method, args } => (receiver, method, args),
        _ => return,
    };

    if cx.symbol_str(method) != "call" {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    // Distinguish implicit `f.(args)` from explicit `f.call(args)`:
    //   - `call_operator_loc` returns None for implicit-call (Prism omits the
    //     call operator from its side table for `.()` invocations)
    //   - `call_operator_loc` returns Some for explicit `f.call(args)`
    // Distinguish implicit `f.(args)` from explicit `f.call(args)`:
    // For implicit `f.(args)`, Prism provides no message_loc (no selector text),
    // so the translator sets loc.name (selector) to Range::ZERO.
    // For explicit `f.call(args)`, loc.name spans the `call` token.
    let is_implicit = cx.selector(node) == murphy_plugin_api::Range::ZERO;

    let offense = match opts.enforced_style {
        // `call` style: flag when the call is implicit (braces form)
        EnforcedStyle::Call => is_implicit,
        // `braces` style: flag when the call is explicit (call form)
        EnforcedStyle::Braces => !is_implicit,
    };

    if !offense {
        return;
    }

    let current_src = cx.raw_source(cx.range(node));
    let preferred = build_preferred(node, recv_id, args, is_implicit, cx);
    let msg = MSG
        .replace("%PREFER%", &preferred)
        .replace("%CURRENT%", current_src);

    cx.emit_offense(cx.range(node), &msg, None);

    // Autocorrect: replace the whole node with the preferred form.
    cx.emit_edit(cx.range(node), &preferred);
}

/// Build the preferred source for the node.
///
/// - `is_implicit == true` => currently `f.(args)` => build `f.call(args)`
/// - `is_implicit == false` => currently `f.call(args)` => build `f.(args)` or `f.()`
fn build_preferred(
    node: NodeId,
    recv_id: NodeId,
    args: NodeList,
    is_implicit: bool,
    cx: &Cx<'_>,
) -> String {
    let receiver_src = cx.raw_source(cx.range(recv_id));

    // Find the dot: for both forms, we scan for the `.` between receiver end
    // and node end using the token stream.
    let recv_end = cx.range(recv_id).end;
    let node_end = cx.range(node).end;
    let dot = dot_between(recv_end, node_end, cx);

    let arg_list = cx.list(args);

    if is_implicit {
        // Convert `f.(args)` => `f.call(args)` or `f.call` if no args
        if arg_list.is_empty() {
            format!("{receiver_src}{dot}call")
        } else {
            let args_src = arg_list
                .iter()
                .map(|&id| cx.raw_source(cx.range(id)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{receiver_src}{dot}call({args_src})")
        }
    } else {
        // Convert `f.call(args)` => `f.(args)` or `f.()` if no args
        if arg_list.is_empty() {
            format!("{receiver_src}{dot}()")
        } else {
            let args_src = arg_list
                .iter()
                .map(|&id| cx.raw_source(cx.range(id)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{receiver_src}{dot}({args_src})")
        }
    }
}

/// Find the `.` or `&.` source between `from` and `until_end` by scanning tokens.
fn dot_between(from: u32, until_end: u32, cx: &Cx<'_>) -> String {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in toks[idx..].iter().take_while(|t| t.range.start < until_end) {
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text == b"." || text == b"&." {
            return std::str::from_utf8(text).unwrap_or(".").to_owned();
        }
    }
    ".".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, LambdaCall, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- EnforcedStyle: call (default) ---

    #[test]
    fn flags_implicit_call_with_args() {
        test::<LambdaCall>().expect_offense(indoc! {"
            lambda.(x, y)
            ^^^^^^^^^^^^^ Prefer the use of `lambda.call(x, y)` over `lambda.(x, y)`.
        "});
    }

    #[test]
    fn corrects_implicit_call_with_args() {
        test::<LambdaCall>().expect_correction(
            indoc! {"
                lambda.(x, y)
                ^^^^^^^^^^^^^ Prefer the use of `lambda.call(x, y)` over `lambda.(x, y)`.
            "},
            "lambda.call(x, y)\n",
        );
    }

    #[test]
    fn flags_implicit_call_no_args() {
        test::<LambdaCall>().expect_offense(indoc! {"
            lambda.()
            ^^^^^^^^^ Prefer the use of `lambda.call` over `lambda.()`.
        "});
    }

    #[test]
    fn corrects_implicit_call_no_args() {
        test::<LambdaCall>().expect_correction(
            indoc! {"
                lambda.()
                ^^^^^^^^^ Prefer the use of `lambda.call` over `lambda.()`.
            "},
            "lambda.call\n",
        );
    }

    #[test]
    fn accepts_explicit_call_with_args() {
        test::<LambdaCall>().expect_no_offenses("lambda.call(x, y)\n");
    }

    #[test]
    fn accepts_explicit_call_no_args() {
        test::<LambdaCall>().expect_no_offenses("lambda.call\n");
    }

    // --- EnforcedStyle: braces ---

    fn braces_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::Braces,
        }
    }

    #[test]
    fn flags_explicit_call_with_args_braces_style() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_offense(indoc! {"
                lambda.call(x, y)
                ^^^^^^^^^^^^^^^^^ Prefer the use of `lambda.(x, y)` over `lambda.call(x, y)`.
            "});
    }

    #[test]
    fn corrects_explicit_call_with_args_braces_style() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_correction(
                indoc! {"
                    lambda.call(x, y)
                    ^^^^^^^^^^^^^^^^^ Prefer the use of `lambda.(x, y)` over `lambda.call(x, y)`.
                "},
                "lambda.(x, y)\n",
            );
    }

    #[test]
    fn flags_explicit_call_no_args_braces_style() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_offense(indoc! {"
                lambda.call
                ^^^^^^^^^^^ Prefer the use of `lambda.()` over `lambda.call`.
            "});
    }

    #[test]
    fn corrects_explicit_call_no_args_braces_style() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_correction(
                indoc! {"
                    lambda.call
                    ^^^^^^^^^^^ Prefer the use of `lambda.()` over `lambda.call`.
                "},
                "lambda.()\n",
            );
    }

    #[test]
    fn accepts_implicit_call_braces_style() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_no_offenses("lambda.(x, y)\n");
    }

    // --- idempotency ---

    #[test]
    fn corrected_explicit_call_is_idempotent() {
        test::<LambdaCall>().expect_no_offenses("lambda.call(x, y)\n");
    }

    #[test]
    fn corrected_implicit_call_braces_is_idempotent() {
        test::<LambdaCall>()
            .with_options(&braces_opts())
            .expect_no_offenses("lambda.(x, y)\n");
    }

    // --- does not check receiver type ---

    #[test]
    fn flags_any_receiver_dot_implicit_call() {
        // The cop flags any `.()` call regardless of receiver type
        test::<LambdaCall>().expect_offense(indoc! {"
            foo.(1)
            ^^^^^^^ Prefer the use of `foo.call(1)` over `foo.(1)`.
        "});
    }
}

murphy_plugin_api::submit_cop!(LambdaCall);


