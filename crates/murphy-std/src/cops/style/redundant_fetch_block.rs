//! `Style/RedundantFetchBlock` — replace `hash.fetch(key) { value }` with
//! `hash.fetch(key, value)` when the default value is a literal or constant.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantFetchBlock
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports the full RuboCop behavior including:
//!     - basic_literal bodies: nil, true, false, int, float, sym, rational, complex
//!     - absent body (empty block `{}`) → default value is nil
//!     - str body only when the file has `# frozen_string_literal: true`
//!     - const body only when SafeForConstants option is true (default: false)
//!     - Rails.cache receiver skip
//!     - Empty block args guard: only fires when block has no params
//!     - Only handles `block` nodes (not numblock/itblock — bodies in those
//!       cannot be basic literals)
//!
//!   Option `SafeForConstants` (default false) is exported via CopOptions and
//!   read live at dispatch time via `cx.options_or_default`, so a configured
//!   `SafeForConstants: true` flags constant default values.
//!
//!   The cop is marked unsafe in RuboCop because it cannot guarantee the receiver
//!   does not have a custom `fetch` implementation. Murphy follows the same default.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! hash.fetch(:key) { 5 }
//! hash.fetch(:key) { true }
//! hash.fetch(:key) { nil }
//! hash.fetch(:key) { :value }
//! hash.fetch(:key) {}
//! # frozen_string_literal: true
//! hash.fetch(:key) { 'value' }  # only with frozen_string_literal: true
//!
//! # good
//! hash.fetch(:key, 5)
//! hash.fetch(:key, true)
//! hash.fetch(:key, nil)
//! Rails.cache.fetch(:key) { expensive_call }
//! hash.fetch(:key) { |k| k.to_s }  # block with params
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantFetchBlock;

/// Cop options for [`RedundantFetchBlock`]. Read live at dispatch time via
/// [`Cx::options_or_default`].
#[derive(CopOptions)]
pub struct Options {
    #[option(name = "SafeForConstants", 
        default = false,
        description = "When true, also flag `fetch` blocks whose body is a constant."
    )]
    pub safe_for_constants: bool,
}

const MSG: &str = "Use `%<good>s` instead of `%<bad>s`.";

#[cop(
    name = "Style/RedundantFetchBlock",
    description = "Identifies places where `fetch(key) { value }` can be replaced by `fetch(key, value)`.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl RedundantFetchBlock {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };

    // Must be a `fetch` call (send or csend).
    if cx.method_name(call) != Some("fetch") {
        return;
    }

    // fetch must have exactly one argument (the key).
    let call_args = cx.call_arguments(call);
    if call_args.len() != 1 {
        return;
    }

    // Block must have no parameters.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    if !cx.list(args_list).is_empty() {
        return;
    }

    // Skip Rails.cache receiver.
    if let Some(recv) = cx.call_receiver(call).get()
        && is_rails_cache(recv, cx) {
            return;
        }

    // Determine if body is acceptable.
    let opts = cx.options_or_default::<Options>();
    let body_node = body.get();
    let default_value_src = match body_node {
        // Empty block {} → default is nil.
        None => "nil".to_string(),
        Some(body_id) => {
            let kind = cx.kind(body_id);
            if is_basic_literal(kind) {
                cx.raw_source(cx.range(body_id)).to_string()
            } else if matches!(kind, NodeKind::Str(_)) {
                // String bodies only fire when frozen_string_literal is enabled.
                if cx.frozen_string_literal_comment().is_none_or(|c| c.value_bool != 1) {
                    return;
                }
                cx.raw_source(cx.range(body_id)).to_string()
            } else if matches!(kind, NodeKind::Const { .. }) {
                // Constant bodies require SafeForConstants option.
                if !opts.safe_for_constants {
                    return;
                }
                cx.raw_source(cx.range(body_id)).to_string()
            } else {
                return;
            }
        }
    };

    let key_src = cx.raw_source(cx.range(call_args[0]));

    // If the key argument is a bare hash literal (e.g. `foo: :bar` without
    // braces), wrapping it in `{}` is required to keep the autocorrect valid.
    // Without braces, `fetch(foo: :bar, value)` is invalid Ruby syntax.
    // Check: key node is a Hash AND source does not start with `{`.
    let key_src_for_autocorrect = if matches!(cx.kind(call_args[0]), NodeKind::Hash(_))
        && !key_src.starts_with('{')
    {
        format!("{{{key_src}}}")
    } else {
        key_src.to_string()
    };

    // Offense range: from fetch selector start to end of block node.
    // Mirrors RuboCop's fetch_range(send, node).
    let fetch_sel_start = cx.selector(call).start;
    let offense_range = Range {
        start: fetch_sel_start,
        end: cx.range(node).end,
    };

    let good = format!("fetch({key_src_for_autocorrect}, {default_value_src})");
    let bad = if let Some(body_id) = body_node {
        let body_src = cx.raw_source(cx.range(body_id));
        format!("fetch({key_src}) {{ {body_src} }}")
    } else {
        format!("fetch({key_src}) {{}}")
    };

    let msg = MSG
        .replace("%<good>s", &good)
        .replace("%<bad>s", &bad);

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: whole-node replacement of the offense range.
    // This is a structural rewrite (removes the block, changes arg count),
    // so whole-node interpolation is appropriate per autocorrect-pattern.md.
    cx.emit_edit(offense_range, &good);
}

