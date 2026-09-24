//! `Lint/NestedPercentLiteral` — flag nested percent literals within percent
//! literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NestedPercentLiteral
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/NestedPercentLiteral: flags nested percent
//!   literals inside `%i`/`%I`/`%w`/`%W` (Array), `%q`/`%Q` (Str/Dstr),
//!   `%x` (Xstr), `%r` (Regexp), and `%s` (Sym). Bare `%` outer literals
//!   are intentionally ignored to preserve RuboCop's "nestings under
//!   percent" behavior. Inner detection uses the same `%<type><non-alnum>`
//!   rule as the array path, searched as a substring of the literal content,
//!   so `%q[100% sure]` (bare `%` + space) does not flag.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct NestedPercentLiteral;

const PERCENT_PREFIXES: &[&str] = &["%i", "%I", "%w", "%W", "%q", "%Q", "%x", "%r", "%s"];

const MSG: &str = "Within percent literals, nested percent literals do not function and may be unwanted in the result.";

#[cop(
    name = "Lint/NestedPercentLiteral",
    description = "Flag nested percent literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NestedPercentLiteral {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !PERCENT_PREFIXES.iter().any(|p| src.starts_with(p)) {
            return;
        }
        let elements = cx.array_elements(node);
        for &child in elements {
            let child_src = cx.raw_source(cx.range(child));
            if child_src.len() >= 3
                && child_src
                    .chars()
                    .nth(2)
                    .is_some_and(|c| !c.is_alphanumeric())
                && PERCENT_PREFIXES.iter().any(|p| child_src.starts_with(p))
            {
                cx.emit_offense(cx.range(node), MSG, None);
                return;
            }
        }
    }

    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip parts of xstr/regexp — those inherit the outer range and are
        // checked by their own handlers.
        if cx
            .parent(node)
            .get()
            .is_some_and(|p| matches!(cx.kind(p), NodeKind::Regexp { .. } | NodeKind::Xstr(_)))
        {
            return;
        }
        let src = cx.raw_source(cx.range(node));
        if !(src.starts_with("%q") || src.starts_with("%Q")) {
            return;
        }
        let NodeKind::Str(id) = *cx.kind(node) else {
            return;
        };
        if contains_nested_percent_literal(cx.string_str(id)) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip dstr parts inside regexp — they are parts, not top-level.
        if cx
            .parent(node)
            .get()
            .is_some_and(|p| matches!(cx.kind(p), NodeKind::Regexp { .. }))
        {
            return;
        }
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with("%Q") {
            return;
        }
        let NodeKind::Dstr(list) = *cx.kind(node) else {
            return;
        };
        for &child in cx.list(list) {
            if let NodeKind::Str(id) = *cx.kind(child)
                && contains_nested_percent_literal(cx.string_str(id))
            {
                cx.emit_offense(cx.range(node), MSG, None);
                return;
            }
        }
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with("%x") {
            return;
        }
        let NodeKind::Xstr(list) = *cx.kind(node) else {
            return;
        };
        for &child in cx.list(list) {
            if let NodeKind::Str(id) = *cx.kind(child)
                && contains_nested_percent_literal(cx.string_str(id))
            {
                cx.emit_offense(cx.range(node), MSG, None);
                return;
            }
        }
    }

    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with("%r") {
            return;
        }
        let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
            return;
        };
        for &child in cx.list(parts) {
            if let NodeKind::Str(id) = *cx.kind(child)
                && contains_nested_percent_literal(cx.string_str(id))
            {
                cx.emit_offense(cx.range(node), MSG, None);
                return;
            }
        }
    }

    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with("%s") {
            return;
        }
        let NodeKind::Sym(sym) = *cx.kind(node) else {
            return;
        };
        if contains_nested_percent_literal(cx.symbol_str(sym)) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

