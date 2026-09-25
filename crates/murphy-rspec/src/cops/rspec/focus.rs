//! `RSpec/Focus` — flags focused specs (`fit`, `fdescribe`, `:focus`, …).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Focus
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `focused_block?` (focused group/example selectors with an
//!   RSpec-or-bare receiver) and `metadata` (bare `:focus` sym arg or
//!   `focus: true` pair inside a focusable selector). Offense ranges mirror
//!   upstream: whole `Send` for focused selectors, the `Sym`/`Pair` node for
//!   metadata. Skips `Def`/`Defs` ancestors and chained sends (receiver-position
//!   sends). Detection is at parity; autocorrect (unfocusing the selector and
//!   removing the metadata) is intentionally not ported in this batch — same
//!   convention as `Bundler/OrderedGems` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` (bare dispatch + in-body gating; the allow-list spans
//! focused + focusable selectors and is documented below rather than encoded
//! as a `methods = [...]` filter to keep the file readable):
//!
//! Focused selectors (whole `Send` flagged):
//! - groups: `fdescribe`, `fcontext`, `ffeature`.
//! - examples: `fit`, `fspecify`, `fexample`, `fscenario`, `focus`.
//!
//! Focus metadata (the `Sym`/`Pair` flagged):
//! - `describe 'x', :focus do ... end` — bare `:focus` sym.
//! - `it 'x', focus: true do ... end` — `focus: true` pair in trailing hash.
//! - Focusable hosts: regular/skipped/pending example groups + examples +
//!   shared groups (`describe`, `context`, `feature`, `example_group`,
//!   `xdescribe`, …, `it`, `specify`, `example`, `scenario`, `its`,
//!   `xit`, …, `skip`, `pending`, `shared_examples`,
//!   `shared_examples_for`, `shared_context`).
//!
//! ## No autocorrect
//!
//! Upstream removes the focus (`fdescribe` → `describe`, drop `:focus`).
//! This batch reports only; the edit needs range-with-comma handling
//! (`RangeHelp`) that has no Murphy equivalent yet.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Focus;

#[cop(
    name = "RSpec/Focus",
    description = "Checks if examples are focused.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Focus {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if is_chained_send(cx, node) {
            return;
        }
        if inside_def(cx, node) {
            return;
        }
        let name = cx.symbol_str(method);
        if is_focused_selector(name) {
            cx.emit_offense(send_without_block_range(cx, node), "Focused spec found.", None);
            return;
        }
        if !is_focusable_selector(name) {
            return;
        }
        check_focus_metadata(cx, node);
    }
}

/// `true` for `ExampleGroups.focused` + `Examples.focused`:
/// `fdescribe`, `fcontext`, `ffeature`, `fit`, `fspecify`, `fexample`,
/// `fscenario`, `focus`.
fn is_focused_selector(name: &str) -> bool {
    matches!(
        name,
        "fdescribe" | "fcontext" | "ffeature" | "fit" | "fspecify" | "fexample" | "fscenario" | "focus"
    )
}

