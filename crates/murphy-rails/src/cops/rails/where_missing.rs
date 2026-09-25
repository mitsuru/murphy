//! `Rails/WhereMissing` — use `where.missing` for missing associations.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereMissing
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND
//!   [:left_joins, :left_outer_joins] behind `minimum_target_rails_version
//!   6.1` (unset means newest). First arg must be Sym. Root-receiver chain
//!   climbing skips `or`/`and` sends; a `where(Hash)` in the same root chain
//!   containing `sym => { id: nil }` with `same_relationship?`
//!   (`^left_joins s?$`) triggers. Offense on `left_joins(:arg)` (selector to
//!   node end); autocorrect replaces the selector with `where.missing` and
//!   either drops the matching pair (multi-condition hash) or removes the
//!   whole `where` (dot handling + whole-line removal for multiline chains,
//!   mirroring upstream `remove_where_method`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereMissing;

#[cop(
    name = "Rails/WhereMissing",
    description = "Use `where.missing(...)` to find missing relationship records.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereMissing {
    #[on_node(kind = "send", methods = ["left_joins", "left_outer_joins"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(6, 1) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "left_joins" && method != "left_outer_joins" {
        return;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }
    if !matches!(*cx.kind(args[0]), NodeKind::Sym(_)) {
        return;
    }
    let left_sym = match *cx.kind(args[0]) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        _ => return,
    };
    let root = root_receiver(cx, node);
    // Collect where candidates in root subtree (root itself + descendants).
    let mut candidates = vec![root];
    candidates.extend(cx.descendants(root));
    for where_node in candidates {
        if !matches!(*cx.kind(where_node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        if cx.method_name(where_node) != Some("where") {
            continue;
        }
        let wargs = cx.call_arguments(where_node);
        if wargs.is_empty() {
            continue;
        }
        // Upstream pattern requires a Hash arg (first arg is hash with id:nil).
        // Handle `where(hash)` single hash arg.
        let hash = wargs[0];
        if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
            continue;
        }
        let Some((pair_id, where_key)) = find_missing_pair(cx, hash, &left_sym) else {
            continue;
        };
        if root_receiver(cx, where_node) != root {
            continue;
        }
        // Found match — emit.
        let range = Range {
            start: cx.selector(node).start,
            end: cx.range(node).end,
        };
        let msg = format!(
            "Use `where.missing(:{left_sym})` instead of `{method}(:{left_sym}).where({where_key}: {{ id: nil }})`."
        );
        // Note: upstream message uses actual where_key (e.g. foos) and method.
        // We reconstruct to match spec exactly (see tests).
        cx.emit_offense(range, &msg, None);
        // Replace `left_joins` selector with `where.missing`.
        cx.emit_edit(cx.selector(node), "where.missing");
        let pairs = cx.hash_pairs(hash);
        if pairs.len() > 1 {
            // Multi-condition: remove only the matching pair.
            if let Some(remove_range) = pair_remove_range(cx, hash, pair_id) {
                cx.emit_edit(remove_range, "");
            }
        } else {
            // Single-condition: remove whole where.
            remove_where_call(cx, node, where_node);
        }
        break;
    }
}

fn sym_value(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        _ => None,
    }
}

fn find_missing_pair(cx: &Cx<'_>, hash: NodeId, left_sym: &str) -> Option<(NodeId, String)> {
    for &p in &cx.hash_pairs(hash) {
        if !matches!(*cx.kind(p), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key, value } = *cx.kind(p) else {
            continue;
        };
        let Some(k) = sym_value(cx, key) else {
            continue;
        };
        if !same_relationship(&k, left_sym) {
            continue;
        }
        // Value must be `{ id: nil }` (single pair).
        if !matches!(*cx.kind(value), NodeKind::Hash(_)) {
            continue;
        }
        let inner = cx.hash_pairs(value);
        if inner.len() != 1 {
            continue;
        }
        let inner_pair = inner[0];
        if !matches!(*cx.kind(inner_pair), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key: ik, value: iv } = *cx.kind(inner_pair) else {
            continue;
        };
        if sym_value(cx, ik).as_deref() != Some("id") {
            continue;
        }
        if !matches!(*cx.kind(iv), NodeKind::Nil) {
            continue;
        }
        return Some((p, k));
    }
    None
}

fn same_relationship(where_key: &str, left_arg: &str) -> bool {
    // Upstream: `where.value.to_s.match?(/^#{left_joins.value}s?$/)`
    where_key == left_arg || where_key == format!("{left_arg}s")
}

fn root_receiver(cx: &Cx<'_>, mut node: NodeId) -> NodeId {
    loop {
        let Some(parent) = cx.parent(node).get() else {
            return node;
        };
        let is_send = matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. });
        if !is_send {
            return node;
        }
        if let Some(m) = cx.method_name(parent)
            && (m == "or" || m == "and")
        {
            return node;
        }
        node = parent;
    }
}

fn pair_remove_range(cx: &Cx<'_>, hash: NodeId, pair: NodeId) -> Option<Range> {
    let pairs = cx.hash_pairs(hash);
    let pos = pairs.iter().position(|&p| p == pair)?;
    if let Some(&right) = pairs.get(pos + 1) {
        // From pair start to right sibling start (includes `, `).
        Some(Range {
            start: cx.range(pair).start,
            end: cx.range(right).start,
        })
    } else if pos > 0 {
        let left = pairs[pos - 1];
        Some(Range {
            start: cx.range(left).end,
            end: cx.range(pair).end,
        })
    } else {
        // Single pair but called in multi-condition? Should not happen.
        Some(cx.range(pair))
    }
}

