//! `Style/ReverseFind` — flag `array.reverse.find` / `array.reverse_each.detect`
//! chains and recommend the single-pass `array.rfind` equivalent.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ReverseFind
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Verbatim port of the call head `(call _ {:find :detect} ...)`
//!   (murphy-s1yc.22): `call` = `{send csend}` covers safe-navigation
//!   (`array.reverse&.find`, `array&.reverse.find`), mirroring RuboCop
//!   `alias on_csend on_send` plus `RESTRICT_ON_SEND [find detect]`; the
//!   wildcard receiver binds an absent or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list. The receiver plus inner
//!   `reverse` shape and `&:sym`-only arg guards below apply separately
//!   (upstream outer is `(call (call _ {:reverse :reverse_each})
//!   {:find :detect} (block_pass sym)?)` in `reverse_find?`).
//!
//!   Detection mirrors RuboCop's `reverse_find?` matcher:
//!     (call (call _ {:reverse :reverse_each}) {:find :detect} (block_pass sym)?)
//!   Dispatch is on the outer `find`/`detect` send (and csend, matching
//!   RuboCop's `alias on_csend on_send`). The receiver must be a *bare*
//!   call (send/csend) — NOT a block-wrapped call — whose method is
//!   `reverse` or `reverse_each`. The receiver of `reverse` itself is
//!   unconstrained (`_`), so bare `reverse.find { }` (implicit self) also
//!   fires, verified against standalone RuboCop 1.87.0.
//!
//!   Argument gate: the `find`/`detect` call must take either no positional
//!   arguments, or exactly one `&:sym` block-pass argument. A literal block
//!   (`{ }` / `do..end`) is the find node's wrapping `Block`, not an argument,
//!   so block-form callers still fire. Positional args (`find(ifnone)`) and
//!   non-symbol block-pass (`find(&block)`) do not match.
//!
//!   Offense range = receiver's selector start .. find/detect selector end,
//!   i.e. exactly the `reverse.find` / `reverse_each.detect` span (RuboCop's
//!   `node.children.first.loc.selector.join(node.loc.selector)`).
//!   Autocorrect replaces that span with the literal `rfind` (a single
//!   whole-range replace, so the two-edit surgical rule does not apply); the
//!   `detect` variant also collapses to `rfind`. Idempotent: `rfind` never
//!   re-fires.
//!
//!   Gated at `minimum_target_ruby_version = "4.0"`, matching RuboCop's
//!   `minimum_target_ruby_version 4.0` (Ruby's `Enumerable#rfind` ships in
//!   the 4.0 line). The host registry only dispatches the cop when the
//!   resolved target is >= 4.0, so it never fires under the default 3.1
//!   floor. The lib-test harness does not enforce the gate, so the tests
//!   below exercise detection directly.
//!
//!   RuboCop marks this cop unsafe (`Safe: false`): it cannot prove the
//!   receiver responds to `rfind`. Murphy ships it `default_enabled = false`
//!   to match upstream `Enabled: pending`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.reverse.find { |item| item.even? }
//! array.reverse.detect { |item| item.even? }
//! array.reverse_each.find { |item| item.even? }
//! array.reverse_each.detect { |item| item.even? }
//! array.reverse.find(&:even?)
//! reverse.find { |item| item.even? }        # implicit-self receiver
//! array&.reverse&.find { |item| item.even? } # safe navigation
//!
//! # good
//! array.rfind { |item| item.even? }
//!
//! # not flagged
//! array.reverse.find(ifnone) { |item| item.even? } # positional arg
//! array.reverse.find(&block)                       # non-symbol block-pass
//! array.reverse_each { |x| x }.find { |y| y }      # block-wrapped receiver
//! array.find { |item| item.even? }                 # no reverse
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.22):
// `(call _ {:find :detect} ...)` — `call` = `{send csend}` covers
// safe-navigation (`array.reverse&.find`), mirroring RuboCop
// `alias on_csend on_send` plus `RESTRICT_ON_SEND [find detect]`. The `_`
// receiver binds an absent or present receiver per murphy-if9y; trailing
// `...` absorbs any argument list, so the receiver plus inner-`reverse`
// shape and `&:sym`-only arg guards below apply separately (upstream outer
// is `(call (call _ {:reverse :reverse_each}) {:find :detect}
// (block_pass sym)?)`).
def_node_matcher!(
    reverse_find_call,
    "(call _ {:find :detect} ...)"
);

/// Stateless unit struct.
#[derive(Default)]
pub struct ReverseFind;

