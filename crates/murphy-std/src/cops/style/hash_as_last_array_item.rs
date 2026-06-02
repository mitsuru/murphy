//! `Style/HashAsLastArrayItem` — checks for presence or absence of braces
//! around a hash literal as the last item in an array literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashAsLastArrayItem
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Enforces `braces` or `no_braces` style for a hash literal that is the
//!   last item in a square-bracket array literal.
//!
//!   Supported:
//!   - EnforcedStyle: no_braces (default, matches RuboCop default)
//!   - EnforcedStyle: braces
//!   - Skip arrays where all items are hashes already in the target style
//!     (the [{a:1},{b:2}] NOTE case)
//!   - Skip if second-to-last item is a hash (multiple trailing hashes)
//!   - Skip percent-literal arrays (%w / %i etc.) — can't have bare hashes
//!   - Skip if the hash's first child is a kwsplat (`{**h}`)
//!   - Autocorrect:
//!     - braces style: wrap bare hash in `{` and `}`
//!     - no_braces style: strip `{` and `}` from braced hash (also remove
//!       trailing comma before the hash if present)
//!
//!   Gaps vs RuboCop:
//!   - Multi-line "wrap with indent" correction (single-line only for now)
//!   - Trailing comma removal after the last element when removing braces
//!     is a best-effort token scan (not range_with_surrounding_space)
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # EnforcedStyle: no_braces (default)
//!
//! # bad
//! [1, 2, { one: 1, two: 2 }]
//!
//! # good
//! [1, 2, one: 1, two: 2]
//!
//! # EnforcedStyle: braces
//!
//! # bad
//! [1, 2, one: 1, two: 2]
//!
//! # good
//! [1, 2, { one: 1, two: 2 }]
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashAsLastArrayItem;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "no_braces")]
    NoBraces,
    #[option(value = "braces")]
    Braces,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "no_braces",
        description = "Whether the last hash in an array should have braces (`braces`) or not (`no_braces`)."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/HashAsLastArrayItem",
    description = "Checks for presence or absence of braces around hash literal as last array item.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl HashAsLastArrayItem {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip if the hash's first child is a kwsplat (`{**h}`).
        if hash_has_leading_kwsplat(node, cx) {
            return;
        }

        // Parent must be an Array.
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        if !matches!(cx.kind(parent), NodeKind::Array(_)) {
            return;
        }

        // Array must use square brackets (not percent literals).
        if !cx.is_square_brackets(parent) {
            return;
        }

        // Must be the last element.
        let elements = cx.array_elements(parent);
        if elements.last() != Some(&node) {
            return;
        }

        let opts = cx.options_or_default::<Options>();

        // Skip if all array elements are hashes already in target style.
        if all_elements_match_style(elements, opts.enforced_style, cx) {
            return;
        }

        // Skip if the second-to-last element is also a hash.
        if elements.len() >= 2 {
            let second_to_last = elements[elements.len() - 2];
            if matches!(cx.kind(second_to_last), NodeKind::Hash(_)) {
                return;
            }
        }

        match opts.enforced_style {
            EnforcedStyle::Braces => check_braces(node, cx),
            EnforcedStyle::NoBraces => check_no_braces(node, cx),
        }
    }
}

/// Returns `true` if the hash's first child node is a `Kwsplat`.
fn hash_has_leading_kwsplat(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Hash(list) = *cx.kind(node) else {
        return false;
    };
    let children = cx.list(list);
    children
        .first()
        .is_some_and(|&first| matches!(cx.kind(first), NodeKind::Kwsplat(_)))
}

/// Returns `true` if every array element is a hash already matching target style.
fn all_elements_match_style(elements: &[NodeId], style: EnforcedStyle, cx: &Cx<'_>) -> bool {
    !elements.is_empty()
        && elements.iter().all(|&el| {
            if !matches!(cx.kind(el), NodeKind::Hash(_)) {
                return false;
            }
            match style {
                EnforcedStyle::Braces => hash_has_braces(el, cx),
                EnforcedStyle::NoBraces => !hash_has_braces(el, cx),
            }
        })
}

/// Returns `true` if the hash node is written with explicit `{` / `}` delimiters.
fn hash_has_braces(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.raw_source(cx.range(node));
    src.starts_with('{')
}

/// EnforcedStyle: braces — flag unbraced hash.
fn check_braces(node: NodeId, cx: &Cx<'_>) {
    if hash_has_braces(node, cx) {
        return;
    }
    let hash_range = cx.range(node);
    cx.emit_offense(hash_range, "Wrap hash in `{` and `}`.", None);
    // Autocorrect: wrap the hash source in `{ }`.
    let hash_src = cx.raw_source(hash_range);
    let replacement = format!("{{ {hash_src} }}");
    cx.emit_edit(hash_range, &replacement);
}

