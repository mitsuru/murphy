//! `Lint/UselessDefaultValueArgument` — Checks for `fetch` and `Array.new`
//! calls that supply both a default value argument and a block (the block
//! always wins, making the default value useless).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessDefaultValueArgument
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: fetch and Array.new detection,
//!   safe-navigation support, keyword-arg exclusion, autocorrect removal
//!   of the redundant default value argument.
//! ```
//!
//! ## Matched shapes
//!
//! - `x.fetch(key, default_value) { block }`
//! - `x&.fetch(key, default_value) { block }`
//! - `Array.new(size, default_value) { block }`
//! - `::Array.new(size, default_value) { block }`
//!
//! ## Autocorrect
//!
//! Remove the default value argument and the comma before it.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct UselessDefaultValueArgument;

#[cop(
    name = "Lint/UselessDefaultValueArgument",
    description = "Checks for `fetch` or `Array.new` with both a default value argument and a block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessDefaultValueArgument {
    #[on_node(kind = "send", methods = ["fetch", "new"])]
    fn on_send(&self, node: NodeId, cx: &Cx<'_>) {
        self.check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn on_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else { return };
        let method_str = cx.symbol_str(method);
        if method_str == "fetch" || method_str == "new" {
            self.check(node, cx);
        }
    }

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let (method, receiver_opt, args_list) = match *cx.kind(node) {
            NodeKind::Send { method, receiver, args } => {
                (method, receiver, cx.list(args))
            }
            NodeKind::Csend { method, receiver, args } => {
                (method, OptNodeId::some(receiver), cx.list(args))
            }
            _ => return,
        };

        let method_name = cx.symbol_str(method);

        // Must be inside a Block where this node is the call.
        let Some(parent) = cx.parent(node).get() else { return };
        let NodeKind::Block { call, .. } = *cx.kind(parent) else { return };
        if call != node {
            return;
        }

        // Must have at least 2 args (key + default_value / size + default_value).
        if args_list.len() < 2 {
            return;
        }

        match method_name {
            "fetch" => {
                self.check_fetch(receiver_opt, args_list, cx);
            }
            "new" => {
                self.check_array_new(receiver_opt, args_list, cx);
            }
            _ => {}
        }
    }

    fn check_fetch(&self, receiver_opt: OptNodeId, args_list: &[NodeId], cx: &Cx<'_>) {
        // Must have a receiver (not bare `fetch(...)`).
        if receiver_opt.get().is_none() {
            return;
        }

        // Must NOT have 3+ positional args.
        if args_list.len() >= 3 {
            return;
        }

        // Must NOT have **kwarg splat in args.
        for &arg_id in args_list.iter() {
            if matches!(*cx.kind(arg_id), NodeKind::Kwsplat(_)) {
                return;
            }
        }

        let default_value = args_list[1];

        // Must NOT be a keyword-style hash without braces (e.g. `default: value`).
        // This distinguishes `x.fetch(key, {})` (braced empty hash = default value)
        // from `x.fetch(key, default: value)` (keyword-arg hash = not a default).
        // Heuristic: check if source starts with `{`. In practice Ruby keyword args
        // without braces never start with `{`, so this is safe despite not parsing
        // the AST structure. A more robust alternative would inspect the hash's pair
        // nodes directly via NodeKind::Pair.
        if let NodeKind::Hash(_) = *cx.kind(default_value) {
            let src = cx.raw_source(cx.range(default_value));
            if !src.starts_with('{') {
                return;
            }
        }

        self.emit_offense_and_correct(args_list[0], default_value, cx);
    }

    fn check_array_new(&self, receiver_opt: OptNodeId, args_list: &[NodeId], cx: &Cx<'_>) {
        // Receiver must be a const reference to `Array`.
        let Some(recv_id) = receiver_opt.get() else {
            return;
        };
        let NodeKind::Const { scope, name } = *cx.kind(recv_id) else {
            return;
        };
        if cx.symbol_str(name) != "Array" {
            return;
        }
        let scope_ok = scope
            .get()
            .is_none_or(|s| matches!(*cx.kind(s), NodeKind::Cbase));
        if !scope_ok {
            return;
        }

        // Must have exactly 2 args (size + default_value).
        if args_list.len() != 2 {
            return;
        }

        self.emit_offense_and_correct(args_list[0], args_list[1], cx);
    }

    fn emit_offense_and_correct(&self, prev_arg: NodeId, default_value: NodeId, cx: &Cx<'_>) {
        cx.emit_offense(
            cx.range(default_value),
            "Block supersedes default value argument.",
            None,
        );

        let prev_range = cx.range(prev_arg);
        let dv_range = cx.range(default_value);
        cx.emit_edit(Range { start: prev_range.end, end: dv_range.end }, "");
    }
}

