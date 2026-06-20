//! `Naming/BlockForwarding` — prefer anonymous block forwarding (`&`) over a
//! named block argument (`&block`) that is only forwarded, or vice versa.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/BlockForwarding
//! upstream_version_checked: 1.87.0
//! version_added: "1.24"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is fully implemented for both EnforcedStyle: anonymous
//!   (default) and EnforcedStyle: explicit, verified against rubocop 1.87.0.
//!
//!   anonymous: flags a `def`/`defs` whose LAST argument is an explicit
//!   block parameter (`&block`) when (a) the def has no kwarg/kwoptarg in
//!   its signature, and (b) the block name is not used as a plain local
//!   variable in the body. The signature `&block` is flagged, plus every
//!   call-site `&block` (block_pass) whose inner forwarded variable name
//!   matches. `&:sym` symbol-to-proc block_pass nodes are excluded.
//!
//!   explicit: flags a `def`/`defs` whose last argument is an anonymous
//!   block parameter (`&`), and every anonymous call-site `&` (block_pass
//!   with no inner expression).
//!
//!   Offense range matches RuboCop's: the full `&block` / `&` of the
//!   parameter and of each block_pass (the leading `&` is included). Column
//!   bounds verified against rubocop 1.87.0 JSON output.
//!
//!   Autocorrect is intentionally omitted (issue scope: no-autocorrect).
//!   RuboCop rewrites `&block` <-> `&` and adds parentheses for the
//!   anonymous direction; that whole-signature rewrite is out of scope here.
//!
//!   Known divergence (target-ruby modelling): RuboCop's `invalidates_syntax?`
//!   suppresses the WHOLE def when ANY block_pass in the body is nested inside
//!   another block AND `target_ruby_version <= 3.3` (a Ruby 3.3.0 bug). On
//!   Ruby >= 3.4 RuboCop fires instead. The cop's active range is Ruby 3.1+
//!   (gated by `minimum_target_ruby_version`); within that range RuboCop
//!   suppresses for 3.1–3.3 and fires for >= 3.4. Murphy has no per-version
//!   branching, so it picks the suppress behavior (matching RuboCop at
//!   3.1–3.3, verified against 1.87.0 at TargetRubyVersion 3.3). Projects
//!   targeting Ruby >= 3.4 would see RuboCop fire on the nested-block shape
//!   where Murphy stays silent — a rare shape, documented here rather than
//!   gated.
//! ```
//!
//! ## Example (EnforcedStyle: anonymous, default)
//!
//! ```ruby
//! # bad
//! def foo(&block)
//!   bar(&block)
//! end
//!
//! # good
//! def foo(&)
//!   bar(&)
//! end
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_ANONYMOUS: &str = "Use anonymous block forwarding.";
const MSG_EXPLICIT: &str = "Use explicit block forwarding.";

/// Which block-forwarding style to enforce.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Prefer anonymous `&` (Ruby 3.1+).
    #[default]
    #[option(value = "anonymous")]
    Anonymous,
    /// Prefer an explicit named block argument (`&block`).
    #[option(value = "explicit")]
    Explicit,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "anonymous",
        description = "Which block-forwarding style to enforce."
    )]
    pub enforced_style: EnforcedStyle,

    #[option(
        name = "BlockForwardingName",
        default = "block",
        description = "Block variable name used for autocorrection to the explicit style."
    )]
    pub block_forwarding_name: String,
}

#[derive(Default)]
pub struct BlockForwarding;

#[cop(
    name = "Naming/BlockForwarding",
    description = "Use anonymous block forwarding.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "3.1",
    options = Options,
)]
impl BlockForwarding {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();

    let Some(args_list) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = *cx.kind(args_list) else {
        return;
    };
    let args = cx.list(list);
    let Some(&last_arg) = args.last() else {
        return; // no arguments → nothing to forward
    };

    match opts.enforced_style {
        EnforcedStyle::Anonymous => check_anonymous(node, args, last_arg, cx),
        EnforcedStyle::Explicit => check_explicit(node, last_arg, cx),
    }
}

