//! `Rails/OrderArguments` — prefer symbol arguments over strings in `order`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/OrderArguments
//! upstream_version_checked: 2.35.0
//! version_added: "2.33"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:order] (send + csend),
//!   `(str)+` gate (all args must be plain Str), comma-splitting of each
//!   string, ORDER_EXPRESSION_REGEX /\A(\w+) ?(asc|desc)?\z/i extraction,
//!   positional-column (`\A\d+\z`) suppression, downcased column names, and
//!   the use_hash conversion (`:col` while leading ASC, `col: :dir`
//!   afterwards). Offense is first-arg-start to last-arg-end; autocorrect
//!   replaces it with the preferred form. Upstream Include gating absent.
//! ```
//!
//! Prefer symbol arguments over strings in `order` method.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct OrderArguments;

#[cop(
    name = "Rails/OrderArguments",
    description = "Prefer symbol arguments over strings in `order` method.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl OrderArguments {
    #[on_node(kind = "send", methods = ["order"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("order") {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    if args.is_empty() {
        return;
    }
    // Upstream `(str $_value)+` — every argument must be a plain Str.
    let mut exprs: Vec<String> = Vec::with_capacity(args.len());
    for &a in &args {
        let NodeKind::Str(sid) = *cx.kind(a) else {
            return;
        };
        exprs.push(cx.string_str(sid).to_owned());
    }
    let Some(preferred) = replacement(&exprs) else {
        return;
    };
    let first_range = cx.range(args[0]);
    let last_range = cx.range(args[args.len() - 1]);
    let offense = murphy_plugin_api::Range {
        start: first_range.start,
        end: last_range.end,
    };
    cx.emit_offense(
        offense,
        &format!("Prefer `{preferred}` instead."),
        None,
    );
    cx.emit_edit(offense, &preferred);
}

fn replacement(exprs: &[String]) -> Option<String> {
    // Flat-map on commas, mirroring `flat_map { |expr| expr.split(',') }`.
    let mut parts: Vec<String> = Vec::new();
    for e in exprs {
        for p in e.split(',') {
            parts.push(p.trim().to_owned());
        }
    }
    let mut cols: Vec<(String, Direction)> = Vec::with_capacity(parts.len());
    for p in &parts {
        let (col, dir) = extract_column_and_direction(p)?;
        if is_positional_column(&col) {
            return None;
        }
        cols.push((col, dir));
    }
    Some(convert_to_preferred(&cols))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Asc,
    Desc,
}

fn extract_column_and_direction(expr: &str) -> Option<(String, Direction)> {
    // ORDER_EXPRESSION_REGEX = /\A(\w+) ?(asc|desc)?\z/i
    // `\w` = [A-Za-z0-9_]. Single optional ASCII space.
    if expr.is_empty() {
        return None;
    }
    // Split off trailing direction if present.
    // Try: whole is word chars only.
    if is_word(expr) {
        return Some((expr.to_ascii_lowercase(), Direction::Asc));
    }
    // Try: word + single space + asc/desc (case-insensitive).
    if let Some(space) = expr.find(' ') {
        let (col, rest) = expr.split_at(space);
        // ` ?` — exactly one space.
        if !rest.starts_with(' ') || rest.starts_with("  ") {
            return None;
        }
        let dir_str = &rest[1..];
        if dir_str.contains(' ') || dir_str.contains('\t') {
            return None;
        }
        if !is_word(col) {
            return None;
        }
        let dir_lower = dir_str.to_ascii_lowercase();
        let dir = match dir_lower.as_str() {
            "asc" => Direction::Asc,
            "desc" => Direction::Desc,
            _ => return None,
        };
        return Some((col.to_ascii_lowercase(), dir));
    }
    None
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_positional_column(col: &str) -> bool {
    !col.is_empty() && col.bytes().all(|b| b.is_ascii_digit())
}

fn convert_to_preferred(cols: &[(String, Direction)]) -> String {
    let mut use_hash = false;
    let mut out: Vec<String> = Vec::with_capacity(cols.len());
    for (col, dir) in cols {
        match dir {
            Direction::Asc if !use_hash => out.push(format!(":{col}")),
            _ => {
                use_hash = true;
                let d = match dir {
                    Direction::Asc => "asc",
                    Direction::Desc => "desc",
                };
                out.push(format!("{col}: :{d}"));
            }
        }
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::OrderArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_string() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('first_name')
                           ^^^^^^^^^^^^ Prefer `:first_name` instead.
            "#},
            "User.order(:first_name)\n",
        );
    }

    #[test]
    fn flags_multiple_strings() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('first_name', 'last_name')
                           ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `:first_name, :last_name` instead.
            "#},
            "User.order(:first_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_desc_direction() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('name DESC')
                           ^^^^^^^^^^^ Prefer `name: :desc` instead.
            "#},
            "User.order(name: :desc)\n",
        );
    }

    #[test]
    fn flags_asc_direction_as_symbol() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('name ASC')
                           ^^^^^^^^^^ Prefer `:name` instead.
            "#},
            "User.order(:name)\n",
        );
    }

    #[test]
    fn flags_desc_then_implicit_asc() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('first_name DESC, last_name')
                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `first_name: :desc, last_name: :asc` instead.
            "#},
            "User.order(first_name: :desc, last_name: :asc)\n",
        );
    }

    #[test]
    fn flags_explicit_asc_then_implicit() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('first_name ASC, last_name')
                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `:first_name, :last_name` instead.
            "#},
            "User.order(:first_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_csend() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User&.order('first_name')
                            ^^^^^^^^^^^^ Prefer `:first_name` instead.
            "#},
            "User&.order(:first_name)\n",
        );
    }

    #[test]
    fn flags_comma_inside_single_string() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('first_name, middle_name', 'last_name')
                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `:first_name, :middle_name, :last_name` instead.
            "#},
            "User.order(:first_name, :middle_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_lowercase_direction() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('name desc')
                           ^^^^^^^^^^^ Prefer `name: :desc` instead.
            "#},
            "User.order(name: :desc)\n",
        );
    }

    #[test]
    fn flags_uppercase_column() {
        test::<OrderArguments>().expect_correction(
            indoc! {r#"
                User.order('NAME DESC')
                           ^^^^^^^^^^^ Prefer `name: :desc` instead.
            "#},
            "User.order(name: :desc)\n",
        );
    }

    #[test]
    fn allows_symbol_argument() {
        test::<OrderArguments>().expect_no_offenses("User.order(:first_name)\n");
    }

    #[test]
    fn allows_mixed_symbol_and_string() {
        test::<OrderArguments>().expect_no_offenses("User.order(:first_name, 'last_name')\n");
    }

    #[test]
    fn allows_expression() {
        test::<OrderArguments>().expect_no_offenses("User.order('LEFT(first_name, 1)')\n");
    }

    #[test]
    fn allows_numeric_column() {
        test::<OrderArguments>().expect_no_offenses("User.order('1')\n");
    }

    #[test]
    fn allows_numeric_with_other() {
        test::<OrderArguments>().expect_no_offenses("User.order('1', 'last_name')\n");
    }

    #[test]
    fn allows_numeric_desc() {
        test::<OrderArguments>().expect_no_offenses("User.order('1 DESC')\n");
    }

    #[test]
    fn allows_numeric_asc() {
        test::<OrderArguments>().expect_no_offenses("User.order('1 ASC')\n");
    }
}
murphy_plugin_api::submit_cop!(OrderArguments);
