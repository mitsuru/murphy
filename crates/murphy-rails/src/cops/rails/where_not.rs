//! `Rails/WhereNot` — use `where.not` hash instead of manual negated SQL.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereNot
//! upstream_version_checked: 2.35.0
//! version_added: "2.8"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_csend` with
//!   RESTRICT_ON_SEND [:where]. Template must be a plain Str;
//!   array-wrapped (`where([...])`) unwrapped. Five negated SQL shapes
//!   (NOT_EQ_ANONYMOUS/NOT_IN_ANONYMOUS/NOT_EQ_NAMED/NOT_IN_NAMED/
//!   IS_NOT_NULL) with `column.count('.') <= 1` gate; named placeholders
//!   require a Hash value with a matching pair. Offense from selector to
//!   node end; autocorrect to `where.not(col: val)` or
//!   `where.not(tbl: { col: val })` preserving `.`/`&.` via the call
//!   operator. Disabled upstream by default (`Enabled: pending`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, regex::Regex};
use std::sync::OnceLock;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereNot;

const MSG_TMPL: &str = "Use `%GOOD%` instead of manually constructing negated SQL in `where`.";

fn message(good: &str) -> String {
    MSG_TMPL.replace("%GOOD%", good)
}

#[cop(
    name = "Rails/WhereNot",
    description = "Use `where.not(...)` instead of manually constructing negated SQL in `where`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereNot {
    #[on_node(kind = "send", methods = ["where"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn not_eq_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A([\w.]+)\s+(?:!=|<>)\s+\?\z").unwrap())
}
fn not_in_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+NOT\s+IN\s+\(\?\)\z").unwrap())
}
fn not_eq_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A([\w.]+)\s+(?:!=|<>)\s+:(\w+)\z").unwrap())
}
fn not_in_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+NOT\s+IN\s+\(:(\w+)\)\z").unwrap())
}
fn is_not_null() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+IS\s+NOT\s+NULL\z").unwrap())
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("where") {
        return;
    }
    let args = cx.call_arguments(node);
    let (template_node, value_node_opt) = if args.len() == 1 && matches!(*cx.kind(args[0]), NodeKind::Array(_)) {
        let NodeKind::Array(list) = *cx.kind(args[0]) else {
            return;
        };
        let elems = cx.list(list);
        if elems.is_empty() {
            return;
        }
        if !matches!(*cx.kind(elems[0]), NodeKind::Str(_)) {
            return;
        }
        let val = if elems.len() >= 2 { Some(elems[1]) } else { None };
        (elems[0], val)
    } else if !args.is_empty() && matches!(*cx.kind(args[0]), NodeKind::Str(_)) {
        let val = if args.len() >= 2 { Some(args[1]) } else { None };
        (args[0], val)
    } else {
        return;
    };
    let NodeKind::Str(sid) = *cx.kind(template_node) else {
        return;
    };
    let template = cx.string_str(sid).to_owned();
    let Some((column, value_src)) = extract_column_and_value(cx, &template, value_node_opt) else {
        return;
    };
    if column.matches('.').count() > 1 {
        return;
    }
    let dot = cx
        .call_operator_loc(node)
        .map(|r| cx.raw_source(r).to_owned())
        .unwrap_or_else(|| ".".to_owned());
    let good = build_good_method(&dot, &column, &value_src);
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(range, &message(&good), None);
    cx.emit_edit(range, &good);
}

