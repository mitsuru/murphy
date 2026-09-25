//! `Rails/WhereRange` — use ranges in `where` instead of manual SQL.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereRange
//! upstream_version_checked: 2.35.0
//! version_added: "2.25"
//! safe: false
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND
//!   [:where, :not] (`not` gated on a `where` receiver). Template must be
//!   a plain Str; array-wrapped unwrapped with all values kept (RANGE needs
//!   >= 2). Six SQL shapes (GTEQ/LTEQ/RANGE x ANONYMOUS/NAMED) with
//!   `column.count('.') <= 1`; named placeholders require a Hash value with
//!   matching pairs. Gated on `TargetRailsVersion >= 6.0` and
//!   `TargetRubyVersion >= 2.6` (unset means newest); `<`-bound LTEQ shapes
//!   additionally require Ruby >= 2.7 (beginless `...`). Offense from
//!   selector to node end; autocorrect to `where(col: lhs..rhs)` or
//!   `where(tbl: { col: ... })` with parens for non-trivial bounds.
//!   Disabled upstream by default (`Enabled: pending`, `SafeAutoCorrect:
//!   false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RubyVersion, cop, regex::Regex};
use std::sync::OnceLock;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereRange;

const MSG_TMPL: &str = "Use `%GOOD%` instead of manually constructing SQL.";

fn message(good: &str) -> String {
    MSG_TMPL.replace("%GOOD%", good)
}

#[cop(
    name = "Rails/WhereRange",
    description = "Use ranges in `where` instead of manually constructing SQL.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereRange {
    #[on_node(kind = "send", methods = ["where", "not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn gteq_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A\s*([\w.]+)\s+>=\s+\?\s*\z").unwrap())
}
fn lteq_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A\s*([\w.]+)\s+(<=?)\s+\?\s*\z").unwrap())
}
fn range_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\A\s*([\w.]+)\s+>=\s+\?\s+(?i:AND)\s+([\w.]+)\s+(<=?)\s+\?\s*\z").unwrap()
    })
}
fn gteq_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A\s*([\w.]+)\s+>=\s+:(\w+)\s*\z").unwrap())
}
fn lteq_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A\s*([\w.]+)\s+(<=?)\s+:(\w+)\s*\z").unwrap())
}
fn range_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\A\s*([\w.]+)\s+>=\s+:(\w+)\s+(?i:AND)\s+([\w.]+)\s+(<=?)\s+:(\w+)\s*\z").unwrap()
    })
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "where" && method != "not" {
        return;
    }
    if method == "not" && !is_where_not(cx, node) {
        return;
    }
    // Upstream `minimum_target_rails_version 6.0`, `minimum_target_ruby_version 2.6`.
    if !cx.rails_version_at_least(6, 0) {
        return;
    }
    if cx
        .target_ruby_version()
        .is_some_and(|v| v < RubyVersion::new(2, 6))
    {
        return;
    }
    let args = cx.call_arguments(node);
    let (template_node, values): (NodeId, Vec<NodeId>) = if args.len() == 1
        && matches!(*cx.kind(args[0]), NodeKind::Array(_))
    {
        let NodeKind::Array(list) = *cx.kind(args[0]) else {
            return;
        };
        let elems = cx.list(list);
        if elems.is_empty() || !matches!(*cx.kind(elems[0]), NodeKind::Str(_)) {
            return;
        }
        (elems[0], elems[1..].to_vec())
    } else if !args.is_empty() && matches!(*cx.kind(args[0]), NodeKind::Str(_)) {
        (args[0], args[1..].to_vec())
    } else {
        return;
    };
    let NodeKind::Str(sid) = *cx.kind(template_node) else {
        return;
    };
    let template = cx.string_str(sid).to_owned();
    let Some((column, value_src)) = extract(cx, &template, &values) else {
        return;
    };
    if column.matches('.').count() > 1 {
        return;
    }
    let good = build_good_method(&method, &column, &value_src);
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(range, &message(&good), None);
    cx.emit_edit(range, &good);
}

fn is_where_not(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    cx.method_name(recv) == Some("where")
}

