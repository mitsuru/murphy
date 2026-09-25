//! `RSpec/MetadataStyle` — consistent RSpec metadata style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MetadataStyle
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `Metadata#on_metadata` (`rspec_metadata`: example /
//!   example-group / shared-group / hook blocks with an RSpec-or-bare
//!   receiver; trailing args after the first split into leading symbols
//!   plus an optional trailing `Hash` per `on_metadata_arguments`).
//!   `EnforcedStyle: symbol` (default) flags `Pair`s with a `Sym` key
//!   and `True` value per `match_boolean_metadata_pair?` (`(pair sym
//!   true)`); `EnforcedStyle: hash` flags every trailing `Sym` per
//!   `bad_metadata_symbol?`. Each offense is the pair / symbol node per
//!   `add_offense(node)` with `Use %<style>s style for metadata.`
//!   Detection is at parity for direct blocks (verified vs 3.7.0,
//!   including `numblock` via the mixin's `on_numblock` alias —
//!   `describe('x', :a) { _1 }` flags under `hash`); the
//!   `RSpec.configure { |c| c.before(...) }` indirection via
//!   `metadata_in_block` is not ported in this batch (status: partial,
//!   same gap as `RSpec/DuplicatedMetadata`). No autocorrect upstream
//!   beyond style rewrite — none here.
//! ```
//!
//! ## Matched shapes
//!
//! Default style `symbol`:
//!
//! - `describe 'S', a: true do; end` — flagged (the `a: true` pair).
//! - `describe 'S', a: 1 do; end` — non-`true` value, clean.
//! - `describe 'S', a: false do; end` — clean.
//! - `describe 'S', :a do; end` — already a symbol, clean.
//! - `it 'x', :a, b: true do; end` — only `b: true` flagged.
//!
//! Style `hash`:
//!
//! - `describe 'S', :a do; end` — flagged (the `:a` symbol).
//! - `describe 'S', :a, :b do; end` — both flagged.
//! - `describe 'S', a: true do; end` — already a pair, clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites between the spellings. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    is_example_group_name, is_hook_name, is_rspec_or_bare_receiver,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MetadataStyle;

#[derive(CopOptions)]
pub struct MetadataStyleOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symbol",
        description = "Whether metadata uses symbol (`:a`) or hash (`a: true`) style."
    )]
    pub enforced_style: MetadataStyleStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum MetadataStyleStyle {
    #[option(value = "symbol")]
    Symbol,
    #[option(value = "hash")]
    Hash,
}

#[cop(
    name = "RSpec/MetadataStyle",
    description = "Use consistent metadata style.",
    default_severity = "warning",
    default_enabled = true,
    options = MetadataStyleOptions,
)]
impl MetadataStyle {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        check_send(cx, call);
    }

    // Upstream `Metadata` aliases `on_numblock`: numbered-param groups
    // carry metadata the same way (`describe('x', :a) { _1 }`).
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, .. } = *cx.kind(node) else {
            return;
        };
        check_send(cx, send);
    }
}

/// Shared `rspec_metadata` gate plus the style check for `block` and
/// `numblock` owners.
fn check_send(cx: &Cx<'_>, call: NodeId) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(call)
    else {
        return;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        return;
    }
    if !is_metadata_owner(cx.symbol_str(method)) {
        return;
    }
    let arg_ids = cx.list(args);
    if arg_ids.len() < 2 {
        return;
    }
    // `on_metadata_arguments`: a trailing `Hash` splits off; the rest
    // are the leading metadata symbols.
    let metadata = &arg_ids[1..];
    let (symbols, hash) = match metadata.last() {
        Some(&last) if matches!(*cx.kind(last), NodeKind::Hash(_)) => {
            (&metadata[..metadata.len() - 1], Some(last))
        }
        _ => (metadata, None),
    };
    let opts = cx.options_or_default::<MetadataStyleOptions>();
    match opts.enforced_style {
        MetadataStyleStyle::Symbol => {
            let Some(hash_id) = hash else {
                return;
            };
            let NodeKind::Hash(pairs) = *cx.kind(hash_id) else {
                return;
            };
            for pair_id in cx.list(pairs).to_vec() {
                let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                    continue;
                };
                // `match_boolean_metadata_pair?`: `(pair sym true)`.
                if !matches!(*cx.kind(key), NodeKind::Sym(_)) {
                    continue;
                }
                if !matches!(*cx.kind(value), NodeKind::True_) {
                    continue;
                }
                cx.emit_offense(
                    cx.range(pair_id),
                    "Use symbol style for metadata.",
                    None,
                );
            }
        }
        MetadataStyleStyle::Hash => {
            for &sym_id in symbols {
                if !matches!(*cx.kind(sym_id), NodeKind::Sym(_)) {
                    continue;
                }
                cx.emit_offense(
                    cx.range(sym_id),
                    "Use hash style for metadata.",
                    None,
                );
            }
        }
    }
}

