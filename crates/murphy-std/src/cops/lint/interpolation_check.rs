//! `Lint/InterpolationCheck` — detect interpolation in single-quoted
//! strings.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/InterpolationCheck
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/InterpolationCheck cop: single-quoted strings
//!   containing `#{...}` are flagged and autocorrected to double-quoted
//!   strings (or `%{...}` when the content contains `"`).

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

#[derive(Default)]
pub struct InterpolationCheck;

#[cop(
    name = "Lint/InterpolationCheck",
    description = "Check for interpolation in single-quoted strings.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl InterpolationCheck {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Str(_) = *cx.kind(node) else { return; };
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with('\'') {
            return;
        }
        if !contains_interpolation_like(src) {
            return;
        }
        if is_in_regexp_or_heredoc(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Interpolation in single quoted string detected. Use double quoted strings if you need interpolation.",
            None,
        );
        let content = &src[1..src.len() - 1];
        if content.contains('"') {
            cx.emit_edit(cx.range(node), &format!("%{{{content}}}"));
        } else {
            cx.emit_edit(cx.range(node), &format!("\"{content}\""));
        }
    }
}

fn contains_interpolation_like(src: &str) -> bool {
    let inner = &src[1..src.len() - 1];
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if c == '#' && inner[i..].starts_with("#{") {
            return true;
        }
    }
    false
}

fn is_in_regexp_or_heredoc(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent_id) = cx.parent(node).get() else { return false; };
    if matches!(*cx.kind(parent_id), NodeKind::Regexp { .. }) {
        return true;
    }
    if matches!(*cx.kind(parent_id), NodeKind::Dstr(_)) {
        if let Some(gp) = cx.parent(parent_id).get() {
            let src = cx.raw_source(cx.range(gp));
            return src.contains("<<~") || src.contains("<<-") || src.contains("<<\"");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::InterpolationCheck;

    fn hits(src: &str) -> usize {
        use murphy_plugin_api::test_support::run_cop;
        run_cop::<InterpolationCheck>(src).len()
    }

    #[test]
    fn flags_interpolation_in_single_quoted_string() {
        assert!(hits("'foo #{bar}'") > 0);
    }

    #[test]
    fn ignores_double_quoted_string() {
        assert_eq!(hits("\"foo #{bar}\""), 0);
    }

    #[test]
    fn ignores_plain_string() {
        assert_eq!(hits("'foo'"), 0);
    }

    #[test]
    fn handles_string_with_embedded_double_quotes() {
        assert!(hits("'foo \"#{bar}\" baz'") > 0);
    }

    #[test]
    fn ignores_escaped_interpolation() {
        assert_eq!(hits("'foo \\#{bar}'"), 0);
    }

    #[test]
    fn ignores_regexp_literal() {
        assert_eq!(hits("/\\#{20}/"), 0);
    }
}
murphy_plugin_api::submit_cop!(InterpolationCheck);
