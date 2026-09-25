//! `RSpec/MultipleMemoizedHelpers` — cap `let` / `subject` helpers per group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MultipleMemoizedHelpers
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`spec_group?`: example and shared
//!   groups, bare or `RSpec` receiver): `all_helpers` (the group's own
//!   in-scope helpers plus every enclosing `Block`'s, per
//!   `ExampleGroup#find_all_in_scope` which recurses through hooks and
//!   plain blocks but stops at nested groups, includes, and examples)
//!   are deduplicated by name (`variable_definition?`: first `Sym` /
//!   `Str` / `Dsym` / `Dstr` argument; nameless definitions collapse to
//!   a single entry exactly like upstream's `nil` capture) and groups
//!   over `Max` (default 5) flag the whole block per
//!   `add_offense(node)` with `Example group has too many memoized
//!   helpers [%<count>d/%<max>d]`. `AllowSubject: true` (default)
//!   counts only `let` / `let!`; `false` also counts `subject` /
//!   `subject!`. Nested over-limit groups each flag (verified vs
//!   3.7.0: outer `[6/5]` plus inner `[7/5]`). No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a spec-group entrypoint:
//!
//! - Six `let`s in one group — flagged (whole block).
//! - Five `let`s — at the limit, clean.
//! - Duplicate names (`let(:foo)` twice) count once — clean.
//! - Inner group sees outer helpers (3 outer + 3 inner flags inner).
//! - `subject { }` plus five `let`s — subjects exempt by default,
//!   clean; with `AllowSubject: false` the group flags.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; extracting helpers needs human
//! judgement about shared setup.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    is_bare_example_block, is_let_node, is_scope_change_block, is_spec_group_call,
    is_subject_block,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleMemoizedHelpers;

#[derive(CopOptions)]
pub struct MultipleMemoizedHelpersOptions {
    #[option(
        name = "Max",
        default = 5,
        description = "Maximum number of memoized helpers per example group."
    )]
    pub max: i64,
    #[option(
        name = "AllowSubject",
        default = true,
        description = "Whether subjects are exempt from the helper count."
    )]
    pub allow_subject: bool,
}

