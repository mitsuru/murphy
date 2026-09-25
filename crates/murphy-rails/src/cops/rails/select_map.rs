//! `Rails/SelectMap` — use `pluck` instead of `select` with `map`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SelectMap
//! upstream_version_checked: 2.35.0
//! version_added: "2.21"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `map`/`collect` (send and csend) with a
//!   single `&:column` block-pass arg, exactly one `select(:col)` /
//!   `select("col")` descendant with matching column name, exactly one such
//!   select in the whole map subtree, and a receiver chain from the map
//!   through the select (begin-unwrapped). Offense spans the select
//!   selector to the map end; autocorrect replaces that span with
//!   `pluck(:col)`. Unsafe upstream (`Safe: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SelectMap;

#[cop(
    name = "Rails/SelectMap",
    description = "Use `pluck` instead of `select` with `map`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl SelectMap {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[map collect]`.
    #[on_node(kind = "send", methods = ["map", "collect"])]
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
    if method != "map" && method != "collect" {
        return;
    }
    // Upstream `return unless node.first_argument`: need `&:column`.
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return;
    };
    let column = match block_pass_column(cx, first) {
        Some(c) => c,
        None => return,
    };
    let Some(select) = find_select_node(cx, node, &column) else {
        return;
    };
    let preferred = format!("pluck(:{column})");
    let offense = Range {
        start: cx.loc(select).name.start,
        end: cx.range(node).end,
    };
    cx.emit_offense(
        offense,
        &format!("Use `{preferred}` instead of `select` with `{method}`."),
        None,
    );
    cx.emit_edit(offense, &preferred);
}

/// `&:column` block-pass: `BlockPass(Sym(:column))` whose source starts
/// with `&:`. Returns the column name.
fn block_pass_column(cx: &Cx<'_>, arg: NodeId) -> Option<String> {
    let NodeKind::BlockPass(inner) = *cx.kind(arg) else {
        return None;
    };
    let sym_node = inner.get()?;
    if !matches!(*cx.kind(sym_node), NodeKind::Sym(_)) {
        return None;
    }
    let NodeKind::Sym(sym) = *cx.kind(sym_node) else {
        return None;
    };
    let name = cx.symbol_str(sym).to_owned();
    // Guard against non-`&:` block-pass (e.g. `map(&foo)`): source must be `&:col`.
    let src = cx.raw_source(cx.range(arg));
    if src.starts_with("&:") {
        Some(name)
    } else {
        None
    }
}

/// Upstream `find_select_node`: exactly one `select` descendant with a
/// matching single sym/str column arg, and a receiver chain from the map
/// through the select.
fn find_select_node(cx: &Cx<'_>, map_node: NodeId, column: &str) -> Option<NodeId> {
    let mut matches = Vec::new();
    for id in cx.descendants(map_node) {
        if !matches!(*cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        if cx.method_name(id) != Some("select") {
            continue;
        }
        if match_column_name(cx, id, column) && receiver_chain(cx, map_node, id) {
            matches.push(id);
        }
    }
    // Upstream requires exactly one; also include the direct-receiver case
    // where select is the immediate receiver (it is a descendant, so covered).
    // Note: `descendants` excludes the map node itself by definition.
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

/// Upstream `match_column_name?`: exactly one arg, sym (`:col`) or str
/// (`"col"`) equal to the map column.
fn match_column_name(cx: &Cx<'_>, select: NodeId, column: &str) -> bool {
    let args = cx.call_arguments(select);
    if args.len() != 1 {
        return false;
    }
    match *cx.kind(args[0]) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == column,
        NodeKind::Str(sid) => cx.string_str(sid) == column,
        _ => false,
    }
}

/// Upstream `receiver_chain?`: walk the map receiver chain (unwrapping
/// `begin`) until the select node is found.
fn receiver_chain(cx: &Cx<'_>, map_node: NodeId, select_node: NodeId) -> bool {
    let mut current = match cx.call_receiver(map_node).get() {
        Some(r) => r,
        None => return false,
    };
    loop {
        // Unwrap `(begin ...)` single-child wrappers like upstream.
        while let NodeKind::Begin(list) = *cx.kind(current) {
            let elems = cx.list(list);
            if elems.len() == 1 {
                current = elems[0];
            } else {
                break;
            }
        }
        if current == select_node {
            return true;
        }
        if !matches!(*cx.kind(current), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            return false;
        }
        match cx.call_receiver(current).get() {
            Some(r) => current = r,
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SelectMap;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_select_map() {
        test::<SelectMap>().expect_correction(
            indoc! {r#"
                Model.select(:column_name).map(&:column_name)
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `pluck(:column_name)` instead of `select` with `map`.
            "#},
            "Model.pluck(:column_name)
",
        );
    }

    #[test]
    fn flags_collect() {
        test::<SelectMap>().expect_correction(
            indoc! {r#"
                Model.select(:column_name).collect(&:column_name)
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `pluck(:column_name)` instead of `select` with `collect`.
            "#},
            "Model.pluck(:column_name)
",
        );
    }

    #[test]
    fn flags_chained() {
        test::<SelectMap>().expect_correction(
            indoc! {r#"
                Model.where(active: true).select(:name).map(&:name)
                                          ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `pluck(:name)` instead of `select` with `map`.
            "#},
            "Model.where(active: true).pluck(:name)
",
        );
    }

    #[test]
    fn flags_string_column() {
        test::<SelectMap>().expect_correction(
            indoc! {r#"
                Model.select("name").map(&:name)
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `pluck(:name)` instead of `select` with `map`.
            "#},
            "Model.pluck(:name)
",
        );
    }

    #[test]
    fn allows_mismatched_column() {
        test::<SelectMap>()
            .expect_no_offenses("Model.select(:a).map(&:b)
");
    }

    #[test]
    fn allows_no_select() {
        test::<SelectMap>().expect_no_offenses("Model.map(&:name)
");
    }

    #[test]
    fn allows_multi_arg_select() {
        test::<SelectMap>()
            .expect_no_offenses("Model.select(:a, :b).map(&:a)
");
    }

    #[test]
    fn allows_block_form() {
        test::<SelectMap>()
            .expect_no_offenses("Model.select(:a).map { |x| x.name }
");
    }
}
murphy_plugin_api::submit_cop!(SelectMap);
