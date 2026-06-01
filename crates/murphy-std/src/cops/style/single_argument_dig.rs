//! `Style/SingleArgumentDig` — flags single-argument `dig` calls and suggests
//! replacing them with bracket notation `[]`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SingleArgumentDig
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 does not implement the DigChain option — RuboCop's
//!   `DigChainEnabled` config (disabled by default) suppresses offenses in dig
//!   chains.  Murphy always flags single-argument dig calls regardless of chaining.
//!   `csend` (`&.dig`) calls are not flagged, matching RuboCop's safety note that
//!   replacing `hash&.dig(:key)` with `hash[:key]` can introduce errors when the
//!   receiver is nil.
//!   The `forwarded_restarg` and `forwarded_args` argument types from RuboCop's
//!   exclusion list do not appear in the Murphy AST (those constructs require
//!   a method that forwards its own parameters) and are therefore omitted.
//!   The cop is marked `Safe: false` in default.yml (unsafe because it cannot
//!   guarantee the receiver implements dig in the standard way).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! { key: 'value' }.dig(:key)
//! [1, 2, 3].dig(0)
//!
//! # good
//! { key: 'value' }[:key]
//! [1, 2, 3][0]
//! { key1: { key2: 'value' } }.dig(:key1, :key2)
//! keys = %i[key1 key2]
//! { key1: { key2: 'value' } }.dig(*keys)
//! hash&.dig(:key)
//! ```
//!
//! ## Autocorrect
//!
//! Whole-node replacement: `receiver.dig(arg)` -> `receiver[arg]`.
//! This is a structural rewrite (receiver moves to a new syntactic position),
//! so per `.claude/rules/autocorrect-pattern.md` whole-node interpolation is used.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SingleArgumentDig;

const MSG: &str = "Use `%<receiver>s[%<argument>s]` instead of `%<original>s`.";

#[cop(
    name = "Style/SingleArgumentDig",
    description = "Avoid using single argument dig method.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SingleArgumentDig {
    /// Only plain `send` nodes -- `csend` (`&.dig`) is deliberately excluded.
    #[on_node(kind = "send", methods = ["dig"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return;
    };

    // Must have a receiver (skip bare `dig(:key)` calls).
    let Some(recv_id) = receiver.get() else {
        return;
    };

    let args_list = cx.list(args);

    // Must have exactly one argument.
    if args_list.len() != 1 {
        return;
    }

    let arg = args_list[0];

    // Skip ignored argument types: block_pass, splat, hash.
    // (forwarded_restarg and forwarded_args do not exist in the Murphy AST.)
    match cx.kind(arg) {
        NodeKind::BlockPass(_) | NodeKind::Splat(_) | NodeKind::Hash(_) => return,
        _ => {}
    }

    let receiver_src = cx.raw_source(cx.range(recv_id));
    let argument_src = cx.raw_source(cx.range(arg));
    let original_src = cx.raw_source(cx.range(node));

    let message = MSG
        .replace("%<receiver>s", receiver_src)
        .replace("%<argument>s", argument_src)
        .replace("%<original>s", original_src);

    cx.emit_offense(cx.range(node), &message, None);

    // Autocorrect: whole-node replacement.
    let corrected = format!("{receiver_src}[{argument_src}]");
    cx.emit_edit(cx.range(node), &corrected);
}

#[cfg(test)]
mod tests {
    use super::SingleArgumentDig;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Hash -----

    #[test]
    fn flags_hash_dig_single_symbol() {
        test::<SingleArgumentDig>().expect_correction(
            indoc! {r#"
                { key: 'value' }.dig(:key)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `{ key: 'value' }[:key]` instead of `{ key: 'value' }.dig(:key)`.
            "#},
            "{ key: 'value' }[:key]\n",
        );
    }

    // ----- Array -----

    #[test]
    fn flags_array_dig_single_int() {
        test::<SingleArgumentDig>().expect_correction(
            indoc! {r#"
                [1, 2, 3].dig(0)
                ^^^^^^^^^^^^^^^^ Use `[1, 2, 3][0]` instead of `[1, 2, 3].dig(0)`.
            "#},
            "[1, 2, 3][0]\n",
        );
    }

    // ----- Variable receiver -----

    #[test]
    fn flags_variable_receiver_dig() {
        test::<SingleArgumentDig>().expect_correction(
            indoc! {r#"
                hash.dig(:key)
                ^^^^^^^^^^^^^^ Use `hash[:key]` instead of `hash.dig(:key)`.
            "#},
            "hash[:key]\n",
        );
    }

    // ----- No offense: multi-arg -----

    #[test]
    fn accepts_multi_arg_dig() {
        test::<SingleArgumentDig>()
            .expect_no_offenses("{ key1: { key2: 'value' } }.dig(:key1, :key2)\n");
    }

    // ----- No offense: splat -----

    #[test]
    fn accepts_splat_arg() {
        test::<SingleArgumentDig>().expect_no_offenses("hash.dig(*keys)\n");
    }

    // ----- No offense: block pass -----

    #[test]
    fn accepts_block_pass_arg() {
        test::<SingleArgumentDig>().expect_no_offenses("foo.dig(&method(:key))\n");
    }

    // ----- No offense: csend -----

    #[test]
    fn accepts_csend_dig() {
        test::<SingleArgumentDig>().expect_no_offenses("hash&.dig(:key)\n");
    }

    // ----- No offense: no receiver -----

    #[test]
    fn accepts_bare_dig() {
        test::<SingleArgumentDig>().expect_no_offenses("dig(:key)\n");
    }

    // ----- No offense: hash keyword arg -----

    #[test]
    fn accepts_hash_arg() {
        test::<SingleArgumentDig>().expect_no_offenses("hash.dig(**kw)\n");
    }
}
murphy_plugin_api::submit_cop!(SingleArgumentDig);
