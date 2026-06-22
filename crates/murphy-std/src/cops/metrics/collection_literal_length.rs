//! `Metrics/CollectionLiteralLength` — flag `Array`/`Hash`/`Set` literals with
//! a large number of entries, which often indicates configuration or data that
//! belongs in an external source.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/CollectionLiteralLength
//! upstream_version_checked: 1.87.0
//! version_added: "1.47"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_array`/`on_hash`/`on_index` exactly. An offense is
//!   raised when the number of direct entries is `>= LengthThreshold` (default
//!   250) — note the comparison is `>=`, NOT `>`: a 250-entry literal fires,
//!   a 249-entry literal does not (verified numerically against rubocop
//!   1.87.0).
//!
//!   Three literal shapes are checked, all emitting the offense on the whole
//!   literal node (RuboCop's `add_offense(node)`):
//!
//!   1. Array literal (`on_array`): `node.children.length` — every direct
//!      element counts as one entry (a nested array `[[...], [...]]` counts as
//!      2 entries, not the inner element count; a splat `*a` counts as 1).
//!   2. Hash literal (`on_hash`): `node.children.length` — every `pair` counts
//!      as one entry. This also fires on brace-less keyword-argument hashes
//!      (`foo(a: 1, ...)`), which murphy represents as a `hash` node, matching
//!      RuboCop.
//!   3. `Set[...]` index (`on_index`/`on_send` for method `[]`): only when the
//!      receiver is the global `Set` constant (bare `Set` or top-level `::Set`,
//!      via RuboCop's `(const {cbase nil?} :Set)` matcher). `node.arguments`
//!      are counted, so the offense skips ordinary index access like `arr[0]`
//!      or `hash[:k]`.
//!
//!   The message is RuboCop's bare `MSG` constant with NO `[count/threshold]`
//!   suffix (`add_offense(node)` passes no message argument):
//!   "Avoid hard coding large quantities of data in code. Prefer reading the
//!   data from an external source."
//!
//!   RuboCop's code-level default is `Float::INFINITY` (effectively disabled
//!   when unconfigured), but murphy ships `LengthThreshold: 250` in
//!   default.yml, so 250 is the option default here.
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (LengthThreshold: 250)
//! [1, 2, '...', 999_999_999]                       # 250+ elements
//! { 1 => 1, 2 => 2, '...' => '...' }               # 250+ pairs
//! Set[1, 2, '...', 999_999_999]                    # 250+ arguments
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct CollectionLiteralLength;

/// Options for [`CollectionLiteralLength`]. Mirrors RuboCop's `LengthThreshold`.
#[derive(CopOptions)]
pub struct CollectionLiteralLengthOptions {
    #[option(
        name = "LengthThreshold",
        default = 250,
        description = "Maximum number of entries allowed in an Array/Hash/Set literal."
    )]
    pub length_threshold: i64,
}

/// RuboCop's `MSG`. Static — no `[count/threshold]` suffix.
const MSG: &str = "Avoid hard coding large quantities of data in code. \
                   Prefer reading the data from an external source.";

#[cop(
    name = "Metrics/CollectionLiteralLength",
    description = "Checks for `Array` or `Hash` literals with many entries.",
    default_severity = "warning",
    default_enabled = true,
    options = CollectionLiteralLengthOptions,
)]
impl CollectionLiteralLength {
    /// RuboCop `on_array`: array literal with too many elements.
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Array(list) = cx.kind(node) else {
            return;
        };
        self.check_count(cx.list(*list).len(), node, cx);
    }

    /// RuboCop `alias on_hash on_array`: hash literal with too many pairs.
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Hash(list) = cx.kind(node) else {
            return;
        };
        self.check_count(cx.list(*list).len(), node, cx);
    }

    /// RuboCop `on_index`/`on_send`: `Set[...]` with too many arguments.
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_set_index(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop `set_const?`: `(const {cbase nil?} :Set)`.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if !cx.is_global_const(receiver, "Set") {
            return;
        }
        self.check_count(cx.call_arguments(node).len(), node, cx);
    }
}

impl CollectionLiteralLength {
    /// Shared check: emit the whole-node offense when `count` meets or exceeds
    /// the configured threshold (RuboCop's `>= collection_threshold`).
    fn check_count(&self, count: usize, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CollectionLiteralLengthOptions>();
        if (count as i64) < opts.length_threshold {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{CollectionLiteralLength, CollectionLiteralLengthOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(threshold: i64) -> CollectionLiteralLengthOptions {
        CollectionLiteralLengthOptions { length_threshold: threshold }
    }

    #[test]
    fn flags_array_at_threshold() {
        // threshold = 3; exactly 3 elements fires (RuboCop's `>=`).
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                ARR = [1, 2, 3]
                      ^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn accepts_array_below_threshold() {
        // 2 elements < 3.
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_no_offenses("ARR = [1, 2]\n");
    }

    #[test]
    fn flags_hash_at_threshold() {
        // 3 pairs fires.
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                H = { 1 => 1, 2 => 2, 3 => 3 }
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn accepts_hash_below_threshold() {
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_no_offenses("H = { 1 => 1, 2 => 2 }\n");
    }

    #[test]
    fn flags_braceless_keyword_hash() {
        // Brace-less kwargs form a `hash` node; RuboCop's on_hash fires here too.
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                foo(a: 1, b: 2, c: 3)
                    ^^^^^^^^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn flags_set_index_at_threshold() {
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                S = Set[1, 2, 3]
                    ^^^^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn flags_cbase_set_index() {
        // Top-level `::Set` is also matched (RuboCop's `cbase` branch).
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                S = ::Set[1, 2, 3]
                    ^^^^^^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn accepts_set_index_below_threshold() {
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_no_offenses("S = Set[1, 2]\n");
    }

    #[test]
    fn ignores_non_set_index() {
        // Ordinary index access / non-Set receiver `[]` is never flagged.
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_no_offenses("x = Foo[1, 2, 3, 4]\n");
    }

    #[test]
    fn ignores_plain_index_access() {
        test::<CollectionLiteralLength>().with_options(&opts(3)).expect_no_offenses("x = arr[0]\n");
    }

    #[test]
    fn nested_array_counts_each_child_once() {
        // The outer array has 4 children (fires at threshold 3); each inner
        // array has 1 element (clean). Entries are NOT flattened — only the
        // outer array's direct-child count matters here. Verified against
        // rubocop 1.87.0 (single offense on the outer literal).
        test::<CollectionLiteralLength>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                ARR = [[1], [2], [3], [4]]
                      ^^^^^^^^^^^^^^^^^^^^ Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.
            "});
    }

    #[test]
    fn default_threshold_accepts_small_literals() {
        // Default LengthThreshold is 250; a tiny literal is clean.
        test::<CollectionLiteralLength>().expect_no_offenses("ARR = [1, 2, 3]\n");
    }
}

murphy_plugin_api::submit_cop!(CollectionLiteralLength);
