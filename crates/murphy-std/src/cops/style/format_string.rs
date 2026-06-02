//! `Style/FormatString` -- enforces use of a single string formatting utility.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FormatString
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: format (default), sprintf, percent.
//!   Detection:
//!     - `sprintf(fmt, args...)` with >=2 args (nil/Kernel receiver)
//!     - `format(fmt, args...)` with >=2 args (nil/Kernel receiver)
//!     - `str % args` where str is a string literal
//!     - `recv % {array|hash}` where receiver is non-nil
//!   Autocorrect:
//!     - `format` <-> `sprintf`: selector rename only (always safe).
//!     - `format`/`sprintf` -> `percent`: not implemented (structural rearrangement).
//!     - `percent` -> `format`/`sprintf`: not implemented.
//!   Gaps vs RuboCop:
//!     - Autocorrect to/from `percent` style is not implemented.
//!     - RuboCop's `variable_argument?` guard for string variables is not replicated.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (default style: format)
//! sprintf("hello %s", name)
//! "hello %s" % name  # (string literal LHS)
//!
//! # good
//! format("hello %s", name)
//! ```
//!
//! ## Autocorrect
//!
//! Only `format` <-> `sprintf` selector renames are autocorrected.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FormatString;

/// Enforced formatting style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FmtMethod {
    /// `Kernel#format` (default).
    #[default]
    #[option(value = "format")]
    Format,
    /// `Kernel#sprintf`.
    #[option(value = "sprintf")]
    Sprintf,
    /// `String#%`.
    #[option(value = "percent")]
    Percent,
}

impl FmtMethod {
    fn as_str(self) -> &'static str {
        match self {
            FmtMethod::Format => "format",
            FmtMethod::Sprintf => "sprintf",
            FmtMethod::Percent => "String#%",
        }
    }
}

#[derive(CopOptions)]
pub struct FormatStringOptions {
    #[option(
        name = "EnforcedStyle",
        default = "format",
        description = "The preferred string formatting method: `format`, `sprintf`, or `percent`."
    )]
    pub enforced_style: FmtMethod,
}

#[cop(
    name = "Style/FormatString",
    description = "Enforce the use of Kernel#sprintf, Kernel#format or String#%.",
    default_severity = "warning",
    default_enabled = true,
    options = FormatStringOptions,
)]
impl FormatString {
    /// Check `format(...)`, `sprintf(...)`, and `str % args` calls.
    #[on_node(kind = "send", methods = ["format", "sprintf", "%"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let method_name = match cx.method_name(node) {
            Some(m) => m,
            None => return,
        };
        if method_name == "%" {
            check_send_percent(node, cx);
        } else {
            check_send_format_sprintf(node, cx);
        }
    }
}

/// Returns true if this send has nil/Kernel receiver (top-level function call).
fn is_kernel_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.call_receiver(node).get() {
        None => true,
        Some(recv) => {
            matches!(cx.kind(recv), NodeKind::Const { name, .. }
                if cx.symbol_str(*name) == "Kernel")
        }
    }
}

fn check_send_format_sprintf(node: NodeId, cx: &Cx<'_>) {
    // Must have >=2 args (format string + at least one arg, matching RuboCop).
    let args = cx.call_arguments(node);
    if args.len() < 2 {
        return;
    }

    // Receiver must be nil or Kernel.
    if !is_kernel_receiver(node, cx) {
        return;
    }

    let opts = cx.options_or_default::<FormatStringOptions>();
    let method_name = cx.method_name(node).unwrap_or("format");

    let detected = if method_name == "sprintf" {
        FmtMethod::Sprintf
    } else {
        FmtMethod::Format
    };

    if detected == opts.enforced_style {
        return;
    }

    // Offense on the selector.
    let selector = cx.selector(node);
    let offense_range = if selector != Range::ZERO {
        selector
    } else {
        cx.range(node)
    };

    let prefer = opts.enforced_style.as_str();
    let current = detected.as_str();
    let msg = format!("Favor `{prefer}` over `{current}`.");
    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: format <-> sprintf is a trivial selector rename.
    // percent <-> format/sprintf requires structural rewrite -- not implemented.
    match (detected, opts.enforced_style) {
        (FmtMethod::Format, FmtMethod::Sprintf) | (FmtMethod::Sprintf, FmtMethod::Format) => {
            cx.emit_edit(offense_range, opts.enforced_style.as_str());
        }
        _ => {
            // No autocorrect for percent-style transitions.
        }
    }
}

