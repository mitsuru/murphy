//! `Lint/BigDecimalNew` — flags deprecated `BigDecimal.new()`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/BigDecimalNew
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   `BigDecimal.new(...)` is deprecated since BigDecimal 1.3.3. Matches both
//!   bare `BigDecimal.new` and `::BigDecimal.new` (Murphy collapses `::Const`
//!   and bare `Const` to `Const{scope:None}`, like deprecated_class_methods).
//!   Offense range is the selector (`new`) only, matching RuboCop's
//!   `add_offense(node.loc.selector)`. Autocorrect removes `.new` (dot +
//!   selector) and strips a leading `::` so `::BigDecimal.new(x)` becomes
//!   `BigDecimal(x)`.
//! ```
use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "`BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.";

#[derive(Default)]
pub struct BigDecimalNew;

#[cop(
    name = "Lint/BigDecimalNew",
    description = "`BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BigDecimalNew {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        // `(const {nil? cbase} :BigDecimal)` — Murphy collapses both bare
        // `BigDecimal` and `::BigDecimal` to `Const{scope:None}`.
        let NodeKind::Const { scope, name } = *cx.kind(receiver) else {
            return;
        };
        if scope.get().is_some() || cx.symbol_str(name) != "BigDecimal" {
            return;
        }

        // Offense range = selector (`new`) only, matching RuboCop's
        // `add_offense(node.loc.selector)`.
        cx.emit_offense(cx.selector(node), MSG, None);

        // Autocorrect: remove `.new` (dot + selector) so the call becomes
        // `BigDecimal(...)`. The dot range covers `.`; the selector covers
        // `new`. Two non-overlapping deletions.
        let dot = cx.loc(node).dot();
        if dot != Range::ZERO {
            cx.emit_edit(dot, "");
        }
        cx.emit_edit(cx.selector(node), "");

        // Strip a leading `::` so `::BigDecimal.new(x)` → `BigDecimal(x)`.
        // Murphy's AST has no separate cbase node, so detect it at source
        // level via the receiver's raw text.
        let recv_range = cx.range(receiver);
        if cx.raw_source(recv_range).starts_with("::") {
            cx.emit_edit(
                Range {
                    start: recv_range.start,
                    end: recv_range.start + 2,
                },
                "",
            );
        }
    }
}

murphy_plugin_api::submit_cop!(BigDecimalNew);

#[cfg(test)]
mod tests {
    use super::BigDecimalNew;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_big_decimal_new() {
        test::<BigDecimalNew>().expect_correction(
            indoc! {r#"
                BigDecimal.new(123.456, 3)
                           ^^^ `BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.
            "#},
            "BigDecimal(123.456, 3)\n",
        );
    }

    #[test]
    fn flags_cbase_big_decimal_new() {
        test::<BigDecimalNew>().expect_correction(
            indoc! {r#"
                ::BigDecimal.new(123.456, 3)
                             ^^^ `BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.
            "#},
            "BigDecimal(123.456, 3)\n",
        );
    }

    #[test]
    fn flags_big_decimal_new_no_args() {
        test::<BigDecimalNew>().expect_correction(
            indoc! {r#"
                BigDecimal.new
                           ^^^ `BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.
            "#},
            "BigDecimal\n",
        );
    }

    #[test]
    fn ignores_namespaced_big_decimal() {
        // `Foo::BigDecimal.new` has a non-nil scope, so it is not flagged.
        test::<BigDecimalNew>().expect_no_offenses("Foo::BigDecimal.new(123.456, 3)\n");
    }

    #[test]
    fn ignores_big_decimal_call() {
        test::<BigDecimalNew>().expect_no_offenses("BigDecimal(123.456, 3)\n");
    }

    #[test]
    fn ignores_other_new() {
        test::<BigDecimalNew>().expect_no_offenses("Foo.new(123.456, 3)\n");
    }
}