#[cop(
    name = "RSpec/MultipleMemoizedHelpers",
    description = "Checks if example groups contain too many `let` and `subject` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = MultipleMemoizedHelpersOptions,
)]
impl MultipleMemoizedHelpers {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_spec_group_call(cx, call) {
            return;
        }
        let opts = cx.options_or_default::<MultipleMemoizedHelpersOptions>();
        // `all_helpers`: own in-scope helpers plus every enclosing
        // block's, deduplicated by name.
        let mut seen: Vec<Option<String>> = Vec::new();
        for helper in helpers_in_scope(cx, node, opts.allow_subject) {
            let key = helper_key(cx, helper);
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        for ancestor in cx.ancestors(node) {
            if !matches!(*cx.kind(ancestor), NodeKind::Block { .. }) {
                continue;
            }
            for helper in helpers_in_scope(cx, ancestor, opts.allow_subject) {
                let key = helper_key(cx, helper);
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
        }
        let count = seen.len() as i64;
        if count <= opts.max {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            &format!(
                "Example group has too many memoized helpers [{count}/{max}]",
                max = opts.max
            ),
            None,
        );
    }
}

/// In-scope helper nodes for `group`: `let?` nodes (and `subject?`
/// blocks unless `allow_subject`), searched recursively from the
/// group's children and stopping at nested scope changes and examples.
///
/// Mirrors `ExampleGroup#find_all_in_scope` / `#find_all` (matched
/// nodes are returned without descending into them).
fn helpers_in_scope(cx: &Cx<'_>, group: NodeId, allow_subject: bool) -> Vec<NodeId> {
    fn walk(cx: &Cx<'_>, id: NodeId, allow_subject: bool, out: &mut Vec<NodeId>) {
        if is_let_node(cx, id) || (!allow_subject && is_subject_block(cx, id)) {
            out.push(id);
            return;
        }
        if is_scope_change_block(cx, id) || is_bare_example_block(cx, id) {
            return;
        }
        for child in cx.children(id) {
            walk(cx, child, allow_subject, out);
        }
    }

    let mut out = Vec::new();
    for child in cx.children(group) {
        walk(cx, child, allow_subject, &mut out);
    }
    out
}

/// The dedupe key for a helper node: the first-argument name, tagged by
/// literal kind (`:foo` and `"foo"` are distinct upstream nodes).
/// Dynamic names use their source text; missing or non-name first
/// arguments collapse to `None` (upstream's `nil` capture, so two
/// unnamed subjects count once).
///
/// Mirrors upstream `variable_definition?` (`(send nil?
/// {#Subjects.all #Helpers.all} $({sym str dsym dstr} ...) ...)`).
fn helper_key(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    let send = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Send { .. } => id,
        _ => return None,
    };
    let NodeKind::Send { args, .. } = *cx.kind(send) else {
        return None;
    };
    let first = cx.list(args).first().copied()?;
    match *cx.kind(first) {
        NodeKind::Sym(sym) => Some(format!("sym:{}", cx.symbol_str(sym))),
        NodeKind::Str(str_id) => Some(format!("str:{}", cx.string_str(str_id))),
        NodeKind::Dsym(_) | NodeKind::Dstr(_) => {
            Some(format!("dyn:{}", cx.raw_source(cx.range(first))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MultipleMemoizedHelpers, MultipleMemoizedHelpersOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn no_subjects() -> MultipleMemoizedHelpersOptions {
        MultipleMemoizedHelpersOptions {
            max: 5,
            allow_subject: false,
        }
    }

    #[test]
    fn flags_six_lets() {
        test::<MultipleMemoizedHelpers>().expect_offense(indoc! {r#"
                describe Foo do let(:a) { 1 }; let(:b) { 2 }; let(:c) { 3 }; let(:d) { 4 }; let(:e) { 5 }; let(:f) { 6 } end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example group has too many memoized helpers [6/5]
            "#});
    }

    #[test]
    fn ignores_five_lets() {
        test::<MultipleMemoizedHelpers>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(:a) { 1 }
                  let(:b) { 2 }
                  let(:c) { 3 }
                  let(:d) { 4 }
                  let(:e) { 5 }
                end
            "#});
    }

    #[test]
    fn dedupes_repeated_names() {
        // `variable_definition?` uniq: two `let(:foo)` count once.
        test::<MultipleMemoizedHelpers>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(:foo) { 1 }
                  let(:foo) { 2 }
                  let(:bar) { 3 }
                end
            "#});
    }

    #[test]
    fn counts_bang_lets() {
        test::<MultipleMemoizedHelpers>().expect_offense(indoc! {r#"
                describe Foo do let(:a) { 1 }; let!(:b) { 2 }; let(:c) { 3 }; let(:d) { 4 }; let(:e) { 5 }; let(:f) { 6 } end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example group has too many memoized helpers [6/5]
            "#});
    }

    #[test]
    fn ignores_subject_by_default() {
        // `AllowSubject: true` counts only lets (verified vs 3.7.0).
        test::<MultipleMemoizedHelpers>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject { {} }
                  let(:a) { 1 }
                  let(:b) { 2 }
                  let(:c) { 3 }
                  let(:d) { 4 }
                  let(:e) { 5 }
                end
            "#});
    }

    #[test]
    fn counts_subject_when_disallowed() {
        test::<MultipleMemoizedHelpers>()
            .with_options(&no_subjects())
            .expect_offense(indoc! {r#"
                describe Foo do subject { {} }; let(:a) { 1 }; let(:b) { 2 }; let(:c) { 3 }; let(:d) { 4 }; let(:e) { 5 } end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example group has too many memoized helpers [6/5]
            "#});
    }

    #[test]
    fn flags_inner_group_seeing_outer_helpers() {
        // The inner group sees 3 outer + 3 own helpers (verified vs
        // 3.7.0); the outer group stays within its own limit.
        test::<MultipleMemoizedHelpers>().expect_offense(indoc! {r#"
                describe Foo do let(:a) { 1 }; let(:b) { 2 }; let(:c) { 3 }; context 'x' do let(:d) { 4 }; let(:e) { 5 }; let(:f) { 6 } end end
                                                                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example group has too many memoized helpers [6/5]
            "#});
    }

    #[test]
    fn ignores_nested_group_helpers() {
        // Scope changes stop the search: the inner group's lets do not
        // count toward the outer group (outer 2, inner 2 + 3 = 5).
        test::<MultipleMemoizedHelpers>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(:a) { 1 }
                  let(:b) { 2 }
                  context 'when x' do
                    let(:c) { 3 }
                    let(:d) { 4 }
                    let(:e) { 5 }
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MultipleMemoizedHelpers);
