//! `Rails/ArelStar` — flag `arel_table["*"]` in favour of `arel_table[Arel.star]`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ArelStar
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:[]] gating, the
//!   `(send {const (send _ :arel_table)} :[] $(str "*"))` shape, str-only
//!   offense range, and `Arel.star` replacement. Send-only (no csend).
//! ```
//!
//! ## Matched shapes (Send node, method `[]`)
//!
//! Outer `Send(receiver=R, method="[]", args=[str "*"])` where `R` is:
//!
//! - a `Const` (e.g. `MyModel["*"]` — ArelExtensions form), or
//! - a zero-arg `Send` with method `arel_table` (e.g. `arel_table["*"]`,
//!   `MyModel.arel_table["*"]`, `select(arel_table["*"])`).
//!
//! `hsh['*']` (lvar receiver) does not match — mirrors upstream's
//! `{const (send _ :arel_table)}` receiver union.
//!
//! ## Offense range and autocorrect
//!
//! Offense on the `str "*"` argument node only (upstream `add_offense(star)`);
//! autocorrect replaces that range with `Arel.star`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ArelStar;

#[cop(
    name = "Rails/ArelStar",
    description = "Use `Arel.star` instead of `\"*\"` for expanded column lists.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ArelStar {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[[]]`.
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive destructure — dispatcher guarantees Send.
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv) = receiver.get() else {
            return;
        };
        // Exactly one argument which must be str "*".
        let args = cx.call_arguments(node);
        if args.len() != 1 {
            return;
        }
        let star = args[0];
        if !is_star_string(cx, star) {
            return;
        }
        if !is_arel_receiver(cx, recv) {
            return;
        }
        cx.emit_offense(
            cx.range(star),
            "Use `Arel.star` instead of `\"*\"` for expanded column lists.",
            None,
        );
        cx.emit_edit(cx.range(star), "Arel.star");
    }
}

fn is_star_string(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Str(sid) => cx.string_str(sid) == "*",
        _ => false,
    }
}

/// Upstream receiver union `{const (send _ :arel_table)}`:
/// any Const, or a Send with method `arel_table`.
fn is_arel_receiver(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Const { .. } => true,
        NodeKind::Send { method, args, .. } => {
            if cx.symbol_str(method) != "arel_table" {
                return false;
            }
            // Upstream pattern `(send _ :arel_table)` has no trailing `...`,
            // i.e. zero args. Be strict here to avoid flagging
            // `arel_table(x)["*"]` custom calls.
            cx.list(args).is_empty()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ArelStar;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_arel_table_star() {
        test::<ArelStar>().expect_offense(indoc! {r#"
            arel_table["*"]
                       ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
        "#});
    }

    #[test]
    fn flags_model_arel_table_star() {
        test::<ArelStar>().expect_offense(indoc! {r#"
            MyModel.arel_table["*"]
                               ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
        "#});
    }

    #[test]
    fn flags_const_bracket_star() {
        // ArelExtensions form: receiver is a bare Const.
        test::<ArelStar>().expect_offense(indoc! {r#"
            MyModel["*"]
                    ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
        "#});
    }

    #[test]
    fn flags_arel_table_star_inside_call() {
        test::<ArelStar>().expect_offense(indoc! {r#"
            select(arel_table["*"])
                              ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
        "#});
    }

    #[test]
    fn does_not_flag_hash_star() {
        test::<ArelStar>().expect_no_offenses("hsh['*']\n");
    }

    #[test]
    fn does_not_flag_non_star_string() {
        test::<ArelStar>().expect_no_offenses("MyModel.arel_table[\"id\"]\n");
    }

    #[test]
    fn corrects_bare_arel_table_star() {
        test::<ArelStar>()
            .expect_correction(
                indoc! {r#"
                    arel_table["*"]
                               ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
                "#},
                "arel_table[Arel.star]\n",
            )
            .expect_no_offenses("arel_table[Arel.star]\n");
    }

    #[test]
    fn corrects_model_arel_table_star() {
        test::<ArelStar>()
            .expect_correction(
                indoc! {r#"
                    MyModel.arel_table["*"]
                                       ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
                "#},
                "MyModel.arel_table[Arel.star]\n",
            )
            .expect_no_offenses("MyModel.arel_table[Arel.star]\n");
    }

    #[test]
    fn corrects_const_bracket_star() {
        test::<ArelStar>()
            .expect_correction(
                indoc! {r#"
                    MyModel["*"]
                            ^^^ Use `Arel.star` instead of `"*"` for expanded column lists.
                "#},
                "MyModel[Arel.star]\n",
            )
            .expect_no_offenses("MyModel[Arel.star]\n");
    }
}
murphy_plugin_api::submit_cop!(ArelStar);