/// Searches `text` for a nested percent-literal opener (`%i`, `%I`, `%w`,
/// `%W`, `%q`, `%Q`, `%x`, `%r`, `%s`) followed by a non-alphanumeric
/// delimiter, anywhere in the string.
///
/// Mirrors the array path's `starts_with` + third-char rule, but as a
/// substring search since non-array content is a single string (e.g.
/// `%q[foo %q[bar]]` nests in the middle). Bare `%` is excluded on purpose:
/// `%q[100% sure]` contains `%` + space but no lettered prefix, so it does
/// not flag.
fn contains_nested_percent_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        if !matches!(
            bytes[i + 1],
            b'i' | b'I' | b'w' | b'W' | b'q' | b'Q' | b'x' | b'r' | b's'
        ) {
            i += 1;
            continue;
        }
        if i + 2 >= bytes.len() {
            i += 1;
            continue;
        }
        // `i` and `i+1` are ASCII, so `i+2` is a char boundary.
        if let Some(c) = text[i + 2..].chars().next()
            && !c.is_alphanumeric()
        {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::NestedPercentLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_nested_percent_i_within_percent_i() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %i[%i[a b]]
            ^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_i() {
        test::<NestedPercentLiteral>().expect_no_offenses("%i[a b]\n");
    }

    #[test]
    fn ignores_regular_array() {
        test::<NestedPercentLiteral>().expect_no_offenses("[:foo, :bar]\n");
    }

    #[test]
    fn flags_nested_percent_w() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %w[%w[a b]]
            ^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_w() {
        test::<NestedPercentLiteral>().expect_no_offenses("%w[a b]\n");
    }

    #[test]
    fn flags_nested_percent_q() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %q[foo %q[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_q() {
        test::<NestedPercentLiteral>().expect_no_offenses("%q[foo]\n");
    }

    #[test]
    fn ignores_plain_string_with_percent_text() {
        test::<NestedPercentLiteral>().expect_no_offenses("\"foo %q[bar]\"\n");
    }

    #[test]
    fn flags_nested_percent_upper_q_without_interpolation() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %Q[foo %Q[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn flags_nested_percent_upper_q_with_interpolation() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %Q[foo #{x} %Q[bar]]
            ^^^^^^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_upper_q() {
        test::<NestedPercentLiteral>().expect_no_offenses("%Q[foo]\n");
    }

    #[test]
    fn ignores_flat_percent_upper_q_with_interpolation() {
        test::<NestedPercentLiteral>().expect_no_offenses("%Q[foo #{x}]\n");
    }

    #[test]
    fn ignores_interpolated_inner_literal() {
        test::<NestedPercentLiteral>().expect_no_offenses("%Q[foo #{%q[bar]}]\n");
    }

    #[test]
    fn flags_nested_percent_x() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %x[foo %x[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_x() {
        test::<NestedPercentLiteral>().expect_no_offenses("%x[ls]\n");
    }

    #[test]
    fn ignores_backtick_with_percent_text() {
        test::<NestedPercentLiteral>().expect_no_offenses("`foo %x[bar]`\n");
    }

    #[test]
    fn flags_nested_percent_r() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %r[foo %r[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_r() {
        test::<NestedPercentLiteral>().expect_no_offenses("%r[foo]\n");
    }

    #[test]
    fn ignores_slash_regexp_with_percent_text() {
        test::<NestedPercentLiteral>().expect_no_offenses("/foo %r[bar]/\n");
    }

    #[test]
    fn flags_nested_percent_s() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %s[foo %s[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_s() {
        test::<NestedPercentLiteral>().expect_no_offenses("%s[foo]\n");
    }

    #[test]
    fn ignores_plain_sym_with_percent_text() {
        test::<NestedPercentLiteral>().expect_no_offenses(":foo\n");
    }

    #[test]
    fn flags_cross_type_nesting() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %q[foo %w[bar]]
            ^^^^^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_percent_sign_without_nested_literal() {
        test::<NestedPercentLiteral>().expect_no_offenses("%q[100% sure]\n");
    }
}
murphy_plugin_api::submit_cop!(NestedPercentLiteral);
