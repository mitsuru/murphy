//! `Style/HashConversion` — avoid `Hash[]` in favor of `ary.to_h` or literal hashes.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashConversion
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (Enabled: pending in RuboCop, matches bundled default.yml).
//!   Autocorrect is unsafe in RuboCop (ArgumentError on odd-element arrays); Murphy
//!   has no Safe/SafeAutoCorrect attribute yet — documented here only.
//!
//!   Handled patterns (all triggered on `Hash[...]`):
//!     1. Single argument, hash literal: `Hash[foo: 1]` → `{foo: 1}`
//!        Message: MSG_LITERAL_HASH_ARG
//!     2. Single argument, splat: `Hash[*ary]`
//!        - AllowSplatArgument true (default): no offense
//!        - AllowSplatArgument false: offense (MSG_SPLAT), no autocorrect
//!     3. Single argument, zip method with no args: `Hash[a.zip(b)]`
//!        NOTE: zip-with-args case treated as generic single-arg path.
//!        zip-without-args: offense (MSG_TO_H), autocorrect uses whole-node
//!        replacement because the structural rewrite is non-trivial.
//!     4. Single argument, other: `Hash[ary]` → `ary.to_h`
//!        Message: MSG_TO_H. Wraps arg in parens if requires_parens.
//!     5. Zero or 2+ arguments, odd count: offense (MSG_LITERAL_MULTI_ARG), no autocorrect.
//!     6. Even count 2+ args: `Hash[k1, v1, k2, v2]` → `{k1 => v1, k2 => v2}`
//!        Message: MSG_LITERAL_MULTI_ARG.
//!
//!   Nested `Hash[Hash[...]]`: RuboCop's `ignore_node` prevents double-flagging;
//!   Murphy fires per-node so nested forms produce multiple offenses. Known gap.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! Hash[ary]
//! Hash[key1, value1, key2, value2]
//! Hash[foo: 1]
//!
//! # good
//! ary.to_h
//! {key1 => value1, key2 => value2}
//! {foo: 1}
//!
//! # good (AllowSplatArgument: true, default)
//! Hash[*ary]
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashConversion;

#[derive(CopOptions)]
pub struct HashConversionOptions {
    #[option(
        name = "AllowSplatArgument",
        default = true,
        description = "Allow splat argument in Hash[] (true by default because replacement is complex)."
    )]
    pub allow_splat_argument: bool,
}

const MSG_TO_H: &str = "Prefer `ary.to_h` to `Hash[ary]`.";
const MSG_LITERAL_MULTI_ARG: &str = "Prefer literal hash to `Hash[arg1, arg2, ...]`.";
const MSG_LITERAL_HASH_ARG: &str = "Prefer literal hash to `Hash[key: value, ...]`.";
const MSG_SPLAT: &str = "Prefer `array_of_pairs.to_h` to `Hash[*array]`.";

#[cop(
    name = "Style/HashConversion",
    description = "Avoid Hash[] in favor of ary.to_h or literal hashes.",
    default_severity = "warning",
    default_enabled = false,
    options = HashConversionOptions,
)]
impl HashConversion {
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Receiver must be `Hash` constant with nil or cbase scope.
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    if !is_hash_const(recv_id, cx) {
        return;
    }

    let arg_list = cx.call_arguments(node);
    let opts = cx.options_or_default::<HashConversionOptions>();

    if arg_list.len() == 1 {
        single_argument(node, arg_list[0], cx, &opts);
    } else {
        multi_argument(node, arg_list, cx);
    }
}

