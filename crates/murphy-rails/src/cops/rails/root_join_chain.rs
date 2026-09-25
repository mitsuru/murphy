//! `Rails/RootJoinChain` — use a single `#join` instead of chaining on `Rails.root`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RootJoinChain
//! upstream_version_checked: 2.35.0
//! version_added: "2.13"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: only the outermost `join` of a chain
//!   rooted at zero-arg `Rails.root`/`Rails.public_path` (`::Rails`
//!   included) flags; a single `join` never flags. The offense covers the
//!   outermost `join` send (so a trailing `.to_s` is excluded) and the
//!   message names the root source (`Rails.root`, `::Rails.root`,
//!   `Rails.public_path`). Autocorrect replaces the span from the end of
//!   the root selector to the end of the outermost `join` with a single
//!   `.join(args...)` built from source. Upstream ships `Enabled: pending`;
//!   Murphy maps that to `default_enabled = false`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RootJoinChain;

#[cop(
    name = "Rails/RootJoinChain",
    description = "Use a single `#join` instead of chaining on `Rails.root` or `Rails.public_path`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RootJoinChain {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[join]`.
    #[on_node(kind = "send", methods = ["join"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    // Upstream `return if join?(node.parent)`: only the end of the chain flags.
    // (The `send` pattern never matches `csend`, so a `&.` parent ends the chain.)
    if let Some(parent) = cx.parent(node).get()
        && matches!(*cx.kind(parent), NodeKind::Send { .. })
        && cx.method_name(parent) == Some("join")
    {
        return;
    }
    // Upstream `return if rails_root?(node.receiver)`: a single `join` never flags.
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if is_rails_root(cx, recv) {
        return;
    }
    // Walk down through `join` sends, prepending each level's args.
    let mut all_args: Vec<NodeId> = Vec::new();
    let mut current = node;
    loop {
        if !matches!(*cx.kind(current), NodeKind::Send { .. })
            || cx.method_name(current) != Some("join")
        {
            break;
        }
        let args = cx.call_arguments(current).to_vec();
        all_args.splice(0..0, args);
        let Some(next) = cx.call_receiver(current).get() else {
            return;
        };
        current = next;
    }
    if !is_rails_root(cx, current) {
        return;
    }
    let rails_src = cx.raw_source(cx.range(current)).to_owned();
    // The arena send range covers an attached block; upstream reports and
    // corrects the outermost `join` send alone.
    let send_end = send_only_end(cx, node);
    cx.emit_offense(
        murphy_plugin_api::Range {
            start: cx.range(node).start,
            end: send_end,
        },
        &format!("Use `{rails_src}.join(...)` instead of chaining `#join` calls."),
        None,
    );
    let selector_end = cx.loc(current).name.end;
    let replacement = format!(
        ".join({})",
        all_args
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_owned())
            .collect::<Vec<_>>()
            .join(", ")
    );
    cx.emit_edit(
        murphy_plugin_api::Range {
            start: selector_end,
            end: send_end,
        },
        &replacement,
    );
}


/// Send-only end: the arena send range covers an attached block, while
/// upstream's offense and correction span the outermost `join` send alone.
fn send_only_end(cx: &Cx<'_>, node: NodeId) -> u32 {
    if cx.block_node(node).get().is_none() {
        return cx.range(node).end;
    }
    if cx.is_parenthesized(node) {
        if let Some(paren_end) = matching_close_paren(cx, node) {
            return paren_end;
        }
        return cx.range(node).end;
    }
    if let Some(&last) = cx.call_arguments(node).last() {
        return cx.range(last).end;
    }
    cx.loc(node).name.end
}

/// Closing paren matching the call's open paren. Depth-counted so nested
/// calls resolve to the outer `)`.
fn matching_close_paren(cx: &Cx<'_>, node: NodeId) -> Option<u32> {
    let toks = cx.tokens_in(cx.range(node));
    let sel_end = cx.selector(node).end;
    let open = toks
        .iter()
        .position(|t| t.kind == SourceTokenKind::LeftParen && t.range.start >= sel_end)?;
    let mut depth = 0i32;
    for tok in &toks[open..] {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    None
}

/// Zero-arg `Rails.root` / `Rails.public_path` (`::Rails` included).
/// Upstream's node pattern implies exact arity, so calls with args never match.
fn is_rails_root(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    let method = cx.method_name(id);
    if method != Some("root") && method != Some("public_path") {
        return false;
    }
    if !cx.call_arguments(id).is_empty() {
        return false;
    }
    cx.call_receiver(id)
        .get()
        .is_some_and(|r| cx.const_name(r).as_deref() == Some("Rails"))
}

#[cfg(test)]
mod tests {
    use super::RootJoinChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_chained_join() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                Rails.root.join('db').join('schema.rb')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.root.join(...)` instead of chaining `#join` calls.
            "#},
            "Rails.root.join('db', 'schema.rb')\n",
        );
    }

    #[test]
    fn flags_triple_chain() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                Rails.root.join('db').join(migrate).join('migration.rb')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.root.join(...)` instead of chaining `#join` calls.
            "#},
            "Rails.root.join('db', migrate, 'migration.rb')\n",
        );
    }

    #[test]
    fn flags_public_path_chain() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                Rails.public_path.join('path').join('file.pdf')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path.join(...)` instead of chaining `#join` calls.
            "#},
            "Rails.public_path.join('path', 'file.pdf')\n",
        );
    }

    #[test]
    fn flags_outer_only_before_to_s() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                Rails.root.join('a').join('b').to_s
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.root.join(...)` instead of chaining `#join` calls.
            "#},
            "Rails.root.join('a', 'b').to_s\n",
        );
    }

    #[test]
    fn keeps_cbase_prefix() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                ::Rails.root.join('a').join('b')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `::Rails.root.join(...)` instead of chaining `#join` calls.
            "#},
            "::Rails.root.join('a', 'b')\n",
        );
    }

    #[test]
    fn keeps_block_with_send_only_offense() {
        test::<RootJoinChain>().expect_correction(
            indoc! {r#"
                Rails.root.join('a').join('b') { |x| puts x }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.root.join(...)` instead of chaining `#join` calls.
            "#},
            "Rails.root.join('a', 'b') { |x| puts x }\n",
        );
    }

    #[test]
    fn allows_single_join() {
        test::<RootJoinChain>().expect_no_offenses("Rails.root.join('db')\n");
    }

    #[test]
    fn allows_non_rails_chain() {
        test::<RootJoinChain>().expect_no_offenses("foo.join('a').join('b')\n");
    }

    #[test]
    fn allows_root_with_args() {
        test::<RootJoinChain>().expect_no_offenses("Rails.root(x).join('a').join('b')\n");
    }
}
murphy_plugin_api::submit_cop!(RootJoinChain);
