//! `RSpec/SingleArgumentMessageChain` — no single-argument `receive_message_chain` / `stub_chain`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SingleArgumentMessageChain
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` gated by `RESTRICT_ON_SEND`
//!   (`receive_message_chain` / `stub_chain`) with the
//!   `message_chain` matcher (any receiver, exactly one arg) and
//!   `valid_usage?` (non-literal non-array args stay clean; a
//!   multi-key `Hash` or multi-element `Array` stays clean; a `Str` /
//!   `Sym` containing `.` stays clean as a pre-joined chain). A lone
//!   `Sym` / `Str` without `.`, a single-key `Hash`, or a
//!   single-element `Array` flags the selector per
//!   `add_offense(node.loc.selector)` with `Use `receive` / `stub`
//!   instead of calling `...` with a single argument.`. Detection is
//!   at parity (verified vs 3.7.0, including the hash/array and
//!   dotted-string clean cases); autocorrect (rewrite to `receive` /
//!   `stub` with `and_return` / unwrapped args) is not ported in this
//!   batch — same convention as `RSpec/BeEql` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` naming `receive_message_chain` / `stub_chain`
//! with exactly one argument:
//!
//! - `allow(x).to receive_message_chain(:one)` — flagged (prefer `receive`).
//! - `allow(x).to receive_message_chain("one")` — flagged.
//! - `allow(x).to receive_message_chain(bar: 42)` — flagged.
//! - `allow(x).to receive_message_chain([:one])` — flagged.
//! - `foo.stub_chain(:one)` — flagged (prefer `stub`).
//! - `receive_message_chain(:one, :two)` — multi-arg, clean.
//! - `receive_message_chain("one.two")` — dotted, clean.
//! - `receive_message_chain(foo)` — dynamic, clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `receive` / `stub` (unwrapping single arrays,
//! splitting single-key hashes into `and_return`). This batch reports
//! only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SingleArgumentMessageChain;

#[cop(
    name = "RSpec/SingleArgumentMessageChain",
    description = "Checks that chains of messages contain more than one element.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SingleArgumentMessageChain {
    #[on_node(
        kind = "send",
        methods = ["receive_message_chain", "stub_chain"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, args, .. } = *cx.kind(node) else {
            return;
        };
        let method_name = cx.symbol_str(method).to_owned();
        let arg_ids = cx.list(args);
        if arg_ids.len() != 1 {
            return;
        }
        let arg = arg_ids[0];
        if is_valid_usage(cx, arg) {
            return;
        }
        let recommended = if method_name == "receive_message_chain" {
            "receive"
        } else {
            "stub"
        };
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!(
                "Use `{recommended}` instead of calling `{method_name}` with a single argument."
            ),
            None,
        );
    }
}

/// `true` when a lone chain arg is an acceptable single-argument use.
///
/// Mirrors upstream `valid_usage?`: non-literal non-array args (sends,
/// variable reads, interpolated strings, splats, ...) stay clean; a
/// `Hash` is clean unless it holds exactly one pair; an `Array` is
/// clean unless it holds exactly one element; any other literal is
/// clean only when its source spells a dotted chain (`"one.two"`).
fn is_valid_usage(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        NodeKind::Array(elems) => cx.list(elems).len() != 1,
        NodeKind::Hash(pairs) => cx.list(pairs).len() != 1,
        NodeKind::Str(str_id) => cx.string_str(str_id).contains('.'),
        NodeKind::Sym(sym) => cx.symbol_str(sym).contains('.'),
        // Interpolated strings / symbols mirror `node.to_s.include?('.')`:
        // a dotted spelling stays clean, anything else flags.
        NodeKind::Dstr(_) | NodeKind::Dsym(_) | NodeKind::Xstr(_) => {
            cx.raw_source(cx.range(arg)).contains('.')
        }
        // Splat / block-pass / forwarded args are dynamic.
        NodeKind::Splat(_) | NodeKind::BlockPass(_) | NodeKind::ForwardedArgs => true,
        // Variable reads, calls, constants, and anything else dynamic.
        NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Send { .. }
        | NodeKind::Csend { .. }
        | NodeKind::Const { .. }
        | NodeKind::Index { .. }
        | NodeKind::Yield(_)
        | NodeKind::Defined(_)
        | NodeKind::Unknown => true,
        // Remaining static literals (numbers, booleans, nil, regexps,
        // ranges, ...) mirror `node.to_s.include?('.')`: only a dotted
        // spelling (e.g. `1.5`) stays clean.
        _ => cx.raw_source(cx.range(arg)).contains('.'),
    }
}

#[cfg(test)]
mod tests {
    use super::SingleArgumentMessageChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_sym_chain() {
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain(:one)
                              ^^^^^^^^^^^^^^^^^^^^^ Use `receive` instead of calling `receive_message_chain` with a single argument.
            "#});
    }

    #[test]
    fn flags_single_string_chain() {
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain("one")
                              ^^^^^^^^^^^^^^^^^^^^^ Use `receive` instead of calling `receive_message_chain` with a single argument.
            "#});
    }

    #[test]
    fn flags_single_key_hash() {
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain(bar: 42)
                              ^^^^^^^^^^^^^^^^^^^^^ Use `receive` instead of calling `receive_message_chain` with a single argument.
            "#});
    }

    #[test]
    fn flags_single_element_array() {
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain([:one])
                              ^^^^^^^^^^^^^^^^^^^^^ Use `receive` instead of calling `receive_message_chain` with a single argument.
            "#});
    }

    #[test]
    fn flags_stub_chain_single_arg() {
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                foo.stub_chain(:one)
                    ^^^^^^^^^^ Use `stub` instead of calling `stub_chain` with a single argument.
            "#});
    }

    #[test]
    fn ignores_multi_arg_chain() {
        test::<SingleArgumentMessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_message_chain(:one, :two)
            "#});
    }

    #[test]
    fn ignores_dotted_string() {
        test::<SingleArgumentMessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_message_chain("one.two")
            "#});
    }

    #[test]
    fn flags_interpolated_string_without_dot() {
        // Upstream treats `dstr` as a literal for `valid_usage?`
        // (verified vs 3.7.0): only a dotted spelling stays clean.
        test::<SingleArgumentMessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain("foo#{bar}")
                              ^^^^^^^^^^^^^^^^^^^^^ Use `receive` instead of calling `receive_message_chain` with a single argument.
            "#});
    }

    #[test]
    fn ignores_dynamic_arg() {
        test::<SingleArgumentMessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_message_chain(foo)
            "#});
    }

    #[test]
    fn ignores_multi_key_hash() {
        test::<SingleArgumentMessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_message_chain(bar: 42, baz: 42)
            "#});
    }

    #[test]
    fn ignores_multi_element_array() {
        test::<SingleArgumentMessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_message_chain([:one, :two])
            "#});
    }
}

murphy_plugin_api::submit_cop!(SingleArgumentMessageChain);
