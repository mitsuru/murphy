//! `Lint/BooleanSymbol` — flags `:true` and `:false` symbols.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/BooleanSymbol
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches `(sym {:true :false})`. Autocorrection is unsafe (RuboCop's
//!   `@safety`): code relying on the symbols breaks when changed to booleans,
//!   so `safe_autocorrect = false`. Skips symbols inside `%i[...]`/`%I[...]`
//!   percent-literal symbol arrays; plain `[:true]` arrays still flag.
//!   Autocorrect strips the leading `:`. For a colon-style hash key
//!   (`{ true: 1 }`), the key is rewritten with a hash rocket
//!   (`{ true => 1 }`) since `{ true: 1 }` would otherwise parse the bare
//!   boolean as a label again. The colon-key offense range excludes the
//!   trailing `:` to match RuboCop (Murphy's sym range includes it).
//! ```
use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG_TRUE: &str = "Symbol with a boolean name - you probably meant to use `true`.";
const MSG_FALSE: &str = "Symbol with a boolean name - you probably meant to use `false`.";

#[derive(Default)]
pub struct BooleanSymbol;

#[cop(
    name = "Lint/BooleanSymbol",
    description = "Checks for `:true` and `:false` symbols.",
    default_severity = "warning",
    default_enabled = true,
    safe_autocorrect = false,
    options = NoOptions
)]
impl BooleanSymbol {
    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sym(sym) = *cx.kind(node) else {
            return;
        };
        let name = cx.symbol_str(sym);
        let message = match name {
            "true" => MSG_TRUE,
            "false" => MSG_FALSE,
            _ => return,
        };

        // Skip symbols inside `%i[...]` / `%I[...]` percent-literal arrays.
        if in_percent_symbol_array(node, cx) {
            return;
        }

        let src = cx.raw_source(cx.range(node));
        if is_colon_hash_key(node, cx) {
            // `{ true: 1 }`: Murphy's sym range includes the trailing `:`, but
            // RuboCop highlights only the word (the `:` is the pair operator),
            // so trim the colon from the offense range.
            let r = cx.range(node);
            let offense_range = Range { start: r.start, end: r.end.saturating_sub(1) };
            cx.emit_offense(offense_range, message, None);
            // `{ true: 1 }` → `{ true => 1 }`: rewrite the whole key (incl. `:`)
            // to `<boolean> =>`.
            cx.emit_edit(cx.range(node), &format!("{name} =>"));
        } else {
            cx.emit_offense(cx.range(node), message, None);
            // `:true` → `true`: strip the leading `:`.
            cx.emit_edit(cx.range(node), src.trim_start_matches(':'));
        }
    }
}

/// True when `node` is a hash-pair key written with colon syntax
/// (`{ true: 1 }`), as opposed to a hash-rocket key (`{ :true => 1 }`) or a
/// standalone symbol. Murphy's AST gives a colon-key sym a source range that
/// includes the trailing `:`.
fn is_colon_hash_key(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(cx.kind(parent), NodeKind::Pair { key, .. } if *key == node) {
        return false;
    }
    cx.raw_source(cx.range(node)).ends_with(':')
}

/// True when `node` lives inside a percent-literal symbol array
/// (`%i[...]` / `%I[...]`). Only `%i`/`%I` can contain `sym` children, so
/// `is_percent_literal` is equivalent to RuboCop's `percent_literal?(:symbol)`
/// here. Plain `[:true]` arrays are not percent literals and still flag.
fn in_percent_symbol_array(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| cx.is_percent_literal(a))
}

murphy_plugin_api::submit_cop!(BooleanSymbol);

#[cfg(test)]
mod tests {
    use super::BooleanSymbol;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_true_symbol() {
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                :true
                ^^^^^ Symbol with a boolean name - you probably meant to use `true`.
            "#},
            "true\n",
        );
    }

    #[test]
    fn flags_false_symbol() {
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                :false
                ^^^^^^ Symbol with a boolean name - you probably meant to use `false`.
            "#},
            "false\n",
        );
    }

    #[test]
    fn flags_colon_hash_key_with_hash_rocket_correction() {
        // RuboCop highlights only the word, excluding the trailing `:`.
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                { true: 'Foo' }
                  ^^^^ Symbol with a boolean name - you probably meant to use `true`.
            "#},
            "{ true => 'Foo' }\n",
        );
    }

    #[test]
    fn flags_false_colon_hash_key() {
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                { false: :bar }
                  ^^^^^ Symbol with a boolean name - you probably meant to use `false`.
            "#},
            "{ false => :bar }\n",
        );
    }

    #[test]
    fn flags_hash_rocket_symbol_key() {
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                { :false => 42 }
                  ^^^^^^ Symbol with a boolean name - you probably meant to use `false`.
            "#},
            "{ false => 42 }\n",
        );
    }

    #[test]
    fn flags_label_key_and_rocket_value_in_one_hash() {
        // Spec case 6: both the label key and the rocket-value symbol flag.
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                { false: :false }
                  ^^^^^ Symbol with a boolean name - you probably meant to use `false`.
                         ^^^^^^ Symbol with a boolean name - you probably meant to use `false`.
            "#},
            "{ false => false }\n",
        );
    }

    #[test]
    fn flags_symbol_in_plain_array() {
        test::<BooleanSymbol>().expect_correction(
            indoc! {r#"
                [:true, :false]
                 ^^^^^ Symbol with a boolean name - you probably meant to use `true`.
                        ^^^^^^ Symbol with a boolean name - you probably meant to use `false`.
            "#},
            "[true, false]\n",
        );
    }

    #[test]
    fn ignores_percent_i_array() {
        test::<BooleanSymbol>().expect_no_offenses("%i[true false]\n");
    }

    #[test]
    fn ignores_percent_cap_i_array() {
        test::<BooleanSymbol>().expect_no_offenses("%I[true false]\n");
    }

    #[test]
    fn ignores_non_boolean_symbols() {
        test::<BooleanSymbol>().expect_no_offenses(":truthy\n:falsey\n:foo\n");
    }

    #[test]
    fn ignores_actual_booleans() {
        test::<BooleanSymbol>().expect_no_offenses("true\nfalse\n");
    }
}
