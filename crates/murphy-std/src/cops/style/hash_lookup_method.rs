//! `Style/HashLookupMethod` — enforces `Hash#[]` or `Hash#fetch` consistency.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashLookupMethod
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Verbatim port of the call head `(call _ {:[] :fetch} ...)`
//!   (murphy-s1yc.15): `call` = `{send csend}` covers safe-navigation
//!   (`hash&.fetch(key)`), mirroring RuboCop `alias on_csend on_send` plus
//!   `RESTRICT_ON_SEND`; the wildcard receiver binds an absent or present
//!   receiver per murphy-if9y; trailing `...` absorbs any argument list.
//!   The receiver plus one-arg plus block plus brackets-csend guards below
//!   apply separately (upstream brackets direction excludes csend via
//!   `!node.csend_type?` while fetch direction flags it; murphy preserves
//!   offense-only with no autocorrect as a complementary gap).
//!   EnforcedStyle: brackets (default) and fetch are supported.
//!   AllowedReceivers is not yet wired (config option deferred).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.15):
// `(call _ {:[] :fetch} ...)` — `call` = `{send csend}` covers
// safe-navigation (`hash&.fetch(key)`), mirroring RuboCop
// `alias on_csend on_send` plus `RESTRICT_ON_SEND`. The `_` receiver binds
// an absent or present receiver per murphy-if9y; trailing `...` absorbs any
// argument list, so the receiver plus one-arg plus block plus
// brackets-csend guards below apply separately (upstream brackets direction
// excludes csend via `!node.csend_type?` while fetch direction flags it
// with a `&.fetch` correction; murphy preserves offense-only with no
// autocorrect).
def_node_matcher!(
    hash_lookup_method_call,
    "(call _ {:[] :fetch} ...)"
);

const BRACKET_MSG: &str = "Use `Hash#[]` instead of `Hash#fetch`.";
const FETCH_MSG: &str = "Use `Hash#fetch` instead of `Hash#[]`.";

#[derive(Default)]
pub struct HashLookupMethod;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "brackets")]
    Brackets,
    #[option(value = "fetch")]
    Fetch,
}