fn extract_column_and_value(
    cx: &Cx<'_>,
    template: &str,
    value_node_opt: Option<NodeId>,
) -> Option<(String, String)> {
    if let Some(caps) = not_eq_anon().captures(template) {
        let col = caps[1].to_owned();
        let val_id = value_node_opt?;
        let src = cx.raw_source(cx.range(val_id)).to_owned();
        return Some((col, src));
    }
    if let Some(caps) = not_in_anon().captures(template) {
        let col = caps[1].to_owned();
        let val_id = value_node_opt?;
        let src = cx.raw_source(cx.range(val_id)).to_owned();
        return Some((col, src));
    }
    if let Some(caps) = not_eq_named().captures(template) {
        let col = caps[1].to_owned();
        let key = caps[2].to_owned();
        let val_id = value_node_opt?;
        if !matches!(*cx.kind(val_id), NodeKind::Hash(_)) {
            return None;
        }
        let pairs = cx.hash_pairs(val_id);
        for &p in &pairs {
            if !matches!(*cx.kind(p), NodeKind::Pair { .. }) {
                continue;
            }
            let NodeKind::Pair { key: k, value: v } = *cx.kind(p) else {
                continue;
            };
            let matches = match *cx.kind(k) {
                NodeKind::Sym(s) => cx.symbol_str(s) == key,
                NodeKind::Str(sid) => cx.string_str(sid) == key,
                _ => false,
            };
            if matches {
                let src = cx.raw_source(cx.range(v)).to_owned();
                return Some((col, src));
            }
        }
        return None;
    }
    if let Some(caps) = not_in_named().captures(template) {
        let col = caps[1].to_owned();
        let key = caps[2].to_owned();
        let val_id = value_node_opt?;
        if !matches!(*cx.kind(val_id), NodeKind::Hash(_)) {
            return None;
        }
        let pairs = cx.hash_pairs(val_id);
        for &p in &pairs {
            if !matches!(*cx.kind(p), NodeKind::Pair { .. }) {
                continue;
            }
            let NodeKind::Pair { key: k, value: v } = *cx.kind(p) else {
                continue;
            };
            let matches = match *cx.kind(k) {
                NodeKind::Sym(s) => cx.symbol_str(s) == key,
                NodeKind::Str(sid) => cx.string_str(sid) == key,
                _ => false,
            };
            if matches {
                let src = cx.raw_source(cx.range(v)).to_owned();
                return Some((col, src));
            }
        }
        return None;
    }
    if let Some(caps) = is_not_null().captures(template) {
        let col = caps[1].to_owned();
        return Some((col, "nil".to_owned()));
    }
    None
}

fn build_good_method(dot: &str, column: &str, value: &str) -> String {
    if column.contains('.') {
        let mut parts = column.splitn(2, '.');
        let table = parts.next().unwrap_or("");
        let col = parts.next().unwrap_or("");
        format!("where{dot}not({table}: {{ {col}: {value} }})")
    } else {
        format!("where{dot}not({column}: {value})")
    }
}

#[cfg(test)]
mod tests {
    use super::WhereNot;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_not_eq_anonymous() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where('name != ?', 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: 'Gabe')` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_not_eq_diamond() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where('name <> ?', 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: 'Gabe')` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_not_eq_named() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where('name != :name', name: 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: 'Gabe')` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_is_not_null() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where('name IS NOT NULL')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: nil)` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: nil)\n",
        );
    }

    #[test]
    fn flags_not_in_anonymous() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where("name NOT IN (?)", ['john', 'jane'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: ['john', 'jane'])` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: ['john', 'jane'])\n",
        );
    }

    #[test]
    fn flags_not_in_named() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where("name NOT IN (:names)", names: ['john', 'jane'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: ['john', 'jane'])` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: ['john', 'jane'])\n",
        );
    }

    #[test]
    fn flags_namespaced_column() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where('users.name != :name', name: 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(users: { name: 'Gabe' })` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(users: { name: 'Gabe' })\n",
        );
    }

    #[test]
    fn flags_array_form() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User.where(['name != ?', 'Gabe'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where.not(name: 'Gabe')` instead of manually constructing negated SQL in `where`.
            "#},
            "User.where.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_safe_navigation() {
        test::<WhereNot>().expect_correction(
            indoc! {r#"
                User&.where('name != ?', 'Gabe')
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where&.not(name: 'Gabe')` instead of manually constructing negated SQL in `where`.
            "#},
            "User&.where&.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn no_offense_hash_where() {
        test::<WhereNot>().expect_no_offenses("User.where.not(name: 'Gabe')\n");
    }

    #[test]
    fn no_offense_positive_eq() {
        test::<WhereNot>().expect_no_offenses("User.where('name = ?', 'john')\n");
    }

    #[test]
    fn no_offense_and_chain() {
        test::<WhereNot>().expect_no_offenses("User.where('name != ? AND age != ?', 'john', 19)\n");
    }

    #[test]
    fn no_offense_double_qualified() {
        test::<WhereNot>().expect_no_offenses("User.where('database.users.name != ?', 'Gabe')\n");
    }

    #[test]
    fn no_offense_missing_value() {
        test::<WhereNot>().expect_no_offenses("User.where('name != ?', )\n");
    }
}

murphy_plugin_api::submit_cop!(WhereNot);