const MSG: &str = "Use `rfind` instead.";

#[cop(
    name = "Style/ReverseFind",
    description = "Use `array.rfind` instead of `array.reverse.find`.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "4.0",
    options = NoOptions,
)]
impl ReverseFind {
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
    // Verbatim `(call _ {:find :detect} ...)` head: filters to `find` /
    // `detect` calls on either send or csend (safe-navigation), with any
    // receiver (absent or present). Without this, an unrelated call on a
    // reverse receiver (e.g. `array.reverse.map`) would run check on every
    // call node instead of being rejected by the method set up front.
    if !reverse_find_call(node, cx) {
        return;
    }
    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `find` is accepted here.
    // The find/detect call must take either no positional argument or a
    // single `&:sym` block-pass. (A literal block is the wrapping `Block`
    // node, not an argument, so block-form callers reach here with no args.)
    if !args_are_acceptable(node, cx) {
        return;
    }

    // Receiver must be a *bare* send/csend (not a block-wrapped call) whose
    // method is `reverse` / `reverse_each`. Gate the node kind before
    // trusting `cx.method_name`, which would otherwise delegate through a
    // `Block` node and flag `array.reverse_each { }.find { }`.
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    if !matches!(cx.kind(receiver), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if !matches!(cx.method_name(receiver), Some("reverse" | "reverse_each")) {
        return;
    }
    // RuboCop's `(call _ {:reverse :reverse_each})` is strict-arity: it only
    // matches `reverse` / `reverse_each` called with no arguments. So
    // `array.reverse(x).find { }` is NOT flagged.
    if !cx.call_arguments(receiver).is_empty() {
        return;
    }

    // Offense range spans the receiver's selector through this call's
    // selector — exactly `reverse.find` / `reverse_each.detect`.
    let recv_selector = cx.selector(receiver);
    let node_selector = cx.selector(node);
    if recv_selector == Range::ZERO || node_selector == Range::ZERO {
        return;
    }
    let range = Range {
        start: recv_selector.start,
        end: node_selector.end,
    };

    cx.emit_offense(range, MSG, None);
    cx.emit_edit(range, "rfind");
}

/// True if the call takes no positional argument, or exactly one `&:sym`
/// block-pass argument (RuboCop's `(block_pass sym)?`).
fn args_are_acceptable(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    match args {
        [] => true,
        [only] => is_symbol_block_pass(*only, cx),
        _ => false,
    }
}

/// True if `node` is a block-pass wrapping a symbol literal (`&:even?`).
fn is_symbol_block_pass(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::BlockPass(inner) = *cx.kind(node) else {
        return false;
    };
    inner
        .get()
        .is_some_and(|inner| matches!(cx.kind(inner), NodeKind::Sym(_)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::ReverseFind;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- offenses ----

    #[test]
    fn flags_reverse_find_block() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array.reverse.find { |item| item.even? }
                  ^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_reverse_detect_block() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array.reverse.detect { |item| item.even? }
                  ^^^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_reverse_each_find_block() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array.reverse_each.find { |item| item.even? }
                  ^^^^^^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_reverse_each_detect_block() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array.reverse_each.detect { |item| item.even? }
                  ^^^^^^^^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_reverse_find_symbol_block_pass() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array.reverse.find(&:even?)
                  ^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_bare_reverse_find_implicit_self() {
        test::<ReverseFind>().expect_offense(indoc! {"
            reverse.find { |item| item.even? }
            ^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    #[test]
    fn flags_safe_navigation() {
        test::<ReverseFind>().expect_offense(indoc! {"
            array&.reverse&.find { |item| item.even? }
                   ^^^^^^^^^^^^^ Use `rfind` instead.
        "});
    }

    // ---- autocorrect ----

    #[test]
    fn corrects_reverse_find_block() {
        test::<ReverseFind>().expect_correction(
            indoc! {"
                array.reverse.find { |item| item.even? }
                      ^^^^^^^^^^^^ Use `rfind` instead.
            "},
            "array.rfind { |item| item.even? }\n",
        );
    }

    #[test]
    fn corrects_reverse_each_detect_symbol_block_pass() {
        test::<ReverseFind>().expect_correction(
            indoc! {"
                array.reverse_each.detect(&:even?)
                      ^^^^^^^^^^^^^^^^^^^ Use `rfind` instead.
            "},
            "array.rfind(&:even?)\n",
        );
    }

    // ---- non-offenses ----

    #[test]
    fn accepts_rfind() {
        test::<ReverseFind>().expect_no_offenses("array.rfind { |item| item.even? }\n");
    }

    #[test]
    fn accepts_plain_find() {
        test::<ReverseFind>().expect_no_offenses("array.find { |item| item.even? }\n");
    }

    #[test]
    fn accepts_reverse_map() {
        test::<ReverseFind>().expect_no_offenses("array.reverse.map { |item| item.even? }\n");
    }

    #[test]
    fn accepts_positional_arg() {
        // `find(ifnone) { }` — positional argument, not a block-pass symbol.
        test::<ReverseFind>()
            .expect_no_offenses("array.reverse.find(ifnone) { |item| item.even? }\n");
    }

    #[test]
    fn accepts_non_symbol_block_pass() {
        // `find(&block)` — block-pass of a local, not a symbol literal.
        test::<ReverseFind>().expect_no_offenses("array.reverse.find(&block)\n");
    }

    #[test]
    fn accepts_reverse_with_receiver_arg() {
        // `array.reverse(x).find { }` — RuboCop's matcher requires `reverse`
        // to be called with no arguments.
        test::<ReverseFind>()
            .expect_no_offenses("array.reverse(x).find { |item| item.even? }\n");
    }

    #[test]
    fn accepts_block_wrapped_receiver() {
        // `array.reverse_each { }.find { }` — the receiver of `find` is a
        // Block node, not a bare `reverse_each` call.
        test::<ReverseFind>()
            .expect_no_offenses("array.reverse_each { |x| x }.find { |y| y }\n");
    }

    // --- Characterization (murphy-s1yc.22): pin the exact node set the
    // dual send (methods = ["find", "detect"]) + manual-csend-filter dispatch
    // matches, so the verbatim `(call _ {:find :detect} ...)` port can be
    // proven byte-identical. `call` covers safe-navigation (mirroring upstream
    // `alias on_csend on_send` plus `RESTRICT_ON_SEND [find detect]`);
    // trailing `...` absorbs any argument list, so the receiver plus inner
    // `reverse` shape and `&:sym`-only arg guards below apply separately.

    #[test]
    fn s1yc22_flags_csend_block_corrects() {
        // Safe navigation on both levels: `call` covers `csend` per
        // murphy-if9y, mirroring upstream `alias on_csend on_send`.
        // (Pre-port the `csend` handler filters `find`/`detect` manually
        // because `methods = [...]` is only valid for `kind = "send"`; the
        // verbatim head collapses the workaround.)
        test::<ReverseFind>().expect_correction(
            indoc! {r#"
                array&.reverse&.find { |item| item.even? }
                       ^^^^^^^^^^^^^ Use `rfind` instead.
            "#},
            "array&.rfind { |item| item.even? }\n",
        );
    }

    #[test]
    fn s1yc22_flags_outer_csend_corrects() {
        // Outer `find` is csend, inner `reverse` is plain send.
        test::<ReverseFind>().expect_correction(
            indoc! {r#"
                array.reverse&.find { |item| item.even? }
                      ^^^^^^^^^^^^^ Use `rfind` instead.
            "#},
            "array.rfind { |item| item.even? }\n",
        );
    }

    #[test]
    fn s1yc22_flags_bare_reverse_find() {
        // Bare inner receiver: `_` in `(call _ {:reverse :reverse_each})`
        // binds an absent receiver per murphy-if9y, so `reverse.find`
        // (implicit self) flags. The verbatim outer head matches and the
        // complementary inner-shape guard flags.
        test::<ReverseFind>().expect_offense(indoc! {r#"
            reverse.find { |item| item.even? }
            ^^^^^^^^^^^^ Use `rfind` instead.
        "#});
    }

    #[test]
    fn s1yc22_accepts_positional_arg() {
        // Outer `find(ifnone)`: trailing `...` absorbs the argument list so
        // the head matches, and the complementary `&:sym`-only arg guard
        // accepts (upstream `(block_pass sym)?`).
        test::<ReverseFind>()
            .expect_no_offenses("array.reverse.find(ifnone) { |item| item.even? }\n");
    }

    #[test]
    fn s1yc22_accepts_unrelated_method() {
        // `map` is outside the verbatim method set, so the head rejects.
        test::<ReverseFind>()
            .expect_no_offenses("array.reverse.map { |item| item.even? }\n");
    }

    // ---- registration / gate ----

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <ReverseFind as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(4, 0)),
        );
    }
}

murphy_plugin_api::submit_cop!(ReverseFind);
