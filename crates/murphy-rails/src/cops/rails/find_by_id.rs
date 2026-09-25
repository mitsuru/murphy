//! `Rails/FindById` — use `find` instead of `where.take!` / `find_by_id!` / `find_by!`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FindById
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: the three shapes (where+take!,
//!   find_by_id!, find_by! with id hash), single-pair id-hash gating,
//!   offense ranges (where selector to outer end; outer selector to
//!   outer end), and `find(id)` replacement. Send and Csend both match.
//!   Upstream ships `Enabled: pending`; Murphy maps that to
//!   `default_enabled = false`.
//! ```
//!
//! Enforces that `ActiveRecord#find` is used instead of `where.take!`,
//! `find_by!`, and `find_by_id!` to retrieve a single record by primary
//! key when you expect it to be found.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FindById;

#[cop(
    name = "Rails/FindById",
    description = "Favor the use of `find` over `where.take!`, `find_by!`, and `find_by_id!` when you need to retrieve a single record by primary key.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl FindById {
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
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    match method.as_str() {
        "take!" => check_where_take(node, cx),
        "find_by_id!" => check_find_by_id(node, cx),
        "find_by!" => check_find_by(node, cx),
        _ => {}
    }
}

/// `User.where(id: id).take!` → `User.find(id)`.
fn check_where_take(node: NodeId, cx: &Cx<'_>) {
    if !cx.call_arguments(node).is_empty() {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if cx.method_name(recv) != Some("where") {
        return;
    }
    if !matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    let Some(id_value) = single_id_hash_value(cx, recv) else {
        return;
    };
    let range = Range {
        start: cx.selector(recv).start,
        end: cx.range(node).end,
    };
    register(cx, range, id_value);
}

/// `User.find_by_id!(id)` → `User.find(id)`.
fn check_find_by_id(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let id_value = args[0];
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    register(cx, range, id_value);
}

/// `User.find_by!(id: id)` → `User.find(id)`.
fn check_find_by(node: NodeId, cx: &Cx<'_>) {
    let Some(id_value) = single_id_hash_value(cx, node) else {
        return;
    };
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    register(cx, range, id_value);
}

/// Returns the `id` value node when `node` is a call with exactly one
/// argument that is a `Hash` with exactly one `Pair` whose key is `:id`.
fn single_id_hash_value(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    let NodeKind::Hash(pairs) = *cx.kind(args[0]) else {
        return None;
    };
    let pairs = cx.list(pairs);
    if pairs.len() != 1 {
        return None;
    }
    let NodeKind::Pair { key, value } = *cx.kind(pairs[0]) else {
        return None;
    };
    let NodeKind::Sym(sym) = *cx.kind(key) else {
        return None;
    };
    if cx.symbol_str(sym) != "id" {
        return None;
    }
    Some(value)
}

fn register(cx: &Cx<'_>, range: Range, id_value: NodeId) {
    let id_src = cx.raw_source(cx.range(id_value)).to_owned();
    let bad_src = cx.raw_source(range).to_owned();
    let good = format!("find({id_src})");
    let msg = format!("Use `{good}` instead of `{bad_src}`.");
    cx.emit_offense(range, &msg, None);
    cx.emit_edit(range, &good);
}

#[cfg(test)]
mod tests {
    use super::FindById;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_where_take_bang() {
        test::<FindById>().expect_offense(indoc! {r#"
            User.where(id: id).take!
                 ^^^^^^^^^^^^^^^^^^^ Use `find(id)` instead of `where(id: id).take!`.
        "#});
    }

    #[test]
    fn autocorrects_where_take_bang() {
        test::<FindById>().expect_correction(
            indoc! {r#"
                User.where(id: id).take!
                     ^^^^^^^^^^^^^^^^^^^ Use `find(id)` instead of `where(id: id).take!`.
            "#},
            "User.find(id)\n",
        );
    }

    #[test]
    fn flags_find_by_id_bang() {
        test::<FindById>().expect_offense(indoc! {r#"
            User.find_by_id!(id)
                 ^^^^^^^^^^^^^^^ Use `find(id)` instead of `find_by_id!(id)`.
        "#});
    }

    #[test]
    fn autocorrects_find_by_id_bang() {
        test::<FindById>().expect_correction(
            indoc! {r#"
                User.find_by_id!(id)
                     ^^^^^^^^^^^^^^^ Use `find(id)` instead of `find_by_id!(id)`.
            "#},
            "User.find(id)\n",
        );
    }

    #[test]
    fn flags_find_by_bang() {
        test::<FindById>().expect_offense(indoc! {r#"
            User.find_by!(id: id)
                 ^^^^^^^^^^^^^^^^ Use `find(id)` instead of `find_by!(id: id)`.
        "#});
    }

    #[test]
    fn autocorrects_find_by_bang() {
        test::<FindById>().expect_correction(
            indoc! {r#"
                User.find_by!(id: id)
                     ^^^^^^^^^^^^^^^^ Use `find(id)` instead of `find_by!(id: id)`.
            "#},
            "User.find(id)\n",
        );
    }

    #[test]
    fn does_not_flag_where_take_bang_multi_pair() {
        test::<FindById>().expect_no_offenses("User.where(id: 1, name: 'x').take!\n");
    }

    #[test]
    fn does_not_flag_where_take_bang_non_id() {
        test::<FindById>().expect_no_offenses("User.where(name: 'x').take!\n");
    }

    #[test]
    fn does_not_flag_find_by_bang_non_id() {
        test::<FindById>().expect_no_offenses("User.find_by!(name: 'x')\n");
    }

    #[test]
    fn does_not_flag_take_bang_with_args() {
        test::<FindById>().expect_no_offenses("User.where(id: 1).take!(2)\n");
    }

    #[test]
    fn flags_safe_navigation_find_by_id() {
        test::<FindById>().expect_offense(indoc! {r#"
            User&.find_by_id!(id)
                  ^^^^^^^^^^^^^^^ Use `find(id)` instead of `find_by_id!(id)`.
        "#});
    }

    #[test]
    fn flags_safe_navigation_where_take() {
        test::<FindById>().expect_offense(indoc! {r#"
            User.where(id: id)&.take!
                 ^^^^^^^^^^^^^^^^^^^^ Use `find(id)` instead of `where(id: id)&.take!`.
        "#});
    }
}
murphy_plugin_api::submit_cop!(FindById);