// ---------------------------------------------------------------------------
// EnforcedStyle: anonymous
// ---------------------------------------------------------------------------

fn check_anonymous(node: NodeId, args: &[NodeId], last_arg: NodeId, cx: &Cx<'_>) {
    // The last argument must be an *explicit* block argument (`&block`).
    let Some(block_name) = explicit_block_argument_name(last_arg, cx) else {
        return;
    };

    // `use_kwarg_in_method_definition?`: a kwarg/kwoptarg in the signature
    // makes anonymous forwarding impossible.
    if args
        .iter()
        .any(|&a| matches!(*cx.kind(a), NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. }))
    {
        return;
    }

    // `use_block_argument_as_local_variable?`: the block name is read/written
    // as a plain local variable in the body (not via `&block` forwarding).
    if uses_block_arg_as_local(node, block_name, cx) {
        return;
    }

    // Collect call-site forwards (`block_pass` whose inner var matches the
    // block name; `&:sym` symbol-to-proc is excluded).
    let body = cx.def_body(node).get();
    let mut forwarded: Vec<NodeId> = Vec::new();
    if let Some(body) = body {
        for desc in cx
            .descendants(body)
            .into_iter()
            .chain(std::iter::once(body))
        {
            let NodeKind::BlockPass(inner) = *cx.kind(desc) else {
                continue;
            };

            // `invalidates_syntax?` runs on EVERY block_pass before the
            // name/sym filter (RuboCop: `return nil if invalidates_syntax?`
            // precedes `next unless block_argument_name_matched?`). On Ruby
            // <= 3.3 a block_pass nested inside another block would be a syntax
            // error once rewritten to `&`, so RuboCop aborts the whole def —
            // even when this particular block_pass is not the forwarded arg.
            if block_pass_inside_block(desc, cx) {
                return;
            }

            let Some(inner) = inner.get() else {
                continue; // anonymous `&` — not a name match
            };
            // `&:sym` symbol-to-proc is never a forward of the block arg.
            if matches!(*cx.kind(inner), NodeKind::Sym(_)) {
                continue;
            }
            if !matches!(*cx.kind(inner), NodeKind::Lvar(name) if cx.symbol_str(name) == block_name)
            {
                continue;
            }

            forwarded.push(desc);
        }
    }

    for &fwd in &forwarded {
        cx.emit_offense(cx.range(fwd), MSG_ANONYMOUS, None);
    }
    // The signature `&block` is always flagged (even with zero call sites).
    cx.emit_offense(blockarg_range(node, last_arg, cx), MSG_ANONYMOUS, None);
}

// ---------------------------------------------------------------------------
// EnforcedStyle: explicit
// ---------------------------------------------------------------------------

fn check_explicit(node: NodeId, last_arg: NodeId, cx: &Cx<'_>) {
    // The last argument must be an *anonymous* block argument (`&`).
    if !is_anonymous_block_argument(last_arg, cx) {
        return;
    }

    // Flag every anonymous call-site forward (`block_pass` with no inner expr).
    if let Some(body) = cx.def_body(node).get() {
        for desc in cx
            .descendants(body)
            .into_iter()
            .chain(std::iter::once(body))
        {
            if let NodeKind::BlockPass(inner) = *cx.kind(desc)
                && inner.get().is_none()
            {
                cx.emit_offense(cx.range(desc), MSG_EXPLICIT, None);
            }
        }
    }
    // The signature `&` is always flagged.
    cx.emit_offense(blockarg_range(node, last_arg, cx), MSG_EXPLICIT, None);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the name of `node` if it is an explicit block argument (`&block`),
/// i.e. a `Blockarg` with a non-empty name. `None` otherwise.
fn explicit_block_argument_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Blockarg(name) => {
            let s = cx.symbol_str(name);
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

/// `true` if `node` is an anonymous block argument (`&`), i.e. a `Blockarg`
/// with an empty name.
fn is_anonymous_block_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Blockarg(name) if cx.symbol_str(name).is_empty())
}

