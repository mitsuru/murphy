//! `RSpec/VerifiedDoubles` — prefer verifying doubles over plain doubles.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/VerifiedDoubles
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `unverified_double` (`(send nil? {:double :spy} $...)`)
//!   with `RESTRICT_ON_SEND = [:double, :spy]`. Only bare-receiver calls flag;
//!   explicit receivers (`obj.double`) never flag. `IgnoreNameless` (default
//!   true) skips zero-arg `double` / `spy`; `IgnoreSymbolicNames` (default
//!   false) skips when the first arg is a `Sym`. The offense range is the whole
//!   node per `add_offense(node)`. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["double", "spy"]` (bare receiver
//! only):
//!
//! - `double("ClassName", method_name: 'value')` — flagged.
//! - `double(:sym)` — flagged by default (`IgnoreSymbolicNames: false`).
//! - `spy("ClassName")` — flagged.
//! - `double` — nameless, not flagged by default (`IgnoreNameless: true`).
//! - `obj.double("x")` — explicit receiver, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; choosing the verifying double type needs
//! human judgement.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct VerifiedDoubles;

#[derive(CopOptions)]
pub struct VerifiedDoublesOptions {
    #[option(
        name = "IgnoreNameless",
        default = true,
        description = "Whether to ignore nameless doubles (no arguments)."
    )]
    pub ignore_nameless: bool,
    #[option(
        name = "IgnoreSymbolicNames",
        default = false,
        description = "Whether to ignore doubles with symbolic names."
    )]
    pub ignore_symbolic_names: bool,
}

#[cop(
    name = "RSpec/VerifiedDoubles",
    description = "Prefer using verifying doubles over normal doubles.",
    default_severity = "warning",
    default_enabled = true,
    options = VerifiedDoublesOptions,
)]
impl VerifiedDoubles {
    #[on_node(kind = "send", methods = ["double", "spy"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let opts = cx.options_or_default::<VerifiedDoublesOptions>();
        let arg_ids = cx.list(args);
        if arg_ids.is_empty() {
            if opts.ignore_nameless {
                return;
            }
        } else if opts.ignore_symbolic_names
            && matches!(*cx.kind(arg_ids[0]), NodeKind::Sym(_))
        {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Prefer using verifying doubles over normal doubles.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{VerifiedDoubles, VerifiedDoublesOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn no_ignore_nameless() -> VerifiedDoublesOptions {
        VerifiedDoublesOptions {
            ignore_nameless: false,
            ignore_symbolic_names: false,
        }
    }

    fn ignore_symbolic() -> VerifiedDoublesOptions {
        VerifiedDoublesOptions {
            ignore_nameless: true,
            ignore_symbolic_names: true,
        }
    }

    #[test]
    fn flags_double_with_string_name() {
        test::<VerifiedDoubles>().expect_offense(indoc! {r#"
                double("ClassName", method_name: 'value')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using verifying doubles over normal doubles.
            "#});
    }

    #[test]
    fn flags_double_with_symbol_name_by_default() {
        test::<VerifiedDoubles>().expect_offense(indoc! {r#"
                double(:sym)
                ^^^^^^^^^^^^ Prefer using verifying doubles over normal doubles.
            "#});
    }

    #[test]
    fn flags_spy() {
        test::<VerifiedDoubles>().expect_offense(indoc! {r#"
                spy("ClassName")
                ^^^^^^^^^^^^^^^^ Prefer using verifying doubles over normal doubles.
            "#});
    }

    #[test]
    fn does_not_flag_nameless_by_default() {
        test::<VerifiedDoubles>().expect_no_offenses(indoc! {r#"
                double
            "#});
    }

    #[test]
    fn flags_nameless_when_not_ignored() {
        test::<VerifiedDoubles>()
            .with_options(&no_ignore_nameless())
            .expect_offense(indoc! {r#"
                double
                ^^^^^^ Prefer using verifying doubles over normal doubles.
            "#});
    }

    #[test]
    fn does_not_flag_symbolic_when_ignored() {
        test::<VerifiedDoubles>()
            .with_options(&ignore_symbolic())
            .expect_no_offenses(indoc! {r#"
                double(:sym)
            "#});
    }

    #[test]
    fn does_not_flag_receiver_double() {
        // Upstream requires a bare (`nil?`) receiver.
        test::<VerifiedDoubles>().expect_no_offenses(indoc! {r#"
                obj.double("x")
            "#});
    }
}

murphy_plugin_api::submit_cop!(VerifiedDoubles);