#[cfg(test)]
mod tests {
    use super::UselessDefaultValueArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── fetch ──────────────────────────────────────────────────────────

    #[test]
    fn flags_fetch_with_default_and_block() {
        test::<UselessDefaultValueArgument>().expect_correction(
            indoc! {r#"
                x.fetch(key, default_value) { block_value }
                             ^^^^^^^^^^^^^ Block supersedes default value argument.
            "#},
            "x.fetch(key) { block_value }\n",
        );
    }

    #[test]
    fn flags_fetch_with_block_args() {
        test::<UselessDefaultValueArgument>().expect_offense(indoc! {r#"
            x.fetch(key, default_value) { |arg| arg }
                         ^^^^^^^^^^^^^ Block supersedes default value argument.
        "#});
    }

    #[test]
    fn flags_safe_nav_fetch() {
        test::<UselessDefaultValueArgument>().expect_offense(indoc! {r#"
            x&.fetch(key, default_value) { block_value }
                          ^^^^^^^^^^^^^ Block supersedes default value argument.
        "#});
    }

    #[test]
    fn flags_fetch_with_hash_default() {
        test::<UselessDefaultValueArgument>().expect_offense(indoc! {r#"
            x.fetch(key, {}) { block_value }
                         ^^ Block supersedes default value argument.
        "#});
    }

    #[test]
    fn accepts_fetch_no_block() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "x.fetch(key, default_value)\n",
        );
    }

    #[test]
    fn accepts_fetch_with_block_no_default() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "x.fetch(key) { block_value }\n",
        );
    }

    #[test]
    fn accepts_fetch_with_keyword_arg() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "x.fetch(key, default: value) { block_value }\n",
        );
    }

    #[test]
    fn accepts_fetch_with_three_args() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "x.fetch(key, default_value, third) { block_value }\n",
        );
    }

    #[test]
    fn accepts_fetch_with_splat() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "x.fetch(key, **kwarg) { block_value }\n",
        );
    }

    #[test]
    fn accepts_bare_fetch() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "fetch(key, default_value) { |arg| arg }\n",
        );
    }

    // ── Array.new ──────────────────────────────────────────────────────

    #[test]
    fn flags_array_new_with_default_and_block() {
        test::<UselessDefaultValueArgument>().expect_correction(
            indoc! {r#"
                Array.new(size, default_value) { block_value }
                                ^^^^^^^^^^^^^ Block supersedes default value argument.
            "#},
            "Array.new(size) { block_value }\n",
        );
    }

    #[test]
    fn flags_array_new_with_cbase() {
        test::<UselessDefaultValueArgument>().expect_offense(indoc! {r#"
            ::Array.new(size, default_value) { block_value }
                              ^^^^^^^^^^^^^ Block supersedes default value argument.
        "#});
    }

    #[test]
    fn accepts_array_new_no_block() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "Array.new(size, default_value)\n",
        );
    }

    #[test]
    fn accepts_array_new_no_default() {
        test::<UselessDefaultValueArgument>().expect_no_offenses(
            "Array.new(size) { block_value }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(UselessDefaultValueArgument);