/// Offense range for a block parameter, including the leading `&`.
///
/// Murphy stores a `Blockarg`'s range as the *name* location only (`block`),
/// excluding the `&`. RuboCop highlights the full `&block`, so we extend the
/// range left to the preceding `&` token. For an anonymous `&` (empty name,
/// `Range::ZERO`), the range is the `&` token itself, recovered by scanning
/// the def's parameter list (`def` provides the search scope).
fn blockarg_range(def: NodeId, blockarg: NodeId, cx: &Cx<'_>) -> Range {
    let name_range = cx.range(blockarg);
    if name_range == Range::ZERO {
        // Anonymous `&`: the `Blockarg` has no name range. Recover the `&` from
        // the def's argument-list source.
        return anonymous_amp_range(def, cx).unwrap_or(name_range);
    }
    // Extend left to the `&` immediately preceding the name.
    match cx.token_before(name_range.start) {
        Some(tok) if cx.raw_source(tok.range) == "&" => Range {
            start: tok.range.start,
            end: name_range.end,
        },
        _ => name_range,
    }
}

/// Locate the trailing `&` of an anonymous block parameter (`def foo(&)`).
/// Scans the def's argument-list tokens for the last lone `&` token.
fn anonymous_amp_range(def: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let args_list = cx.def_arguments(def).get()?;
    let args_range = cx.range(args_list);
    cx.tokens_in(args_range)
        .iter()
        .rev()
        .map(|tok| tok.range)
        .find(|&r| cx.raw_source(r) == "&")
}

/// `use_block_argument_as_local_variable?`: the block-arg name appears as a
/// plain `Lvar`/`Lvasgn` in the body whose parent is NOT a `block_pass`.
fn uses_block_arg_as_local(node: NodeId, block_name: &str, cx: &Cx<'_>) -> bool {
    let Some(body) = cx.def_body(node).get() else {
        return false;
    };
    cx.descendants(body)
        .into_iter()
        .chain(std::iter::once(body))
        .any(|desc| {
            let matches_name = match *cx.kind(desc) {
                NodeKind::Lvar(name) => cx.symbol_str(name) == block_name,
                NodeKind::Lvasgn { name, .. } => cx.symbol_str(name) == block_name,
                _ => false,
            };
            if !matches_name {
                return false;
            }
            // Exclude the `&block` forwarding use itself (parent is block_pass).
            match cx.parent(desc).get() {
                Some(p) => !matches!(*cx.kind(p), NodeKind::BlockPass(_)),
                None => true,
            }
        })
}

/// `invalidates_syntax?`: the `block_pass` node has any block ancestor
/// (`each_ancestor(:any_block).any?`). Matches RuboCop's unbounded walk.
fn block_pass_inside_block(block_pass: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors_of_type(block_pass, "any_block").next().is_some()
}

