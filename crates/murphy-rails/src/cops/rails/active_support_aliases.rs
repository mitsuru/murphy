//! `Rails/ActiveSupportAliases` — flag ActiveSupport aliases of core Ruby methods.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActiveSupportAliases
//! upstream_version_checked: 2.35.0
//! version_added: "0.48"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (aliased to `on_csend`) with the
//!   four `def_node_matcher` shapes: `(call str :starts_with? _)` /
//!   `(call str :ends_with? _)` (string-literal receiver, exactly one arg)
//!   and `(call array :append _)` / `(call array :prepend _)` (array-literal
//!   receiver, exactly one arg). Non-literal receivers (`foo`, `x`),
//!   interpolated strings, and multi-arg calls do not flag. Offense range
//!   is selector-to-expression-end (`loc.selector.join(source_range.end)`);
//!   autocorrect renames the selector, except `append` which has no
//!   correction upstream (`next if append(node)`).
//! ```
//!
//! ## Matched shapes
//!
//! - `'s'.starts_with?('p')` → `start_with?` (same for `&.`)
//! - `'s'.ends_with?('s')` → `end_with?` (same for `&.`)
//! - `[1].prepend('b')` → `unshift` (same for `&.`)
//! - `[1].append('b')` → offense only, no correction
//!
//! ## Autocorrect
//!
//! Selector rename (`loc.selector`): `starts_with?` → `start_with?`,
//! `ends_with?` → `end_with?`, `prepend` → `unshift`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActiveSupportAliases;

#[cop(
    name = "Rails/ActiveSupportAliases",
    description = "Use `start_with?`, `end_with?`, `<<` and `unshift` instead of ActiveSupport aliases.",
    default_enabled = true,
    options = NoOptions,
)]
impl ActiveSupportAliases {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[starts_with? ends_with? append prepend]`.
    // Csend cannot use `methods` filtering (macro restriction), so it uses a
    // bare kind subscription with a manual method check.
    #[on_node(
        kind = "send",
        methods = ["starts_with?", "ends_with?", "append", "prepend"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m,
        None => return,
    };
    // Upstream matchers require a literal receiver of a fixed type and
    // exactly one argument (`(call str :starts_with? _)` etc.).
    let (want_str, prefer): (bool, &str) = match method {
        "starts_with?" => (true, "start_with?"),
        "ends_with?" => (true, "end_with?"),
        "append" => (false, "<<"),
        "prepend" => (false, "unshift"),
        _ => return,
    };
    if cx.call_arguments(node).len() != 1 {
        return;
    }
    let receiver = cx.call_receiver(node).get();
    let receiver_ok = match (want_str, receiver) {
        (true, Some(r)) => matches!(*cx.kind(r), NodeKind::Str(_)),
        (false, Some(r)) => matches!(*cx.kind(r), NodeKind::Array(_)),
        _ => false,
    };
    if !receiver_ok {
        return;
    }
    let msg = format!("Use `{prefer}` instead of `{method}`.");
    // Upstream range: `node.loc.selector.join(node.source_range.end)`.
    let sel = cx.selector(node);
    let end = cx.range(node).end;
    cx.emit_offense(
        murphy_plugin_api::Range { start: sel.start, end },
        &msg,
        None,
    );
    // Upstream: `next if append(node)` — `append` has no correction.
    if method == "append" {
        return;
    }
    cx.emit_edit(sel, prefer);
}

#[cfg(test)]
mod tests {
    use super::ActiveSupportAliases;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_starts_with() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                'some_string'.starts_with?('prefix')
                              ^^^^^^^^^^^^^^^^^^^^^^ Use `start_with?` instead of `starts_with?`.
            "#},
            "'some_string'.start_with?('prefix')\n",
        );
    }

    #[test]
    fn flags_csend_starts_with() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                'some_string'&.starts_with?('prefix')
                               ^^^^^^^^^^^^^^^^^^^^^^ Use `start_with?` instead of `starts_with?`.
            "#},
            "'some_string'&.start_with?('prefix')\n",
        );
    }

    #[test]
    fn flags_ends_with() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                'some_string'.ends_with?('prefix')
                              ^^^^^^^^^^^^^^^^^^^^ Use `end_with?` instead of `ends_with?`.
            "#},
            "'some_string'.end_with?('prefix')\n",
        );
    }

    #[test]
    fn flags_csend_ends_with() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                'some_string'&.ends_with?('prefix')
                               ^^^^^^^^^^^^^^^^^^^^ Use `end_with?` instead of `ends_with?`.
            "#},
            "'some_string'&.end_with?('prefix')\n",
        );
    }

    #[test]
    fn flags_append_without_correction() {
        test::<ActiveSupportAliases>().expect_offense(indoc! {r#"
            [1, 'a', 3].append('element')
                        ^^^^^^^^^^^^^^^^^ Use `<<` instead of `append`.
        "#});
    }

    #[test]
    fn flags_csend_append_without_correction() {
        test::<ActiveSupportAliases>().expect_offense(indoc! {r#"
            [1, 'a', 3]&.append('element')
                         ^^^^^^^^^^^^^^^^^ Use `<<` instead of `append`.
        "#});
    }

    #[test]
    fn flags_prepend() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                [1, 'a', 3].prepend('element')
                            ^^^^^^^^^^^^^^^^^^ Use `unshift` instead of `prepend`.
            "#},
            "[1, 'a', 3].unshift('element')\n",
        );
    }

    #[test]
    fn flags_csend_prepend() {
        test::<ActiveSupportAliases>().expect_correction(
            indoc! {r#"
                [1, 'a', 3]&.prepend('element')
                             ^^^^^^^^^^^^^^^^^^ Use `unshift` instead of `prepend`.
            "#},
            "[1, 'a', 3]&.unshift('element')\n",
        );
    }

    #[test]
    fn allows_start_with() {
        test::<ActiveSupportAliases>()
            .expect_no_offenses("'some_string'.start_with?('prefix')\n");
    }

    #[test]
    fn allows_end_with() {
        test::<ActiveSupportAliases>()
            .expect_no_offenses("'some_string'.end_with?('prefix')\n");
    }

    #[test]
    fn allows_shift_operator() {
        test::<ActiveSupportAliases>()
            .expect_no_offenses("[1, 'a', 3] << 'element'\n");
    }

    #[test]
    fn allows_unshift() {
        test::<ActiveSupportAliases>()
            .expect_no_offenses("[1, 'a', 3].unshift('element')\n");
    }

    #[test]
    fn ignores_non_literal_receiver() {
        test::<ActiveSupportAliases>().expect_no_offenses("foo.starts_with?('prefix')\n");
    }

    #[test]
    fn ignores_non_literal_array_receiver() {
        test::<ActiveSupportAliases>().expect_no_offenses("foo.append('element')\n");
    }

    #[test]
    fn ignores_multi_arg() {
        test::<ActiveSupportAliases>()
            .expect_no_offenses("'some_string'.starts_with?('a', 'b')\n");
    }

    #[test]
    fn ignores_zero_arg() {
        test::<ActiveSupportAliases>().expect_no_offenses("[1, 'a', 3].append\n");
    }
}

murphy_plugin_api::submit_cop!(ActiveSupportAliases);