fn ruby_at_least(cx: &Cx<'_>, major: u16, minor: u16) -> bool {
    cx.target_ruby_version()
        .is_none_or(|v| v >= RubyVersion::new(major, minor))
}

fn range_operator(op: &str) -> &'static str {
    if op == "<" { "..." } else { ".." }
}

fn find_pair(cx: &Cx<'_>, hash: NodeId, key: &str) -> Option<NodeId> {
    for &p in &cx.hash_pairs(hash) {
        if !matches!(*cx.kind(p), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key: k, value: _ } = *cx.kind(p) else {
            continue;
        };
        let matches = match *cx.kind(k) {
            NodeKind::Sym(s) => cx.symbol_str(s) == key,
            NodeKind::Str(sid) => cx.string_str(sid) == key,
            _ => false,
        };
        if matches {
            return Some(p);
        }
    }
    None
}

fn pair_value(cx: &Cx<'_>, pair: NodeId) -> NodeId {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return pair;
    };
    value
}

// Returns (column, "lhs..rhs") with parens where needed.
fn extract(cx: &Cx<'_>, template: &str, values: &[NodeId]) -> Option<(String, String)> {
    // Each branch mirrors upstream `extract_column_and_value`.
    if let Some(caps) = gteq_anon().captures(template) {
        let col = caps[1].to_owned();
        let lhs = *values.first()?;
        let lhs_src = maybe_paren(cx, lhs);
        return Some((col, format!("{lhs_src}..")));
    }
    if let Some(caps) = lteq_anon().captures(template) {
        if !ruby_at_least(cx, 2, 7) {
            return None;
        }
        let col = caps[1].to_owned();
        let op = caps[2].to_owned();
        let rhs = *values.first()?;
        let rhs_src = maybe_paren(cx, rhs);
        let rop = range_operator(&op);
        return Some((col, format!("{rop}{rhs_src}")));
    }
    if let Some(caps) = range_anon().captures(template) {
        if values.len() < 2 {
            return None;
        }
        let col1 = caps[1].to_owned();
        let col2 = caps[2].to_owned();
        if !col1.eq_ignore_ascii_case(&col2) {
            return None;
        }
        let col = col1;
        let op = caps[3].to_owned();
        let lhs_src = maybe_paren(cx, values[0]);
        let rhs_src = maybe_paren(cx, values[1]);
        let rop = range_operator(&op);
        return Some((col, format!("{lhs_src}{rop}{rhs_src}")));
    }
    if let Some(caps) = gteq_named().captures(template) {
        let col = caps[1].to_owned();
        let key = caps[2].to_owned();
        let hash = *values.first()?;
        if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
            return None;
        }
        let pair = find_pair(cx, hash, &key)?;
        let lhs_src = maybe_paren(cx, pair_value(cx, pair));
        return Some((col, format!("{lhs_src}..")));
    }
    if let Some(caps) = lteq_named().captures(template) {
        if !ruby_at_least(cx, 2, 7) {
            return None;
        }
        let col = caps[1].to_owned();
        let op = caps[2].to_owned();
        let key = caps[3].to_owned();
        let hash = *values.first()?;
        if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
            return None;
        }
        let pair = find_pair(cx, hash, &key)?;
        let rhs_src = maybe_paren(cx, pair_value(cx, pair));
        let rop = range_operator(&op);
        return Some((col, format!("{rop}{rhs_src}")));
    }
    if let Some(caps) = range_named().captures(template) {
        let col1 = caps[1].to_owned();
        let key1 = caps[2].to_owned();
        let col2 = caps[3].to_owned();
        let op = caps[4].to_owned();
        let key2 = caps[5].to_owned();
        if !col1.eq_ignore_ascii_case(&col2) {
            return None;
        }
        let col = col1;
        let hash = *values.first()?;
        if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
            return None;
        }
        let p1 = find_pair(cx, hash, &key1)?;
        let p2 = find_pair(cx, hash, &key2)?;
        let lhs_src = maybe_paren(cx, pair_value(cx, p1));
        let rhs_src = maybe_paren(cx, pair_value(cx, p2));
        let rop = range_operator(&op);
        return Some((col, format!("{lhs_src}{rop}{rhs_src}")));
    }
    None
}