fn check_send_percent(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<FormatStringOptions>();

    // If enforced style is already percent, no offense.
    if opts.enforced_style == FmtMethod::Percent {
        return;
    }

    // RuboCop's detection for `%`:
    //   1. `(send {str dstr} :% ...)` -- string literal on LHS (any rhs)
    //   2. `(send !nil? :% {array hash})` -- non-nil receiver, array/hash rhs
    let recv = cx.call_receiver(node).get();
    let args = cx.call_arguments(node);

    let flagged = if let Some(recv_node) = recv {
        match cx.kind(recv_node) {
            NodeKind::Str(_) | NodeKind::Dstr(_) => true,
            _ => {
                // Non-string receiver: only flag if rhs is array or hash.
                if let Some(&first_arg) = args.first() {
                    matches!(cx.kind(first_arg), NodeKind::Array(_) | NodeKind::Hash(_))
                } else {
                    false
                }
            }
        }
    } else {
        false
    };

    if !flagged {
        return;
    }

    // Offense on the selector.
    let selector = cx.selector(node);
    let offense_range = if selector != Range::ZERO {
        selector
    } else {
        cx.range(node)
    };

    let prefer = opts.enforced_style.as_str();
    let msg = format!("Favor `{prefer}` over `String#%`.");
    cx.emit_offense(offense_range, &msg, None);
    // No autocorrect for percent -> format/sprintf (structural rewrite needed).
}

#[cfg(test)]
mod tests {
    use super::{FormatString, FormatStringOptions, FmtMethod};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_format_with_single_arg() {
        // Only one argument -- not flagged (that's RedundantFormat territory).
        test::<FormatString>().expect_no_offenses("format('hello')\n");
    }

    #[test]
    fn no_offense_format_already_preferred() {
        test::<FormatString>().expect_no_offenses("format('%s', name)\n");
    }

    #[test]
    fn no_offense_format_on_arbitrary_receiver() {
        test::<FormatString>().expect_no_offenses("obj.format('%s', name)\n");
    }

    #[test]
    fn no_offense_sprintf_in_sprintf_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Sprintf,
            })
            .expect_no_offenses("sprintf('%s', name)\n");
    }

    #[test]
    fn no_offense_percent_in_percent_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Percent,
            })
            .expect_no_offenses("'%s' % name\n");
    }

    // --- format mode (default) ---

    #[test]
    fn flags_sprintf_in_format_mode() {
        test::<FormatString>().expect_offense(indoc! {r#"
            sprintf('%s', name)
            ^^^^^^^ Favor `format` over `sprintf`.
        "#});
    }

    #[test]
    fn flags_percent_operator_in_format_mode() {
        test::<FormatString>().expect_offense(indoc! {r#"
            '%s' % name
                 ^ Favor `format` over `String#%`.
        "#});
    }

    #[test]
    fn flags_percent_with_array_rhs_in_format_mode() {
        test::<FormatString>().expect_offense(indoc! {r#"
            '%s %s' % [name, other]
                    ^ Favor `format` over `String#%`.
        "#});
    }

    // --- sprintf mode ---

    #[test]
    fn flags_format_in_sprintf_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Sprintf,
            })
            .expect_offense(indoc! {r#"
                format('%s', name)
                ^^^^^^ Favor `sprintf` over `format`.
            "#});
    }

    #[test]
    fn flags_percent_in_sprintf_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Sprintf,
            })
            .expect_offense(indoc! {r#"
                '%s' % name
                     ^ Favor `sprintf` over `String#%`.
            "#});
    }

    // --- percent mode ---

    #[test]
    fn flags_format_in_percent_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Percent,
            })
            .expect_offense(indoc! {r#"
                format('%s', name)
                ^^^^^^ Favor `String#%` over `format`.
            "#});
    }

    #[test]
    fn flags_sprintf_in_percent_mode() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Percent,
            })
            .expect_offense(indoc! {r#"
                sprintf('%s', name)
                ^^^^^^^ Favor `String#%` over `sprintf`.
            "#});
    }

    // --- Autocorrect: format <-> sprintf ---

    #[test]
    fn corrects_sprintf_to_format() {
        test::<FormatString>().expect_correction(
            indoc! {r#"
                sprintf('%s', name)
                ^^^^^^^ Favor `format` over `sprintf`.
            "#},
            "format('%s', name)\n",
        );
    }

    #[test]
    fn corrects_format_to_sprintf() {
        test::<FormatString>()
            .with_options(&FormatStringOptions {
                enforced_style: FmtMethod::Sprintf,
            })
            .expect_correction(
                indoc! {r#"
                    format('%s', name)
                    ^^^^^^ Favor `sprintf` over `format`.
                "#},
                "sprintf('%s', name)\n",
            );
    }

    // --- Kernel-qualified receivers ---

    #[test]
    fn flags_kernel_sprintf() {
        test::<FormatString>().expect_offense(indoc! {r#"
            Kernel.sprintf('%s', name)
                   ^^^^^^^ Favor `format` over `sprintf`.
        "#});
    }

    // --- Non-flagged percent patterns ---

    #[test]
    fn no_offense_non_string_receiver_non_array_rhs() {
        // Arbitrary receiver with non-array rhs: skip (heuristic limit).
        test::<FormatString>().expect_no_offenses("x % name\n");
    }
}

murphy_plugin_api::submit_cop!(FormatString);