fn single_argument(node: NodeId, arg: NodeId, cx: &Cx<'_>, opts: &HashConversionOptions) {
    let node_range = cx.range(node);

    if matches!(cx.kind(arg), NodeKind::Hash { .. }) {
        // Hash literal argument: Hash[foo: 1] → {foo: 1}
        let arg_src = cx.raw_source(cx.range(arg)).to_owned();
        cx.emit_offense(node_range, MSG_LITERAL_HASH_ARG, None);
        cx.emit_edit(node_range, &format!("{{{arg_src}}}"));
    } else if matches!(cx.kind(arg), NodeKind::Splat(_)) {
        // Splat argument: Hash[*ary]
        if !opts.allow_splat_argument {
            // No autocorrect for splat — replacement is complex.
            cx.emit_offense(node_range, MSG_SPLAT, None);
        }
    } else if cx.method_name(arg) == Some("zip") && cx.call_arguments(arg).is_empty() {
        // zip method with no arguments: Hash[a.zip] → a.zip([]).to_h
        register_offense_for_zip(node, arg, cx);
    } else {
        // Generic single arg: Hash[ary] → ary.to_h
        let arg_src = cx.raw_source(cx.range(arg)).to_owned();
        let replacement = if requires_parens(arg, cx) {
            format!("({arg_src}).to_h")
        } else {
            format!("{arg_src}.to_h")
        };
        cx.emit_offense(node_range, MSG_TO_H, None);
        cx.emit_edit(node_range, &replacement);
    }
}

/// `Hash[a.zip]` (zip with no args) → `a.zip([]).to_h`.
/// Handles both parenthesized (`zip()`) and bare (`zip`) forms.
fn register_offense_for_zip(outer: NodeId, zip_node: NodeId, cx: &Cx<'_>) {
    let node_range = cx.range(outer);
    cx.emit_offense(node_range, MSG_TO_H, None);

    // Detect whether the zip call is parenthesized by looking for a `(`
    // token right after the zip method name.
    let zip_name = cx.node(zip_node).loc.name;
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < zip_name.end);
    let zip_end = cx.range(zip_node).end;

    let has_parens = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < zip_end)
        .any(|t| t.kind == SourceTokenKind::LeftParen);

    // Whole-node replacement — the structural rewrite shuffles AST significantly.
    let mut zip_src = cx.raw_source(cx.range(zip_node)).to_owned();
    if has_parens {
        // `a.zip()` → `a.zip([]).to_h` — remove the trailing `)`, append `([]).to_h`.
        zip_src.pop(); // removes the closing `)`
        let replacement = format!("{zip_src}[]).to_h");
        cx.emit_edit(node_range, &replacement);
    } else {
        // `a.zip` → `a.zip([]).to_h`
        let replacement = format!("{zip_src}([]).to_h");
        cx.emit_edit(node_range, &replacement);
    }
}

/// True when the argument node requires parentheses when used as receiver with `.to_h`.
///
/// Mirrors RuboCop's `requires_parens?`:
/// - call node with unparenthesized arguments (but NOT the `[]` method)
/// - operator keyword nodes (and, or, not)
fn requires_parens(node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(node), NodeKind::Send { .. }) {
        if cx.method_name(node) == Some("[]") {
            return false;
        }
        !cx.call_arguments(node).is_empty() && !cx.is_parenthesized(node)
    } else {
        matches!(
            cx.kind(node),
            NodeKind::And { .. } | NodeKind::Or { .. } | NodeKind::Not { .. }
        )
    }
}

fn multi_argument(node: NodeId, arg_list: &[NodeId], cx: &Cx<'_>) {
    let node_range = cx.range(node);

    if arg_list.len() % 2 != 0 {
        // Odd count is a bug — offense but no autocorrect.
        cx.emit_offense(node_range, MSG_LITERAL_MULTI_ARG, None);
    } else {
        let content: Vec<String> = arg_list
            .chunks(2)
            .map(|pair| {
                let k = cx.raw_source(cx.range(pair[0])).to_owned();
                let v = cx.raw_source(cx.range(pair[1])).to_owned();
                format!("{k} => {v}")
            })
            .collect();
        let replacement = format!("{{{}}}", content.join(", "));
        cx.emit_offense(node_range, MSG_LITERAL_MULTI_ARG, None);
        cx.emit_edit(node_range, &replacement);
    }
}

