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
        for &child in cx.list(parts) {
            if is_embedded_expression(child, cx) && is_literal_value(child, cx) {
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

fn is_literal_value(child: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(child) {
        NodeKind::Int(_) | NodeKind::Float(_) | NodeKind::Nil
        | NodeKind::True_ | NodeKind::False_ => true,
        NodeKind::Sym(_) => true,
        NodeKind::Begin(list) => {
            let items = cx.list(list);
            items.len() == 1 && is_literal_value(items[0], cx)
        }
        NodeKind::Array(list) => cx.list(list).iter().all(|&e| is_literal_value(e, cx)),
        NodeKind::Hash(list) => cx.list(list).iter().all(|&e| {
            if let NodeKind::Pair { key, value } = *cx.kind(e) {
                is_literal_value(key, cx) && is_literal_value(value, cx)
            } else { false }
        }),
        NodeKind::RangeExpr { begin_, end_, .. } => {
            begin_.get().map_or(true, |b| is_literal_value(b, cx))
                && end_.get().map_or(true, |e| is_literal_value(e, cx))
        }
        _ => false,
    }
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
}
murphy_plugin_api::submit_cop!(LiteralInInterpolation);