#[derive(CopOptions)]
pub struct HashLookupMethodOptions {
    #[option(name = "EnforcedStyle", 
        default = "brackets",
        description = "Enforced style for hash lookup."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/HashLookupMethod",
    description = "Enforce consistent hash lookup method.",
    default_severity = "warning",
    default_enabled = false,
    options = HashLookupMethodOptions
)]
impl HashLookupMethod {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ {:[] :fetch} ...)` head: filters to `[]` / `fetch`
    // calls on either send or csend (safe-navigation), with any receiver
    // (absent or present). Without this, an unrelated call on a hash
    // receiver (e.g. `hash.delete(key)`) would run check on every call
    // node instead of being rejected by the method set up front.
    if !hash_lookup_method_call(node, cx) {
        return;
    }

    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `fetch(key)` is accepted
    // here (upstream brackets direction also accepts bare: it requires
    // `node.receiver`).
    if cx.call_receiver(node).get().is_none() {
        return;
    }
    let opts = cx.options_or_default::<HashLookupMethodOptions>();
    let method_name = cx.method_name(node);

    match opts.enforced_style {
        EnforcedStyle::Brackets => {
            if method_name == Some("fetch") {
                // Skip safe-navigation: replacing `hash&.fetch(key)` with
                // `hash[key]` can introduce NoMethodError on nil. Mirrors
                // upstream `!node.csend_type?` in `offense_for_brackets?`
                // (pre-port csend was never visited, so this also accepts).
                if matches!(cx.kind(node), NodeKind::Csend { .. }) {
                    return;
                }
                // Skip fetch-with-block: cannot replace with `hash[key]`
                // because the block provides a default on KeyError.
                if cx.block_node(node).get().is_some() {
                    return;
                }
                // Trailing `...` absorbs any argument list, so the
                // zero/multi-argument cases are accepted here (upstream
                // requires exactly one argument).
                if cx.call_arguments(node).len() == 1 {
                    cx.emit_offense(cx.range(node), BRACKET_MSG, None);
                }
            }
        }
        EnforcedStyle::Fetch => {
            // Safe-navigation `hash&.[](key)` flags here per upstream
            // `alias on_csend on_send` plus fetch-direction csend handling
            // (upstream corrects to `hash&.fetch(key)`; murphy preserves
            // offense-only with no autocorrect). Pre-port csend was never
            // visited.
            if method_name == Some("[]") {
                // Trailing `...` absorbs any argument list, so the
                // zero/multi-argument cases are accepted here (upstream
                // requires exactly one argument).
                if cx.call_arguments(node).len() == 1 {
                    cx.emit_offense(cx.range(node), FETCH_MSG, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HashLookupMethod, HashLookupMethodOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn brackets_style_flags_fetch() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_offense(indoc! {"
                hash.fetch(key)
                ^^^^^^^^^^^^^^^ Use `Hash#[]` instead of `Hash#fetch`.
            "});
    }

    #[test]
    fn brackets_style_accepts_brackets() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash[key]\n");
    }

    #[test]
    fn fetch_style_flags_brackets() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_offense(indoc! {"
                hash[key]
                ^^^^^^^^^ Use `Hash#fetch` instead of `Hash#[]`.
            "});
    }

    #[test]
    fn fetch_style_accepts_fetch() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_no_offenses("hash.fetch(key)\n");
    }

    #[test]
    fn fetch_with_default_is_ignored() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash.fetch(key, default)\n");
    }

    #[test]
    fn fetch_with_block_is_ignored() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash.fetch(key) { |k| default }\n");
    }

    #[test]
    fn brackets_with_two_args_are_ignored() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_no_offenses("hash[key, other]\n");
    }

    #[test]
    fn default_style_is_brackets() {
        let opts = HashLookupMethodOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Brackets);
    }

    // --- Characterization (murphy-s1yc.15): pin the exact node set the
    // hand-rolled send-only dispatch matches, so the verbatim
    // `(call _ {:[] :fetch} ...)` port can be proven byte-identical.
    // `call` covers safe-navigation (mirroring upstream `alias on_csend
    // on_send` plus `RESTRICT_ON_SEND`); trailing `...` absorbs any
    // argument list, so the receiver plus one-arg plus block plus
    // brackets-csend guards below apply separately (upstream brackets
    // direction excludes csend via `!node.csend_type?`; fetch direction
    // flags csend with a `&.fetch` correction; murphy preserves
    // offense-only with no autocorrect).

    #[test]
    fn s1yc15_flags_csend_fetch_style_index() {
        // Safe navigation on `[]` under fetch style: `call` covers `csend`
        // per murphy-if9y, mirroring upstream `alias on_csend on_send`
        // plus fetch-direction csend handling. (Pre-port the `csend`
        // handler is missing: `check_send` destructures `NodeKind::Send`
        // only and `#[on_node(kind = "send", methods = [...])]` never
        // visits csend.)
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_offense(indoc! {"
                hash&.[](key)
                ^^^^^^^^^^^^^ Use `Hash#fetch` instead of `Hash#[]`.
            "});
    }

    #[test]
    fn s1yc15_accepts_csend_brackets_fetch() {
        // Safe navigation on `fetch` under brackets style: `_` binds the
        // present receiver so the head matches, but the complementary
        // csend guard accepts, mirroring upstream `!node.csend_type?` in
        // `offense_for_brackets?`. (Pre-port this also accepts: csend is
        // never visited.)
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash&.fetch(key)\n");
    }

    #[test]
    fn s1yc15_accepts_bare_fetch() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard accepts.
        // (Upstream brackets direction also accepts bare: it requires
        // `node.receiver`.)
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("fetch(key)\n");
    }

    #[test]
    fn s1yc15_accepts_no_arg_fetch() {
        // No arguments: trailing `...` absorbs the empty list, so the head
        // matches and the complementary one-arg guard accepts.
        // (Upstream requires exactly one argument.)
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash.fetch\n");
    }

    #[test]
    fn s1yc15_accepts_unrelated_method() {
        // `delete` is outside the verbatim method set, so the head rejects.
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash.delete(key)\n");
    }
}
murphy_plugin_api::submit_cop!(HashLookupMethod);