/// EnforcedStyle: no_braces — flag braced hash.
fn check_no_braces(node: NodeId, cx: &Cx<'_>) {
    if !hash_has_braces(node, cx) {
        return;
    }
    // Skip empty hash — cannot be "unbraced".
    let NodeKind::Hash(list) = *cx.kind(node) else {
        return;
    };
    if cx.list(list).is_empty() {
        return;
    }
    let hash_range = cx.range(node);
    cx.emit_offense(hash_range, "Omit the braces around the hash.", None);

    // Autocorrect: extract inner content between `{` and `}`.
    let toks = cx.tokens_in(hash_range);
    let open_brace = toks.iter().find(|t| t.kind == SourceTokenKind::LeftBrace);
    let close_brace = toks.iter().rfind(|t| t.kind == SourceTokenKind::RightBrace);

    if let (Some(ob), Some(cb)) = (open_brace, close_brace) {
        let inner_start = ob.range.end;
        let inner_end = cb.range.start;
        if inner_start >= inner_end {
            return;
        }
        let inner_src = cx.raw_source(Range {
            start: inner_start,
            end: inner_end,
        });
        let inner_trimmed = inner_src.trim();
        // Replace the entire braced hash with the inner content (no braces).
        cx.emit_edit(hash_range, inner_trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, HashAsLastArrayItem, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- EnforcedStyle: braces ----

    #[test]
    fn flags_unbraced_hash_at_end_with_braces_style() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_offense(indoc! {"
                [1, 2, one: 1, two: 2]
                       ^^^^^^^^^^^^^^ Wrap hash in `{` and `}`.
            "});
    }

    #[test]
    fn does_not_flag_braced_hash_with_braces_style() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [1, 2, { one: 1, two: 2 }]
            "});
    }

    #[test]
    fn flags_sole_unbraced_hash_with_braces_style() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_offense(indoc! {"
                [one: 1, two: 2]
                 ^^^^^^^^^^^^^^ Wrap hash in `{` and `}`.
            "});
    }

    // ---- EnforcedStyle: no_braces (default) ----

    #[test]
    fn flags_braced_hash_at_end_with_no_braces_style() {
        test::<HashAsLastArrayItem>().expect_offense(indoc! {"
                [1, 2, { one: 1, two: 2 }]
                       ^^^^^^^^^^^^^^^^^^ Omit the braces around the hash.
            "});
    }

    #[test]
    fn does_not_flag_unbraced_hash_with_no_braces_style() {
        test::<HashAsLastArrayItem>().expect_no_offenses(indoc! {"
            [1, 2, one: 1, two: 2]
        "});
    }

    #[test]
    fn flags_sole_braced_hash_with_no_braces_style() {
        test::<HashAsLastArrayItem>().expect_offense(indoc! {"
                [{ one: 1, two: 2 }]
                 ^^^^^^^^^^^^^^^^^^ Omit the braces around the hash.
            "});
    }

    // ---- Skip cases ----

    #[test]
    fn does_not_flag_hash_not_last_element() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [{ one: 1 }, 2]
            "});
    }

    #[test]
    fn does_not_flag_kwsplat_hash() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [1, {**h}]
            "});
    }

    #[test]
    fn does_not_flag_all_braced_hashes_with_braces_style() {
        // All elements are already braced hashes — skip per NOTE in RuboCop.
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [{ one: 1 }, { two: 2 }]
            "});
    }

    #[test]
    fn does_not_flag_second_to_last_is_hash() {
        // When second-to-last is a hash, multiple trailing hashes — skip.
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [1, { a: 1 }, b: 2]
            "});
    }

    #[test]
    fn does_not_flag_empty_hash_with_no_braces() {
        // Empty braced hash cannot be "unbraced".
        test::<HashAsLastArrayItem>().expect_no_offenses(indoc! {"
            [1, {}]
        "});
    }

    // ---- Autocorrect: braces style ----

    #[test]
    fn corrects_unbraced_hash_to_braced() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_correction(
                indoc! {"
                    [1, 2, one: 1, two: 2]
                           ^^^^^^^^^^^^^^ Wrap hash in `{` and `}`.
                "},
                indoc! {"
                    [1, 2, { one: 1, two: 2 }]
                "},
            );
    }

    // ---- Autocorrect: no_braces style ----

    #[test]
    fn corrects_braced_hash_to_unbraced() {
        test::<HashAsLastArrayItem>().expect_correction(
            indoc! {"
                    [1, 2, { one: 1, two: 2 }]
                           ^^^^^^^^^^^^^^^^^^ Omit the braces around the hash.
                "},
            indoc! {"
                    [1, 2, one: 1, two: 2]
                "},
        );
    }

    // ---- Idempotency ----

    #[test]
    fn no_braces_style_is_idempotent() {
        test::<HashAsLastArrayItem>().expect_no_offenses(indoc! {"
            [1, 2, one: 1, two: 2]
        "});
    }

    #[test]
    fn braces_style_is_idempotent() {
        test::<HashAsLastArrayItem>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                [1, 2, { one: 1, two: 2 }]
            "});
    }
}

murphy_plugin_api::submit_cop!(HashAsLastArrayItem);