#[cfg(test)]
mod tests {
    use super::{BlockForwarding, EnforcedStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn explicit_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::Explicit,
            block_forwarding_name: "block".to_string(),
        }
    }

    // --- anonymous style (default) — ground truth: rubocop 1.87.0, JSON
    //     start_column/last_column. ---

    #[test]
    fn flags_signature_and_call_site() {
        // rubocop: 1:9..14 (`&block`), 2:7..12 (`&block`).
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(&block)
                    ^^^^^^ Use anonymous block forwarding.
              bar(&block)
                  ^^^^^^ Use anonymous block forwarding.
            end
        "#});
    }

    #[test]
    fn flags_signature_only_when_no_call_site() {
        // `def foo(&block); end` — signature fires alone (1:9..14).
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(&block)
                    ^^^^^^ Use anonymous block forwarding.
            end
        "#});
    }

    #[test]
    fn flags_multiple_call_sites() {
        // rubocop: 1:9..14, 2:7..12, 3:7..12.
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(&block)
                    ^^^^^^ Use anonymous block forwarding.
              bar(&block)
                  ^^^^^^ Use anonymous block forwarding.
              baz(&block)
                  ^^^^^^ Use anonymous block forwarding.
            end
        "#});
    }

    #[test]
    fn flags_singleton_method() {
        // `def self.foo(&block)`: signature at 1:14..19, call at 2:7..12.
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def self.foo(&block)
                         ^^^^^^ Use anonymous block forwarding.
              bar(&block)
                  ^^^^^^ Use anonymous block forwarding.
            end
        "#});
    }

    #[test]
    fn flags_with_leading_positional_args() {
        // `def foo(*args, &block)` — restarg does NOT suppress.
        // signature `&block` at 1:16..21, call at 2:7..12.
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(*args, &block)
                           ^^^^^^ Use anonymous block forwarding.
              bar(&block)
                  ^^^^^^ Use anonymous block forwarding.
            end
        "#});
    }

    #[test]
    fn signature_fires_but_symbol_to_proc_call_excluded() {
        // `bar(&:upcase)` is symbol-to-proc, not a block forward. Only the
        // signature fires (1:9..14).
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(&block)
                    ^^^^^^ Use anonymous block forwarding.
              bar(&:upcase)
            end
        "#});
    }

    #[test]
    fn signature_fires_but_non_matching_forward_excluded() {
        // `bar(&other)` forwards a different variable, not the block arg.
        // rubocop flags only the signature (1:9..14), not the call site.
        test::<BlockForwarding>().expect_offense(indoc! {r#"
            def foo(&block)
                    ^^^^^^ Use anonymous block forwarding.
              other = proc {}
              bar(&other)
            end
        "#});
    }

    // --- anonymous style — exclusions (verified: rubocop produces NO offense). ---

    #[test]
    fn ignores_when_kwarg_present() {
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(k:, &block)
              bar(&block)
            end
        "#});
    }

    #[test]
    fn ignores_when_block_used_as_local_variable() {
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(&block)
              x = block
              bar(&block)
            end
        "#});
    }

    #[test]
    fn ignores_nested_block_forward_target_ruby_le_33() {
        // RuboCop default target (2.7 <= 3.3): a block_pass nested in another
        // block aborts the whole def (invalidates_syntax?). No offense.
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(&block)
              block_method { bar(&block) }
            end
        "#});
    }

    #[test]
    fn ignores_def_when_any_nested_block_pass_present_target_ruby_le_33() {
        // `invalidates_syntax?` runs on EVERY block_pass, even a non-matching
        // nested `&:to_s`. RuboCop (target <= 3.3) aborts the whole def, so
        // neither the signature nor `baz(&block)` is flagged.
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(&block)
              each { bar(&:to_s) }
              baz(&block)
            end
        "#});
    }

    #[test]
    fn ignores_anonymous_signature_under_anonymous_style() {
        // `def foo(&)` already conforms to anonymous style.
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(&)
              bar(&)
            end
        "#});
    }

    #[test]
    fn ignores_def_with_no_block_argument() {
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo(a, b)
              a + b
            end
        "#});
    }

    #[test]
    fn ignores_def_with_no_arguments() {
        test::<BlockForwarding>().expect_no_offenses(indoc! {r#"
            def foo
              42
            end
        "#});
    }

    // --- explicit style. ---

    #[test]
    fn explicit_flags_anonymous_signature_and_call() {
        // rubocop EnforcedStyle: explicit on `def foo(&); bar(&); end`:
        // signature `&` at 1:9..9, call `&` at 2:7..7.
        test::<BlockForwarding>()
            .with_options(&explicit_opts())
            .expect_offense(indoc! {r#"
                def foo(&)
                        ^ Use explicit block forwarding.
                  bar(&)
                      ^ Use explicit block forwarding.
                end
            "#});
    }

    #[test]
    fn explicit_flags_signature_only() {
        test::<BlockForwarding>()
            .with_options(&explicit_opts())
            .expect_offense(indoc! {r#"
                def foo(&)
                        ^ Use explicit block forwarding.
                end
            "#});
    }

    #[test]
    fn explicit_ignores_named_block_argument() {
        // `def foo(&block)` already conforms to explicit style.
        test::<BlockForwarding>()
            .with_options(&explicit_opts())
            .expect_no_offenses(indoc! {r#"
                def foo(&block)
                  bar(&block)
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(BlockForwarding);
