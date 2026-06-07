//! `Lint/UselessElseWithoutRescue` — checks for `else` in `begin..end`
//! without a corresponding `rescue` clause.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessElseWithoutRescue
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   RuboCop uses the parser's `:useless_else` diagnostic emitted during
//!   parsing. Murphy does not expose parser diagnostics, so this cop
//!   walks the AST looking for `Rescue` nodes that have an `else_` clause
//!   but no `resbodies`. In Ruby 2.6+, `else` without `rescue` is a
//!   syntax error, so Prism may not produce a `Rescue` node for the
//!   offending construct. Coverage is therefore limited to cases where
//!   Prism does emit such a node (e.g., parses under a Ruby 2.5 target).
//! ```
//!
//! ## Matched shapes
//!
//! - `begin; ...; else; ...; end` without any `rescue` clause.
//!
//! ## Autocorrect
//!
//! None — the syntax is invalid in Ruby 2.6+; the user must remove
//! the `else` clause manually.

use murphy_plugin_api::{Cx, NoOptions, NodeKind, cop};

#[derive(Default)]
pub struct UselessElseWithoutRescue;

#[cop(
    name = "Lint/UselessElseWithoutRescue",
    description = "Checks for useless `else` in `begin..end` without `rescue`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessElseWithoutRescue {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        for id in cx.descendants(cx.root()) {
            if let NodeKind::Rescue { resbodies, else_, .. } = *cx.kind(id) {
                if cx.list(resbodies).is_empty() && else_.get().is_some() {
                    cx.emit_offense(
                        cx.range(else_.get().unwrap()),
                        "`else` without `rescue` is useless.",
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UselessElseWithoutRescue;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_else_with_rescue() {
        test::<UselessElseWithoutRescue>().expect_no_offenses(indoc! {r#"
            begin
              do_something
            rescue ArgumentError
              handle_argument_error
            else
              handle_unknown_errors
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(UselessElseWithoutRescue);