fn line_of(source: &str, offset: u32) -> usize {
    let off = offset as usize;
    let upto = &source[..off.min(source.len())];
    upto.bytes().filter(|&b| b == b'\n').count()
}

fn remove_where_call(cx: &Cx<'_>, left_joins_node: NodeId, where_node: NodeId) {
    let where_range = Range {
        start: cx.selector(where_node).start,
        end: cx.range(where_node).end,
    };
    let src = cx.source().to_owned();
    let left_multiline = cx.is_multiline(left_joins_node);
    let same_line = line_of(&src, cx.selector(left_joins_node).start)
        == line_of(&src, cx.selector(where_node).start);
    if left_multiline && !same_line {
        let whole = cx.range_by_whole_lines(where_range, true);
        // Upstream expands to whole lines (including leading indent + trailing
        // newline) when multiline with leading/trailing dots. Our
        // `range_by_whole_lines` mirrors that; remove the whole line.
        // Note: the dot before `where` lives on the same line (leading-dot)
        // or previous line (trailing-dot) and is included in the whole-line
        // range, so no separate dot removal is needed in this branch.
        cx.emit_edit(whole, "");
        return;
    }
    // Single-line (or same-line multiline): remove dot + where range.
    if cx.call_receiver(where_node).get().is_some() {
        if let Some(dot) = cx.call_operator_loc(where_node) {
            cx.emit_edit(dot, "");
        }
    } else if let Some(dot) = cx.call_operator_loc(left_joins_node) {
        cx.emit_edit(dot, "");
    }
    cx.emit_edit(where_range, "");
}

#[cfg(test)]
mod tests {
    use super::WhereMissing;
    use murphy_plugin_api::test_support::{indoc, test};

    fn rails61<T: murphy_plugin_api::NodeCop + Default>(
        t: murphy_plugin_api::test_support::Tester<T>,
    ) -> murphy_plugin_api::test_support::Tester<T> {
        t.with_target_rails_version(6, 1)
    }

    #[test]
    fn flags_left_joins_where() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_joins(:foo).where(foos: { id: nil }).where(bar: "bar")
                    ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_left_joins_singular_where() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_joins(:foo).where(foo: { id: nil }).where(bar: "bar")
                    ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foo: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_bare_chain() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                left_joins(:foo).where(foo: { id: nil }).where(bar: "bar")
                ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foo: { id: nil })`.
            "#},
            "where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_left_outer_joins() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_outer_joins(:foo).where(foos: { id: nil }).where(bar: "bar")
                    ^^^^^^^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_outer_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_where_before_left_joins() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.where(foos: { id: nil }).left_joins(:foo).where(bar: "bar")
                                             ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_where_multi_then_left_joins() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.where(foos: { id: nil }, bar: "bar").left_joins(:foo)
                                                         ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where(bar: \"bar\").where.missing(:foo)\n",
        );
    }

    #[test]
    fn flags_multi_condition_after() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_joins(:foo).where(foos: { id: nil }, bar: "bar")
                    ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_multi_condition_before() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_joins(:foo).where(bar: "bar", foos: { id: nil })
                    ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_with_joins_between() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.left_joins(:foo).joins(:bar).where(foos: { id: nil })
                    ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
            "#},
            "Foo.where.missing(:foo).joins(:bar)\n",
        );
    }

    #[test]
    fn no_offense_or_separated() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.left_joins(:foo).or(Foo.where(foos: { id: nil }))\n",
        );
    }

    #[test]
    fn no_offense_and_separated() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.where(foos: { id: nil }).and(Foo.left_joins(:foo))\n",
        );
    }

    #[test]
    fn no_offense_different_relationship() {
        rails61(test::<WhereMissing>()).expect_no_offenses(indoc! {r#"
            Foo.left_joins(:foo).where(bazs: { id: nil })
        "#});
    }

    #[test]
    fn no_offense_not_id() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.left_joins(:foo).where(foos: { name: nil })\n",
        );
    }

    #[test]
    fn no_offense_not_nil() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.left_joins(:foo).where(foos: { id: 1 })\n",
        );
    }

    #[test]
    fn no_offense_hash_arg() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.left_joins(foo: :bar).where(bars: { id: nil })\n",
        );
    }

    #[test]
    fn no_offense_no_sym_arg() {
        rails61(test::<WhereMissing>()).expect_no_offenses(
            "Foo.left_joins(left_joins).where(bars: { id: nil })\n",
        );
    }

    #[test]
    fn flags_multiline_leading_dot() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo
                  .left_joins(:foo)
                   ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
                  .where(foos: { id: nil })
                  .where(bar: "bar")
            "#},
            "Foo\n  .where.missing(:foo)\n  .where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_multiline_trailing_dot() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo.
                  left_joins(:foo).
                  ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
                  where(foos: { id: nil }).
                  where(bar: "bar")
            "#},
            "Foo.\n  where.missing(:foo).\n  where(bar: \"bar\")\n",
        );
    }

    #[test]
    fn flags_multiline_multi_condition() {
        rails61(test::<WhereMissing>()).expect_correction(
            indoc! {r#"
                Foo
                  .left_joins(:foo)
                   ^^^^^^^^^^^^^^^^ Use `where.missing(:foo)` instead of `left_joins(:foo).where(foos: { id: nil })`.
                  .where(
                    foos: { id: nil },
                    bar: "bar"
                  )
            "#},
            "Foo\n  .where.missing(:foo)\n  .where(\n    bar: \"bar\"\n  )\n",
        );
    }

    #[test]
    fn no_offense_below_rails_61() {
        test::<WhereMissing>()
            .with_target_rails_version(6, 0)
            .expect_no_offenses("Foo.left_joins(:foo).where(foos: { id: nil }).where(bar: \"bar\")\n");
    }
}

murphy_plugin_api::submit_cop!(WhereMissing);
