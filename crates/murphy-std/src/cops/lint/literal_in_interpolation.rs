//! `Lint/LiteralInInterpolation` — flag literal values in
//! interpolation.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/LiteralInInterpolation
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/LiteralInInterpolation cop: literal values
//!   (integers, floats, strings, symbols, nil, arrays, hashes, ranges)
//!   inside `#{}` interpolation in double-quoted strings are flagged.
//!   Autocorrect is not yet implemented (v1 gap: requires source
//!   reconstruction of the literal value).

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

#[derive(Default)]
pub struct LiteralInInterpolation;

#[cop(
    name = "Lint/LiteralInInterpolation",
    description = "Flag literal values in interpolation.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LiteralInInterpolation {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dstr(parts) = *cx.kind(node) else { return; };
        // A heredoc dstr's raw source begins with the `<<` opener.
        let is_heredoc = cx.raw_source(cx.range(node)).starts_with("<<");
        for &child in cx.list(parts) {
            if is_embedded_expression(child, cx) && is_literal_value(child, cx) {
                // RuboCop keeps a whitespace-only `#{' '}` that ends a heredoc
                // line: it deliberately preserves trailing whitespace that
                // `Layout/TrailingWhitespace` would otherwise strip.
                if is_heredoc
                    && is_blank_string_interpolation(child, cx)
                    && ends_physical_line(child, cx)
                {
                    continue;
                }
                cx.emit_offense(
                    cx.range(child),
                    "Literal interpolation detected.",
                    None,
                );
            }
        }
    }
}

fn is_embedded_expression(child: NodeId, cx: &Cx<'_>) -> bool {
    !matches!(*cx.kind(child), NodeKind::Str(_))
}

/// RuboCop's `prints_as_self?`: a basic immutable literal, or a composite
/// (array/hash/pair/range, plus the interpolation's own `Begin` wrapper) whose
/// children all print as themselves. Deliberately narrower than
/// `cx.is_recursive_literal`, which also treats operator calls like `'a' * 30`
/// and `1 == 2` as literals — RuboCop's `Lint/LiteralInInterpolation` does not
/// flag those (they are `send` nodes, not literals).
fn is_literal_value(child: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(child) {
        NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Rational(_)
        | NodeKind::Complex(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::Nil => true,
        NodeKind::Array(_)
        | NodeKind::Hash(_)
        | NodeKind::Pair { .. }
        | NodeKind::RangeExpr { .. }
        | NodeKind::Begin(_) => cx.children(child).iter().all(|&c| is_literal_value(c, cx)),
        _ => false,
    }
}

/// `#{...}` whose entire content is a single whitespace-only (or empty) string
/// literal — RuboCop's `space_literal?`.
fn is_blank_string_interpolation(child: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Begin(list) = *cx.kind(child) else {
        return false;
    };
    match cx.list(list) {
        [only] => match *cx.kind(*only) {
            NodeKind::Str(id) => cx.string_str(id).chars().all(char::is_whitespace),
            _ => false,
        },
        _ => false,
    }
}

/// True when the interpolation ends its physical source line (the closing `}`
/// is the last non-newline byte on the line) — RuboCop's `ends_heredoc_line?`.
fn ends_physical_line(child: NodeId, cx: &Cx<'_>) -> bool {
    let bytes = cx.source().as_bytes();
    let mut i = cx.range(child).end as usize;
    // Skip the interpolation's closing `}` if it is not part of `child`'s range.
    if bytes.get(i) == Some(&b'}') {
        i += 1;
    }
    i >= bytes.len() || bytes[i] == b'\n'
}

#[cfg(test)]
mod tests {
    use super::LiteralInInterpolation;

    fn check(src: &str) -> usize {
        use murphy_plugin_api::test_support::run_cop;
        run_cop::<LiteralInInterpolation>(src).len()
    }

    #[test]
    fn flags_integer_in_interpolation() {
        assert!(check("\"result is #{10}\"") > 0);
    }

    #[test]
    fn flags_symbol_in_interpolation() {
        assert!(check("\"result is #{:foo}\"") > 0);
    }

    #[test]
    fn ignores_variable_in_interpolation() {
        assert_eq!(check("\"result is #{var}\\n\""), 0);
    }

    #[test]
    fn ignores_plain_string() {
        assert_eq!(check("\"just a string\\n\""), 0);
    }

    #[test]
    fn flags_nil_in_interpolation() {
        assert!(check("\"is #{nil} zero?\"") > 0);
    }

    #[test]
    fn ignores_blank_interpolation_at_heredoc_line_end() {
        // `#{' '}` at the end of a heredoc line preserves trailing whitespace.
        assert_eq!(check("<<~SQL\n  total / #{' '}\n  more\nSQL\n"), 0);
    }

    #[test]
    fn flags_blank_interpolation_in_regular_string() {
        // Not a heredoc: a literal `#{' '}` is still flagged.
        assert!(check("\"a#{' '}b\"") > 0);
    }

    #[test]
    fn flags_blank_interpolation_not_at_heredoc_line_end() {
        // Whitespace literal followed by more content on the line is flagged.
        assert!(check("<<~SQL\n  x#{' '}y\nSQL\n") > 0);
    }

    #[test]
    fn ignores_string_repetition_in_interpolation() {
        // `'a' * 30` is a method call (`send`), not a literal that prints as
        // itself, so RuboCop leaves it — common in spec fixtures.
        assert_eq!(check("\"#{'a' * 30}.com\""), 0);
        assert_eq!(check("\"#{'x' * 1000}\""), 0);
    }
}
murphy_plugin_api::submit_cop!(LiteralInInterpolation);
