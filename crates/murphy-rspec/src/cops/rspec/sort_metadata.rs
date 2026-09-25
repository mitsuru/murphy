//! `RSpec/SortMetadata` — sort RSpec metadata alphabetically.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SortMetadata
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `Metadata#on_metadata` (`rspec_metadata`: example /
//!   example-group / shared-group / hook blocks with an RSpec-or-bare
//!   receiver, first arg skipped, trailing `Hash` split off) plus
//!   `trailing_symbols` (the trailing `Sym` run, dropping a final
//!   ambiguous non-literal arg per
//!   `match_ambiguous_trailing_metadata?`), `sorted?` (case-insensitive
//!   symbol and pair-key order), and the crime-scene range (first to last
//!   metadata node) with `Sort metadata alphabetically.` Detection is at
//!   parity (verified vs 3.7.0, including combined symbol+hash metadata,
//!   the ambiguous-trailing-variable drop, and the hook / shared-group
//!   owners); autocorrect (replace with the sorted spelling) is not
//!   ported in this batch — same convention as `RSpec/MetadataStyle`
//!   (status: partial, autocorrect as gap). Upstream's `on_numblock`
//!   alias never matches (`rspec_metadata` requires `block`), so only
//!   `Block` is dispatched here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` with metadata owners:
//!
//! - `describe 'Something', :b, :a do; end` — flagged (`:b, :a`).
//! - `context 'Something', foo: 'bar', baz: true do; end` — flagged
//!   (the hash range).
//! - `it 'works', :b, :a, foo: 'bar', baz: true do; end` — flagged
//!   (symbols plus hash).
//! - `describe 'Something', 'description', :a, :b, :z do; end` —
//!   trailing run sorted, clean.
//! - `context 'Something', :z, variable, :a, :b do; end` — ambiguous
//!   `variable` breaks the run, trailing `:a, :b` sorted, clean.
//! - `describe 'Something', :a, :b do; end` — already sorted, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces the crime scene with the sorted spelling. This
//! batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

use crate::cops::rspec_helpers::{
    is_example_group_name, is_example_name, is_hook_name, is_rspec_or_bare_receiver,
    is_shared_group_name,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SortMetadata;

#[cop(
    name = "RSpec/SortMetadata",
    description = "Sort RSpec metadata alphabetically.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SortMetadata {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
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
        // `rspec_metadata`: `(send ... _ $...)` — the first arg is the
        // description / scope, the rest is the metadata.
        if arg_ids.is_empty() {
            return;
        }
        let metadata = &arg_ids[1..];
        // `on_metadata_arguments`: a trailing `Hash` splits off.
        let (leading, hash) = match metadata.last() {
            Some(&last) if matches!(*cx.kind(last), NodeKind::Hash(_)) => {
                (&metadata[..metadata.len() - 1], Some(last))
            }
            _ => (metadata, None),
        };
        let symbols = trailing_symbols(cx, leading);
        let pairs: Vec<NodeId> = match hash {
            Some(hash_id) => {
                let NodeKind::Hash(list) = *cx.kind(hash_id) else {
                    return;
                };
                cx.list(list).to_vec()
            }
            None => Vec::new(),
        };
        if symbols.is_empty() && pairs.is_empty() {
            return;
        }
        if sorted_symbols(cx, &symbols) && sorted_pairs(cx, &pairs) {
            return;
        }
        // `crime_scene`: first to last metadata node.
        let first = symbols.first().or(pairs.first()).expect("non-empty");
        let last = pairs.last().or(symbols.last()).expect("non-empty");
        cx.emit_offense(
            Range {
                start: cx.range(*first).start,
                end: cx.range(*last).end,
            },
            "Sort metadata alphabetically.",
            None,
        );
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

/// Trailing `Sym` run of `args`, dropping a final ambiguous arg that
/// could be a brace-less hash (`match_ambiguous_trailing_metadata?`:
/// the send's last arg is not a hash / sym / string literal).
fn trailing_symbols(cx: &Cx<'_>, args: &[NodeId]) -> Vec<NodeId> {
    let mut args = args;
    if let Some(&last) = args.last()
        && !matches!(
            *cx.kind(last),
            NodeKind::Hash(_)
                | NodeKind::Sym(_)
                | NodeKind::Str(_)
                | NodeKind::Dstr(_)
                | NodeKind::Xstr(_)
        )
    {
        args = &args[..args.len() - 1];
    }
    let mut out: Vec<NodeId> = args
        .iter()
        .rev()
        .take_while(|&&id| matches!(*cx.kind(id), NodeKind::Sym(_)))
        .copied()
        .collect();
    out.reverse();
    out
}

/// `true` when `symbols` are already sorted case-insensitively.
fn sorted_symbols(cx: &Cx<'_>, symbols: &[NodeId]) -> bool {
    let lowered: Vec<String> = symbols
        .iter()
        .map(|&id| {
            let NodeKind::Sym(sym) = *cx.kind(id) else {
                return String::new();
            };
            cx.symbol_str(sym).to_lowercase()
        })
        .collect();
    let mut ordered = lowered.clone();
    ordered.sort();
    lowered == ordered
}

/// `true` when `pairs` are already sorted by key source,
/// case-insensitively (`pair.key.source.downcase`).
fn sorted_pairs(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    let keys: Vec<String> = pairs
        .iter()
        .map(|&id| {
            let NodeKind::Pair { key, .. } = *cx.kind(id) else {
                return String::new();
            };
            cx.raw_source(cx.range(key)).to_lowercase()
        })
        .collect();
    let mut ordered = keys.clone();
    ordered.sort();
    keys == ordered
}

#[cfg(test)]
mod tests {
    use super::SortMetadata;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unsorted_symbols() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                describe 'Something', :b, :a do
                                      ^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn flags_unsorted_hash() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                context 'Something', foo: 'bar', baz: true do
                                     ^^^^^^^^^^^^^^^^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn flags_unsorted_symbols_and_hash() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                it 'works', :b, :a, foo: 'bar', baz: true do
                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn ignores_sorted_trailing_metadata() {
        test::<SortMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', 'description', :a, :b, :z do
                end
            "#});
    }

    #[test]
    fn ignores_sorted_after_ambiguous_variable() {
        test::<SortMetadata>().expect_no_offenses(indoc! {r#"
                context 'Something', :z, variable, :a, :b do
                end
            "#});
    }

    #[test]
    fn flags_unsorted_before_ambiguous_variable() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                context 'x', :b, :a, variable do
                             ^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn ignores_sorted_symbols() {
        test::<SortMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', :a, :b do
                end
            "#});
    }

    #[test]
    fn flags_hook_metadata() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                before(:each, :b, :a) do
                              ^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn flags_shared_group_metadata() {
        test::<SortMetadata>().expect_offense(indoc! {r#"
                shared_examples 'x', :b, :a do
                                     ^^^^^^ Sort metadata alphabetically.
                end
            "#});
    }

    #[test]
    fn ignores_case_insensitive_sorted() {
        // Sorted ignoring case stays clean; `:B, :a` flags because
        // `["b", "a"]` is not sorted.
        test::<SortMetadata>().expect_no_offenses(indoc! {r#"
                describe 'x', :a, :B do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(SortMetadata);