/// Returns true if `node` is a `Const` with name `Hash` and nil or cbase scope.
fn is_hash_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "Hash" {
        return false;
    }
    if let Some(scope_id) = scope.get() {
        if !matches!(cx.kind(scope_id), NodeKind::Cbase) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Single arg, generic: Hash[ary] → ary.to_h -----

    #[test]
    fn flags_hash_bracket_single_arg() {
        test::<HashConversion>().expect_offense(indoc! {r#"
            Hash[ary]
            ^^^^^^^^^ Prefer `ary.to_h` to `Hash[ary]`.
        "#});
    }

    #[test]
    fn corrects_hash_bracket_single_arg() {
        test::<HashConversion>().expect_correction(
            indoc! {r#"
                Hash[ary]
                ^^^^^^^^^ Prefer `ary.to_h` to `Hash[ary]`.
            "#},
            "ary.to_h\n",
        );
    }

    // ----- Single arg, hash literal: Hash[foo: 1] → {foo: 1} -----

    #[test]
    fn flags_hash_bracket_hash_arg() {
        test::<HashConversion>().expect_offense(indoc! {r#"
            Hash[foo: 1]
            ^^^^^^^^^^^^ Prefer literal hash to `Hash[key: value, ...]`.
        "#});
    }

    #[test]
    fn corrects_hash_bracket_hash_arg() {
        test::<HashConversion>().expect_correction(
            indoc! {r#"
                Hash[foo: 1]
                ^^^^^^^^^^^^ Prefer literal hash to `Hash[key: value, ...]`.
            "#},
            "{foo: 1}\n",
        );
    }

    // ----- Single arg, splat -----

    #[test]
    fn allows_splat_by_default() {
        test::<HashConversion>().expect_no_offenses("Hash[*ary]\n");
    }

    #[test]
    fn flags_splat_when_not_allowed() {
        test::<HashConversion>()
            .with_options(&HashConversionOptions {
                allow_splat_argument: false,
            })
            .expect_offense(indoc! {r#"
                Hash[*ary]
                ^^^^^^^^^^ Prefer `array_of_pairs.to_h` to `Hash[*array]`.
            "#});
    }

    // ----- Multi-arg, even count -----

    #[test]
    fn flags_hash_multi_arg_even() {
        test::<HashConversion>().expect_offense(indoc! {r#"
            Hash[key1, value1, key2, value2]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer literal hash to `Hash[arg1, arg2, ...]`.
        "#});
    }

    #[test]
    fn corrects_hash_multi_arg_even() {
        test::<HashConversion>().expect_correction(
            indoc! {r#"
                Hash[key1, value1, key2, value2]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer literal hash to `Hash[arg1, arg2, ...]`.
            "#},
            "{key1 => value1, key2 => value2}\n",
        );
    }

    // ----- Multi-arg, odd count — offense but no autocorrect -----

    #[test]
    fn flags_hash_multi_arg_odd() {
        test::<HashConversion>().expect_offense(indoc! {r#"
            Hash[key1, value1, key2]
            ^^^^^^^^^^^^^^^^^^^^^^^^ Prefer literal hash to `Hash[arg1, arg2, ...]`.
        "#});
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_non_hash_const() {
        test::<HashConversion>().expect_no_offenses("MyHash[ary]\n");
    }

    #[test]
    fn accepts_namespaced_hash() {
        test::<HashConversion>().expect_no_offenses("Foo::Hash[ary]\n");
    }

    // ----- ::Hash prefix -----

    #[test]
    fn flags_cbase_hash() {
        test::<HashConversion>().expect_offense(indoc! {r#"
            ::Hash[ary]
            ^^^^^^^^^^^ Prefer `ary.to_h` to `Hash[ary]`.
        "#});
    }

    #[test]
    fn corrects_cbase_hash() {
        test::<HashConversion>().expect_correction(
            indoc! {r#"
                ::Hash[ary]
                ^^^^^^^^^^^ Prefer `ary.to_h` to `Hash[ary]`.
            "#},
            "ary.to_h\n",
        );
    }

    // ----- requires_parens: call with unparenthesized args -----

    #[test]
    fn corrects_single_arg_unparenthesized_call_wraps_parens() {
        test::<HashConversion>().expect_correction(
            indoc! {r#"
                Hash[foo :bar]
                ^^^^^^^^^^^^^^ Prefer `ary.to_h` to `Hash[ary]`.
            "#},
            "(foo :bar).to_h\n",
        );
    }
}

murphy_plugin_api::submit_cop!(HashConversion);