/// `true` for every selector that accepts focus metadata:
/// regular + skipped + pending groups/examples plus shared groups.
/// (Focused selectors are handled separately above.)
fn is_focusable_selector(name: &str) -> bool {
    matches!(
        name,
        // ExampleGroups.regular
        "describe" | "context" | "feature" | "example_group"
        // ExampleGroups.skipped
        | "xdescribe" | "xcontext" | "xfeature"
        // Examples.regular
        | "it" | "specify" | "example" | "scenario" | "its"
        // Examples.skipped
        | "xit" | "xspecify" | "xexample" | "xscenario" | "skip"
        // Examples.pending
        | "pending"
        // SharedGroups.all
        | "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

/// Upstream `node.chained?`: the send is the receiver of another send/csend
/// (e.g. `fit("x").something`). A block wrapper (`fit ... do end`) is not
/// chaining — the send is the `call` of a `Block`.
fn is_chained_send(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Send { receiver, .. } => receiver.get() == Some(node),
        NodeKind::Csend { receiver, .. } => receiver == node,
        _ => false,
    }
}

/// Upstream `node.each_ancestor(:any_def).any?`: skip focused-looking sends
/// inside `def`/`defs` bodies (a domain method named e.g. `focus`, not RSpec).
fn inside_def(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.ancestors(node).any(|a| {
        matches!(
            *cx.kind(a),
            NodeKind::Def { .. } | NodeKind::Defs { .. }
        )
    })
}

/// Flags `:focus` sym args and `focus: true` pairs. Mirrors upstream
/// `metadata` matcher: `(send ... <$(sym :focus) ...>)` plus
/// `(send ... (hash <$(pair (sym :focus) true) ...>))`.
fn check_focus_metadata(cx: &Cx<'_>, send: NodeId) {
    let NodeKind::Send { args, .. } = *cx.kind(send) else {
        return;
    };
    // Silence unused-import parity with peers; OptNodeId is used via receiver checks.
    let _ = OptNodeId::NONE;
    for arg in cx.list(args).iter().copied() {
        match *cx.kind(arg) {
            NodeKind::Sym(sym) if cx.symbol_str(sym) == "focus" => {
                cx.emit_offense(cx.range(arg), "Focused spec found.", None);
            }
            NodeKind::Hash(pairs) => {
                for pair_id in cx.list(pairs).iter().copied() {
                    let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                        continue;
                    };
                    let is_focus_key = matches!(*cx.kind(key), NodeKind::Sym(s) if cx.symbol_str(s) == "focus");
                    if !is_focus_key {
                        continue;
                    }
                    if matches!(*cx.kind(value), NodeKind::True_) {
                        cx.emit_offense(cx.range(pair_id), "Focused spec found.", None);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Focus;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_focused_block_selectors() {
        test::<Focus>().expect_offense(indoc! {r#"
                fdescribe 'test' do; end
                ^^^^^^^^^^^^^^^^ Focused spec found.
            "#});
        test::<Focus>().expect_offense(indoc! {r#"
                fit 'test' do; end
                ^^^^^^^^^^ Focused spec found.
            "#});
        test::<Focus>().expect_offense(indoc! {r#"
                focus 'test' do; end
                ^^^^^^^^^^^^ Focused spec found.
            "#});
    }

    #[test]
    fn flags_rspec_focused_block() {
        test::<Focus>().expect_offense(indoc! {r#"
                RSpec.fdescribe 'test' do; end
                ^^^^^^^^^^^^^^^^^^^^^^ Focused spec found.
            "#});
    }

    #[test]
    fn flags_focus_sym_metadata() {
        test::<Focus>().expect_offense(indoc! {r#"
                describe 'test', :focus do; end
                                 ^^^^^^ Focused spec found.
            "#});
    }

    #[test]
    fn flags_focus_true_metadata() {
        test::<Focus>().expect_offense(indoc! {r#"
                it 'test', focus: true do; end
                           ^^^^^^^^^^^ Focused spec found.
            "#});
    }

    #[test]
    fn flags_shared_group_focus_metadata() {
        test::<Focus>().expect_offense(indoc! {r#"
                shared_examples 'test', focus: true do; end
                                        ^^^^^^^^^^^ Focused spec found.
            "#});
    }

    #[test]
    fn does_not_flag_unfocused() {
        test::<Focus>().expect_no_offenses(indoc! {r#"
                describe 'test' do; end
                it 'test' do; end
                context 'test' do; end
            "#});
    }

    #[test]
    fn does_not_flag_focus_false() {
        test::<Focus>().expect_no_offenses(indoc! {r#"
                it 'test', focus: false do; end
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<Focus>().expect_no_offenses(indoc! {r#"
                obj.fit 'test' do; end
            "#});
    }

    #[test]
    fn does_not_flag_inside_def() {
        test::<Focus>().expect_no_offenses(indoc! {r#"
                def helper
                  fit 'test' do; end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(Focus);