/// `true` when `name` can carry RSpec metadata (`rspec_metadata` owners:
/// examples, example groups, shared groups, hooks).
fn is_metadata_owner(name: &str) -> bool {
    is_example_name(name)
        || is_example_group_name(name)
        || is_shared_group_name(name)
        || is_hook_name(name)
}

/// Example selectors (`Examples.all`): regular + focused + skipped +
/// pending, mirroring `config/default.yml` Language section.
fn is_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

/// Shared-group selectors (`SharedGroups.all`).
fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

#[cfg(test)]
mod tests {
    use super::{MetadataStyle, MetadataStyleOptions, MetadataStyleStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn hash_style() -> MetadataStyleOptions {
        MetadataStyleOptions {
            enforced_style: MetadataStyleStyle::Hash,
        }
    }

    #[test]
    fn flags_true_pair_in_default_symbol_style() {
        test::<MetadataStyle>().expect_offense(indoc! {r#"
                describe 'Something', a: true do; end
                                      ^^^^^^^ Use symbol style for metadata.
            "#});
    }

    #[test]
    fn ignores_non_true_pair_in_symbol_style() {
        test::<MetadataStyle>().expect_no_offenses(indoc! {r#"
                describe 'Something', a: 1 do; end
            "#});
    }

    #[test]
    fn ignores_false_pair_in_symbol_style() {
        test::<MetadataStyle>().expect_no_offenses(indoc! {r#"
                describe 'Something', a: false do; end
            "#});
    }

    #[test]
    fn ignores_symbol_in_symbol_style() {
        test::<MetadataStyle>().expect_no_offenses(indoc! {r#"
                describe 'Something', :a do; end
            "#});
    }

    #[test]
    fn flags_only_true_pair_among_mixed_metadata() {
        test::<MetadataStyle>().expect_offense(indoc! {r#"
                it 'x', :a, b: true do; end
                            ^^^^^^^ Use symbol style for metadata.
            "#});
    }

    #[test]
    fn ignores_symbol_pair_in_symbol_style() {
        // `:a, :b` carry no hash at all — clean (verified vs 3.7.0).
        test::<MetadataStyle>().expect_no_offenses(indoc! {r#"
                describe 'S', :a, :b do; end
            "#});
    }

    #[test]
    fn flags_symbol_in_hash_style() {
        test::<MetadataStyle>()
            .with_options(&hash_style())
            .expect_offense(indoc! {r#"
                describe 'Something', :a do; end
                                      ^^ Use hash style for metadata.
            "#});
    }

    #[test]
    fn flags_each_symbol_in_hash_style() {
        test::<MetadataStyle>()
            .with_options(&hash_style())
            .expect_offense(indoc! {r#"
                describe 'S', :a, :b do; end
                              ^^ Use hash style for metadata.
                                  ^^ Use hash style for metadata.
            "#});
    }

    #[test]
    fn ignores_pair_in_hash_style() {
        test::<MetadataStyle>()
            .with_options(&hash_style())
            .expect_no_offenses(indoc! {r#"
                describe 'Something', a: true do; end
            "#});
    }

    #[test]
    fn ignores_non_metadata_owner() {
        test::<MetadataStyle>().expect_no_offenses(indoc! {r#"
                foo 'Something', a: true do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MetadataStyle);