/// True if the node kind is a basic literal: nil, true, false, int, float,
/// sym, rational, or complex.
///
/// Note: `str` is NOT included here — it's handled separately because it
/// requires the frozen_string_literal check.
fn is_basic_literal(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Nil
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Sym(_)
            | NodeKind::Rational(_)
            | NodeKind::Complex(_)
    )
}

/// True if `node` is `Rails.cache` — the well-known Rails lazy-cache receiver
/// that should not be converted (it relies on block-lazy evaluation).
fn is_rails_cache(node: NodeId, cx: &Cx<'_>) -> bool {
    // Shape: send :cache on (const :Rails nil)
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(method) != "cache" {
        return false;
    }
    if !cx.list(args).is_empty() {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    matches!(
        *cx.kind(recv),
        NodeKind::Const { name, scope, .. }
            if cx.symbol_str(name) == "Rails" && scope.get().is_none()
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{Options, RedundantFetchBlock};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases -----

    #[test]
    fn flags_int_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { 5 }
                 ^^^^^^^^^^^^^^^^^ Use `fetch(:key, 5)` instead of `fetch(:key) { 5 }`.
        "#});
    }

    #[test]
    fn flags_true_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { true }
                 ^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, true)` instead of `fetch(:key) { true }`.
        "#});
    }

    #[test]
    fn flags_false_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { false }
                 ^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, false)` instead of `fetch(:key) { false }`.
        "#});
    }

    #[test]
    fn flags_nil_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { nil }
                 ^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, nil)` instead of `fetch(:key) { nil }`.
        "#});
    }

    #[test]
    fn flags_sym_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { :value }
                 ^^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, :value)` instead of `fetch(:key) { :value }`.
        "#});
    }

    #[test]
    fn flags_float_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { 1.5 }
                 ^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, 1.5)` instead of `fetch(:key) { 1.5 }`.
        "#});
    }

    #[test]
    fn flags_complex_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { 2i }
                 ^^^^^^^^^^^^^^^^^^ Use `fetch(:key, 2i)` instead of `fetch(:key) { 2i }`.
        "#});
    }

    #[test]
    fn flags_rational_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) { 2r }
                 ^^^^^^^^^^^^^^^^^^ Use `fetch(:key, 2r)` instead of `fetch(:key) { 2r }`.
        "#});
    }

    #[test]
    fn flags_empty_block() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(:key) {}
                 ^^^^^^^^^^^^^^ Use `fetch(:key, nil)` instead of `fetch(:key) {}`.
        "#});
    }

    #[test]
    fn flags_array_fetch_int_body() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            array.fetch(5) { :value }
                  ^^^^^^^^^^^^^^^^^^^^ Use `fetch(5, :value)` instead of `fetch(5) { :value }`.
        "#});
    }

    #[test]
    fn flags_str_body_when_frozen_string_literal() {
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            # frozen_string_literal: true
            hash.fetch(:key) { 'value' }
                 ^^^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, 'value')` instead of `fetch(:key) { 'value' }`.
        "#});
    }

    // ----- Autocorrect cases -----

    #[test]
    fn corrects_int_body() {
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch(:key) { 5 }
                     ^^^^^^^^^^^^^^^^^ Use `fetch(:key, 5)` instead of `fetch(:key) { 5 }`.
            "#},
            "hash.fetch(:key, 5)\n",
        );
    }

    #[test]
    fn corrects_nil_body() {
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch(:key) { nil }
                     ^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, nil)` instead of `fetch(:key) { nil }`.
            "#},
            "hash.fetch(:key, nil)\n",
        );
    }

    #[test]
    fn corrects_sym_body() {
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch(:key) { :value }
                     ^^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, :value)` instead of `fetch(:key) { :value }`.
            "#},
            "hash.fetch(:key, :value)\n",
        );
    }

    #[test]
    fn corrects_empty_block() {
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch(:key) {}
                     ^^^^^^^^^^^^^^ Use `fetch(:key, nil)` instead of `fetch(:key) {}`.
            "#},
            "hash.fetch(:key, nil)\n",
        );
    }

    #[test]
    fn corrects_str_body_when_frozen_string_literal() {
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                # frozen_string_literal: true
                hash.fetch(:key) { 'value' }
                     ^^^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, 'value')` instead of `fetch(:key) { 'value' }`.
            "#},
            "# frozen_string_literal: true\nhash.fetch(:key, 'value')\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_fetch_with_default_arg_already() {
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key, 5)\n");
    }

    #[test]
    fn accepts_fetch_with_block_params() {
        // Block with parameters is not redundant.
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key) { |k| k.to_s }\n");
    }

    #[test]
    fn accepts_fetch_with_non_literal_block() {
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key) { expensive_call() }\n");
    }

    #[test]
    fn accepts_str_body_without_frozen_string_literal() {
        // String body without the frozen_string_literal comment should not fire.
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key) { 'value' }\n");
    }

    #[test]
    fn accepts_const_body_without_safe_for_constants() {
        // Constant body requires SafeForConstants: true — default is false.
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key) { VALUE }\n");
    }

    #[test]
    fn flags_const_body_when_safe_for_constants_enabled() {
        // `SafeForConstants: true` is read live via `cx.options_or_default`, so a
        // constant default value is flagged.
        test::<RedundantFetchBlock>()
            .with_options(&Options {
                safe_for_constants: true,
            })
            .expect_offense(indoc! {r#"
                hash.fetch(:key) { VALUE }
                     ^^^^^^^^^^^^^^^^^^^^^ Use `fetch(:key, VALUE)` instead of `fetch(:key) { VALUE }`.
            "#});
    }

    #[test]
    fn accepts_rails_cache_receiver() {
        // Rails.cache.fetch should not be flagged.
        test::<RedundantFetchBlock>()
            .expect_no_offenses("Rails.cache.fetch(:key) { 'value' }\n");
    }

    #[test]
    fn accepts_fetch_with_multiple_args() {
        // fetch with 2 args already has a default — no block to simplify here,
        // but confirm the multi-key case is not flagged.
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch(:key, :default) { 5 }\n");
    }

    #[test]
    fn accepts_fetch_with_no_args() {
        test::<RedundantFetchBlock>().expect_no_offenses("hash.fetch { 5 }\n");
    }

    // ----- Bare hash key: autocorrect must wrap in {} -----

    #[test]
    fn flags_bare_hash_key() {
        // Bare hash key `foo: :bar` (no braces) — offense is flagged.
        test::<RedundantFetchBlock>().expect_offense(indoc! {r#"
            hash.fetch(foo: :bar) { 1 }
                 ^^^^^^^^^^^^^^^^^^^^^^^ Use `fetch({foo: :bar}, 1)` instead of `fetch(foo: :bar) { 1 }`.
        "#});
    }

    #[test]
    fn corrects_bare_hash_key_wraps_in_braces() {
        // Bare hash key must be wrapped in `{}` so autocorrect produces valid Ruby.
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch(foo: :bar) { 1 }
                     ^^^^^^^^^^^^^^^^^^^^^^^ Use `fetch({foo: :bar}, 1)` instead of `fetch(foo: :bar) { 1 }`.
            "#},
            "hash.fetch({foo: :bar}, 1)\n",
        );
    }

    #[test]
    fn corrects_braced_hash_key_no_double_wrap() {
        // Hash key with explicit braces `{foo: :bar}` should NOT be double-wrapped.
        test::<RedundantFetchBlock>().expect_correction(
            indoc! {r#"
                hash.fetch({foo: :bar}) { 1 }
                     ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `fetch({foo: :bar}, 1)` instead of `fetch({foo: :bar}) { 1 }`.
            "#},
            "hash.fetch({foo: :bar}, 1)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantFetchBlock);
