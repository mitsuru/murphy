//! `Rails/WhereEquals` — use hash conditions instead of manual SQL in `where`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereEquals
//! upstream_version_checked: 2.35.0
//! version_added: "2.9"
//! version_changed: "2.26"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_csend` with
//!   RESTRICT_ON_SEND [:where, :not] (`not` gated on a `where` receiver).
//!   Template must be a plain Str; array-wrapped (`where([...])`) unwrapped.
//!   Five SQL shapes (EQ_ANONYMOUS/IN_ANONYMOUS/EQ_NAMED/IN_NAMED/IS_NULL)
//!   with `column.count('.') <= 1` gate; named placeholders require a Hash
//!   value with a matching pair. Offense from selector to node end;
//!   autocorrect to `where(col: val)` or `where(tbl: { col: val })`.
//!   Disabled upstream by default (`Enabled: pending`, `SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, regex::Regex};
use std::sync::OnceLock;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereEquals;

const MSG_TMPL: &str = "Use `%GOOD%` instead of manually constructing SQL.";

fn message(good: &str) -> String {
    MSG_TMPL.replace("%GOOD%", good)
}

#[cop(
    name = "Rails/WhereEquals",
    description = "Pass conditions to `where` and `where.not` as a hash instead of manually constructing SQL.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereEquals {
    #[on_node(kind = "send", methods = ["where", "not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn eq_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A([\w.]+)\s+=\s+\?\z").unwrap())
}
fn in_anon() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+IN\s+\(\?\)\z").unwrap())
}
fn eq_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A([\w.]+)\s+=\s+:(\w+)\z").unwrap())
}
fn in_named() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+IN\s+\(:(\w+)\)\z").unwrap())
}
fn is_null() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\A([\w.]+)\s+IS\s+NULL\z").unwrap())
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
    let args = cx.call_arguments(node);
    // Unwrap array form: single Array arg.
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
        // Upstream array form with >2 elements? Only first two matter; extra
        // elements would make it not match (e.g. AND chain needs 3 args, but
        // array with 3 would still have template + value + extra? Upstream
        // pattern `(array $str $_ ?)` allows optional third, but
        // `extract` only uses first value; `AND` template fails regex anyway.
        (elems[0], val)
    } else if !args.is_empty() && matches!(*cx.kind(args[0]), NodeKind::Str(_)) {
        let val = if args.len() >= 2 { Some(args[1]) } else { None };
        // If more than 2 args (e.g. `where('a=? AND b=?', x, y)`), template
        // won't match regex (contains AND), so safe to take args[1] as value;
        // upstream would also fail to match and return.
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

fn extract_column_and_value(
    cx: &Cx<'_>,
    template: &str,
    value_node_opt: Option<NodeId>,
) -> Option<(String, String)> {
    if let Some(caps) = eq_anon().captures(template) {
        let col = caps[1].to_owned();
        let val_id = value_node_opt?;
        let src = cx.raw_source(cx.range(val_id)).to_owned();
        return Some((col, src));
    }
    if let Some(caps) = in_anon().captures(template) {
        let col = caps[1].to_owned();
        let val_id = value_node_opt?;
        let src = cx.raw_source(cx.range(val_id)).to_owned();
        return Some((col, src));
    }
    if let Some(caps) = eq_named().captures(template) {
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
    if let Some(caps) = in_named().captures(template) {
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
    if let Some(caps) = is_null().captures(template) {
        let col = caps[1].to_owned();
        return Some((col, "nil".to_owned()));
    }
    None
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
    use super::WhereEquals;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_eq_anonymous() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where('name = ?', 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_eq_anonymous_with_not() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where.not('name = ?', 'Gabe')
                           ^^^^^^^^^^^^^^^^^^^^^^^ Use `not(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User.where.not(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_safe_navigation() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User&.where('name = ?', 'Gabe')
                      ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User&.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_eq_named() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where('name = :name', name: 'Gabe')
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_is_null() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where('name IS NULL')
                     ^^^^^^^^^^^^^^^^^^^^^ Use `where(name: nil)` instead of manually constructing SQL.
            "#},
            "User.where(name: nil)\n",
        );
    }

    #[test]
    fn flags_in_anonymous() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where("name IN (?)", ['john', 'jane'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: ['john', 'jane'])` instead of manually constructing SQL.
            "#},
            "User.where(name: ['john', 'jane'])\n",
        );
    }

    #[test]
    fn flags_in_named() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where("name IN (:names)", names: ['john', 'jane'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: ['john', 'jane'])` instead of manually constructing SQL.
            "#},
            "User.where(name: ['john', 'jane'])\n",
        );
    }

    #[test]
    fn flags_namespaced_column() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                Course.where('enrollments.student_id = ?', student.id)
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(enrollments: { student_id: student.id })` instead of manually constructing SQL.
            "#},
            "Course.where(enrollments: { student_id: student.id })\n",
        );
    }

    #[test]
    fn flags_array_form() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where(['name = ?', 'Gabe'])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_array_safe_navigation() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User&.where(['name = ?', 'Gabe'])
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User&.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_array_named() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where(['name = :name', { name: 'Gabe' }])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: 'Gabe')` instead of manually constructing SQL.
            "#},
            "User.where(name: 'Gabe')\n",
        );
    }

    #[test]
    fn flags_array_is_null() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where(['name IS NULL'])
                     ^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: nil)` instead of manually constructing SQL.
            "#},
            "User.where(name: nil)\n",
        );
    }

    #[test]
    fn flags_array_in_anonymous() {
        test::<WhereEquals>().expect_correction(
            indoc! {r#"
                User.where(["name IN (?)", ['john', 'jane']])
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `where(name: ['john', 'jane'])` instead of manually constructing SQL.
            "#},
            "User.where(name: ['john', 'jane'])\n",
        );
    }

    #[test]
    fn no_offense_hash_where() {
        test::<WhereEquals>().expect_no_offenses("User.where(name: nil)\n");
    }

    #[test]
    fn no_offense_hash_name() {
        test::<WhereEquals>().expect_no_offenses("User.where(name: 'john')\n");
    }

    #[test]
    fn no_offense_negation() {
        test::<WhereEquals>().expect_no_offenses("User.where('name != ?', 'john')\n");
    }

    #[test]
    fn no_offense_and_chain() {
        test::<WhereEquals>().expect_no_offenses("User.where('name = ? AND age = ?', 'john', 19)\n");
    }

    #[test]
    fn no_offense_missing_value_anon() {
        test::<WhereEquals>().expect_no_offenses("User.where('name = ?', )\n");
    }

    #[test]
    fn no_offense_not_without_where() {
        test::<WhereEquals>().expect_no_offenses("users.not('name = ?', 'Gabe')\n");
    }

    #[test]
    fn no_offense_double_qualified() {
        test::<WhereEquals>().expect_no_offenses("User.where('database.users.name = ?', 'Gabe')\n");
    }
}

murphy_plugin_api::submit_cop!(WhereEquals);
