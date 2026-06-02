//! `Style/EndlessMethod` — avoid multi-line endless method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EndlessMethod
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the core `allow_single_line` (default), `allow_always`, and
//!   `disallow` styles:
//!
//!   - `allow_single_line` (default): flags endless method definitions that span
//!     more than one line. Single-line endless methods are allowed.
//!   - `allow_always`: no offense is raised for any endless method.
//!   - `disallow`: flags all endless method definitions regardless of length.
//!
//!   Detection: `def` nodes without an `end` keyword (`cx.loc(node).end_keyword()
//!   == Range::ZERO`) are endless methods.
//!
//!   Autocorrect for `allow_single_line` (multiline flag) and `disallow`: rewrites
//!   the endless method to the standard `def … end` form.
//!
//!   Deferred (gaps):
//!     - `require_single_line` style (enforce endless for single-line methods)
//!     - `require_always` style (enforce endless whenever possible)
//!     - Autocorrect for `require_*` styles
//!     - `use_heredoc?` check (skip when body contains heredocs)
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (allow_single_line default)
//! def foo = some_very_long_expression \
//!             .that_spans_multiple_lines
//!
//! # good
//! def foo = bar
//! def foo
//!   bar
//! end
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_MULTI_LINE: &str = "Avoid endless method definitions with multiple lines.";
const MSG_DISALLOW: &str = "Avoid endless method definitions.";

/// Enforced style for `Style/EndlessMethod`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EndlessMethodStyle {
    /// Only single-line endless methods are allowed (default).
    #[default]
    #[option(value = "allow_single_line")]
    AllowSingleLine,
    /// All endless methods are allowed.
    #[option(value = "allow_always")]
    AllowAlways,
    /// All endless methods are disallowed.
    #[option(value = "disallow")]
    Disallow,
}

/// Configuration options for `Style/EndlessMethod`.
#[derive(CopOptions)]
pub struct EndlessMethodOptions {
    #[option(
        name = "EnforcedStyle",
        default = "allow_single_line",
        description = "The enforced style for endless method definitions."
    )]
    pub enforced_style: EndlessMethodStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct EndlessMethod;

#[cop(
    name = "Style/EndlessMethod",
    description = "Avoid the use of multi-lined endless method definitions.",
    default_severity = "warning",
    default_enabled = false,
    options = EndlessMethodOptions,
)]
impl EndlessMethod {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Check if this is an endless method (no `end` keyword).
    let is_endless = cx.loc(node).end_keyword() == Range::ZERO;

    let opts = cx.options_or_default::<EndlessMethodOptions>();

    match opts.enforced_style {
        EndlessMethodStyle::AllowAlways => {
            // No offense for any endless method.
        }
        EndlessMethodStyle::AllowSingleLine => {
            // Flag endless methods that span multiple lines.
            if is_endless && cx.is_multiline(node) {
                cx.emit_offense(cx.range(node), MSG_MULTI_LINE, None);
                autocorrect_to_multiline(node, cx);
            }
        }
        EndlessMethodStyle::Disallow => {
            // Flag all endless methods.
            if is_endless {
                cx.emit_offense(cx.range(node), MSG_DISALLOW, None);
                autocorrect_to_multiline(node, cx);
            }
        }
    }
}

/// Autocorrect: rewrite endless `def foo = expr` → `def foo\n  expr\nend`.
fn autocorrect_to_multiline(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Def {
        receiver,
        name,
        args,
        body,
    } = *cx.kind(node)
    else {
        return;
    };

    let Some(body_id) = body.get() else {
        return;
    };

    let name_str = cx.symbol_str(name);

    // Build receiver prefix: `self.` or `obj.`
    let receiver_part = if let Some(recv_id) = receiver.get() {
        format!("{}.{}", cx.raw_source(cx.range(recv_id)), name_str)
    } else {
        name_str.to_string()
    };

    // Build argument portion: Prism's ArgumentsNode range does not include
    // the surrounding parentheses, so we must wrap them explicitly.
    let args_part = {
        let NodeKind::Args(arg_list) = *cx.kind(args) else {
            return;
        };
        if cx.list(arg_list).is_empty() {
            String::new()
        } else {
            format!("({})", cx.raw_source(cx.range(args)))
        }
    };

    let body_src = cx.raw_source(cx.range(body_id));

    let replacement = format!("def {receiver_part}{args_part}\n  {body_src}\nend");
    cx.emit_edit(cx.range(node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::{EndlessMethod, EndlessMethodOptions, EndlessMethodStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_single_line_endless_method() {
        // Default allow_single_line: single-line endless method is OK.
        test::<EndlessMethod>().expect_no_offenses("def foo = bar\n");
    }

    #[test]
    fn accepts_regular_multiline_method() {
        test::<EndlessMethod>().expect_no_offenses(indoc! {"
            def foo
              bar
            end
        "});
    }

    #[test]
    fn accepts_regular_singleline_method() {
        test::<EndlessMethod>().expect_no_offenses("def foo; bar; end\n");
    }

    #[test]
    fn flags_disallow_style_endless_single_line() {
        test::<EndlessMethod>()
            .with_options(&EndlessMethodOptions {
                enforced_style: EndlessMethodStyle::Disallow,
            })
            .expect_offense(indoc! {"
                def foo = bar
                ^^^^^^^^^^^^^ Avoid endless method definitions.
            "});
    }

    #[test]
    fn autocorrects_disallow_style_endless() {
        test::<EndlessMethod>()
            .with_options(&EndlessMethodOptions {
                enforced_style: EndlessMethodStyle::Disallow,
            })
            .expect_correction(
                indoc! {"
                    def foo = bar
                    ^^^^^^^^^^^^^ Avoid endless method definitions.
                "},
                "def foo\n  bar\nend\n",
            );
    }

    #[test]
    fn accepts_allow_always_style() {
        test::<EndlessMethod>()
            .with_options(&EndlessMethodOptions {
                enforced_style: EndlessMethodStyle::AllowAlways,
            })
            .expect_no_offenses("def foo = bar\n");
    }
}

murphy_plugin_api::submit_cop!(EndlessMethod);