fn maybe_paren(cx: &Cx<'_>, id: NodeId) -> String {
    let src = cx.raw_source(cx.range(id)).to_owned();
    if parentheses_not_needed(cx, id) {
        src
    } else {
        format!("({src})")
    }
}

fn parentheses_not_needed(cx: &Cx<'_>, id: NodeId) -> bool {
    if cx.is_variable(id) || cx.is_literal(id) || cx.is_reference(id) {
        return true;
    }
    if matches!(*cx.kind(id), NodeKind::Const { .. } | NodeKind::Begin(_)) {
        return true;
    }
    if matches!(*cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        let args = cx.call_arguments(id);
        if args.is_empty() {
            return true;
        }
        if cx.is_parenthesized(id) {
            return true;
        }
        return false;
    }
    false
}

fn build_good_method(method: &str, column: &str, value: &str) -> String {
    if column.contains('.') {
        let mut parts = column.splitn(2, '.');
        let table = parts.next().unwrap_or("");
        let col = parts.next().unwrap_or("");
        format!("{method}({table}: {{ {col}: {value} }})")
    } else {
        format!("{method}({column}: {value})")
    }
}

#[cfg(test)]
mod tests {
    use super::WhereRange;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_gteq_anonymous() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('age >= ?', 18)
                     ^^^^^^^^^^^^^^^^^^^^^ Use `where(age: 18..)` instead of manually constructing SQL.
            "#},
            "User.where(age: 18..)\n",
        );
    }

    #[test]
    fn flags_lteq_anonymous() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('age < ?', 18)
                     ^^^^^^^^^^^^^^^^^^^^ Use `where(age: ...18)` instead of manually constructing SQL.
            "#},
            "User.where(age: ...18)\n",
        );
    }

    #[test]
    fn flags_lteq_eq_anonymous() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('age <= ?', 18)
                     ^^^^^^^^^^^^^^^^^^^^^ Use `where(age: ..18)` instead of manually constructing SQL.
            "#},
            "User.where(age: ..18)\n",
        );
    }

    #[test]
    fn flags_range_anonymous() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('age >= ? AND age < ?', 18, 21)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(age: 18...21)` instead of manually constructing SQL.
            "#},
            "User.where(age: 18...21)\n",
        );
    }

    #[test]
    fn flags_gteq_named() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('age >= :start', start: 18)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(age: 18..)` instead of manually constructing SQL.
            "#},
            "User.where(age: 18..)\n",
        );
    }

    #[test]
    fn flags_namespaced() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where('users.age >= ?', 18)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(users: { age: 18.. })` instead of manually constructing SQL.
            "#},
            "User.where(users: { age: 18.. })\n",
        );
    }

    #[test]
    fn flags_where_not() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where.not('age >= ?', 18)
                           ^^^^^^^^^^^^^^^^^^^ Use `not(age: 18..)` instead of manually constructing SQL.
            "#},
            "User.where.not(age: 18..)\n",
        );
    }

    #[test]
    fn flags_array_form() {
        test::<WhereRange>().expect_correction(
            indoc! {r#"
                User.where(['age >= ?', 18])
                     ^^^^^^^^^^^^^^^^^^^^^^^ Use `where(age: 18..)` instead of manually constructing SQL.
            "#},
            "User.where(age: 18..)\n",
        );
    }

    #[test]
    fn no_offense_beginless_gt() {
        // `>` has no beginless-range form; upstream documents `age > ?` as good.
        test::<WhereRange>().expect_no_offenses("User.where('age > ?', 18)\n");
    }

    #[test]
    fn no_offense_hash() {
        test::<WhereRange>().expect_no_offenses("User.where(age: 18..)\n");
    }

    #[test]
    fn no_offense_bare_not() {
        test::<WhereRange>().expect_no_offenses("users.not('age >= ?', 18)\n");
    }

    #[test]
    fn no_offense_double_qualified() {
        test::<WhereRange>().expect_no_offenses("User.where('a.b.age >= ?', 18)\n");
    }
}

murphy_plugin_api::submit_cop!(WhereRange);
