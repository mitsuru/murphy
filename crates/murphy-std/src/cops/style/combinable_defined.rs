//! `Style/CombinableDefined` — combine nested `defined?` calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CombinableDefined
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags multiple `defined?` calls joined by `&&`/`and` where one call's
//!   subject is the *immediate* namespace (const scope) or receiver of another
//!   call's subject, and autocorrects by deleting each redundant `defined?`
//!   term together with its adjacent operator. Detection mirrors RuboCop's
//!   `namespace`/`receiver` (immediate-only) check, so skip-level chains such
//!   as `defined?(Foo) && defined?(Foo::Bar::Baz)` are correctly NOT flagged.
//!   Detection is byte-for-byte matched with RuboCop 1.87.0. On clean
//!   forward-order chains the autocorrect output also matches RuboCop exactly.
//!   On reverse/mixed-order chains (redundant term precedes its namespace),
//!   Murphy emits non-overlapping edits that yield the *correct* combined
//!   result, whereas RuboCop's multi-pass corrector clobbers overlapping
//!   ranges and can leave malformed output (e.g. a missing operator space or
//!   a still-redundant residual term). This divergence is an intentional
//!   improvement, not a detection gap.
//! ```
//!
//! ## Matched shapes
//!
//! A top-level `and` chain whose terms are all `defined?` calls, where at least
//! one term's subject (`const` or `call`) is the immediate namespace/receiver
//! of another term's subject:
//!
//! - `defined?(Foo) && defined?(Foo::Bar)` → `defined?(Foo::Bar)`
//! - `defined?(foo) && defined?(foo.bar)` → `defined?(foo.bar)`
//! - `defined?(Foo) && defined?(Foo::Bar) && defined?(Foo::Bar::Baz)`
//!   → `defined?(Foo::Bar::Baz)`
//!
//! ## Autocorrect
//!
//! Each redundant term is removed surgically: a non-last redundant term plus
//! its trailing operator (`[term.start, next_term.start)`), or the last
//! redundant term plus its preceding operator (`[prev_term.end, term.end)`).
//! Surviving terms and their operators (including the `and` keyword) pass
//! through byte-for-byte.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct CombinableDefined;

/// A `defined?` term in the `and` chain whose subject is a `const`/`call`.
struct Term<'a> {
    /// The whole `defined?(...)` node range.
    range: Range,
    /// The subject's source text (e.g. `Foo::Bar` or `foo.bar`).
    subject_src: &'a str,
    /// The subject's *immediate* namespace/receiver source, if any.
    namespace_src: Option<&'a str>,
}

#[cop(
    name = "Style/CombinableDefined",
    description = "Combine nested `defined?` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CombinableDefined {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        // Only handle the top-level `and` of a chain; nested `and` nodes are
        // visited as part of the outermost one.
        if cx.parent(node).get().is_some_and(|parent| matches!(cx.kind(parent), NodeKind::And { .. })) {
            return;
        }

        // Collect the chain terms in source order. RuboCop requires *all*
        // terms to be `defined?` calls, otherwise the cop does not fire.
        let mut defined_nodes = Vec::new();
        if !collect_defined_terms(node, cx, &mut defined_nodes) {
            return;
        }
        if defined_nodes.len() < 2 {
            return;
        }
        defined_nodes.sort_by_key(|&dn| cx.range(dn).start);

        // Build the subject/namespace view for each `defined?` term whose
        // argument is a `const` or `call` (other subjects are ignored, like
        // RuboCop's `defined_calls` filter).
        let mut terms: Vec<Term<'_>> = Vec::new();
        for &dn in &defined_nodes {
            let NodeKind::Defined(subject) = *cx.kind(dn) else {
                continue;
            };
            let namespace_src = match cx.kind(subject) {
                NodeKind::Const { scope, .. } => {
                    scope.get().map(|s| cx.raw_source(cx.range(s)))
                }
                NodeKind::Send { .. } => {
                    cx.call_receiver(subject).get().map(|r| cx.raw_source(cx.range(r)))
                }
                _ => continue,
            };
            terms.push(Term {
                range: cx.range(dn),
                subject_src: cx.raw_source(cx.range(subject)),
                namespace_src,
            });
        }

        // A term is redundant when its subject is the immediate namespace /
        // receiver of some *other* term's subject.
        let redundant: Vec<usize> = terms
            .iter()
            .enumerate()
            .filter(|&(_, term)| {
                terms.iter().any(|other| {
                    other.namespace_src == Some(term.subject_src)
                })
            })
            .map(|(i, _)| i)
            .collect();

        if redundant.is_empty() {
            return;
        }

        cx.emit_offense(cx.range(node), "Combine nested `defined?` calls.", None);

        // Autocorrect: delete redundant terms with their adjacent operators.
        //
        // Redundant indices are grouped into maximal consecutive runs so that
        // a run abutting the end of the chain (no surviving term after it) is
        // removed together with the *preceding* operator anchored to the last
        // surviving term — never to a term that is itself being deleted. This
        // keeps every emitted edit non-overlapping (overlapping edits are a
        // corruption hazard, not just wrong output).
        let is_redundant = |i: usize| redundant.binary_search(&i).is_ok();
        let mut i = 0;
        while i < terms.len() {
            if !is_redundant(i) {
                i += 1;
                continue;
            }
            // Extend the run [run_start, run_end] of consecutive redundant terms.
            let run_start = i;
            let mut run_end = i;
            while run_end + 1 < terms.len() && is_redundant(run_end + 1) {
                run_end += 1;
            }
            let range = if run_end + 1 < terms.len() {
                // A term survives after the run: remove the run plus the
                // operator that follows it (up to the next term's start).
                Range { start: terms[run_start].range.start, end: terms[run_end + 1].range.start }
            } else {
                // Trailing run: anchor to the end of the last surviving term
                // (which sits at run_start - 1, since run_start > 0 — the chain
                // always retains at least one non-redundant term).
                Range { start: terms[run_start - 1].range.end, end: terms[run_end].range.end }
            };
            cx.emit_edit(range, "");
            i = run_end + 1;
        }
    }
}

/// Walk the `and` chain, pushing each non-`and` term. Returns `false` as soon
/// as a term is not a `defined?` call (RuboCop's all-terms requirement).
fn collect_defined_terms(node: NodeId, cx: &Cx<'_>, out: &mut Vec<NodeId>) -> bool {
    let mut work = vec![node];
    while let Some(current) = work.pop() {
        match cx.kind(current) {
            NodeKind::And { lhs, rhs } => {
                work.push(*lhs);
                work.push(*rhs);
            }
            NodeKind::Defined(_) => out.push(current),
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::CombinableDefined;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_combinable_nested_constants() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo) && defined?(Foo::Bar) && defined?(Foo::Bar::Baz)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar::Baz)\n",
        );
    }

    #[test]
    fn flags_combinable_nested_methods() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(foo) && defined?(foo.bar) && defined?(foo.bar.baz)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(foo.bar.baz)\n",
        );
    }

    #[test]
    fn flags_simple_pair_constants() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo) && defined?(Foo::Bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar)\n",
        );
    }

    #[test]
    fn flags_simple_pair_methods() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(foo) && defined?(foo.bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(foo.bar)\n",
        );
    }

    #[test]
    fn keeps_unrelated_surviving_term() {
        // Only `Foo` is the immediate namespace of `Foo::Bar`; `Baz` is unrelated
        // and survives along with its operator.
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo) && defined?(Foo::Bar) && defined?(Baz)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar) && defined?(Baz)\n",
        );
    }

    #[test]
    fn preserves_and_keyword_operator() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo) && defined?(Foo::Bar) and defined?(Foo::Bar::Baz)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar::Baz)\n",
        );
    }

    #[test]
    fn flags_reverse_order_pair() {
        // Redundant term is the *last* term (`Foo` is the namespace of the
        // earlier `Foo::Bar`).
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo::Bar) && defined?(Foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar)\n",
        );
    }

    #[test]
    fn flags_reverse_order_triple() {
        // Trailing run of two redundant terms; non-overlapping deletion leaves
        // only the deepest subject.
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo::Bar::Baz) && defined?(Foo::Bar) && defined?(Foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar::Baz)\n",
        );
    }

    #[test]
    fn flags_redundant_last_with_unrelated_middle() {
        test::<CombinableDefined>().expect_correction(
            indoc! {"
                defined?(Foo::Bar) && defined?(Baz) && defined?(Foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
            "},
            "defined?(Foo::Bar) && defined?(Baz)\n",
        );
    }

    #[test]
    fn accepts_skip_level_constants() {
        // `Foo` is NOT the immediate namespace of `Foo::Bar::Baz` (that is
        // `Foo::Bar`), so RuboCop does not flag this — neither do we.
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(Foo) && defined?(Foo::Bar::Baz)\n",
        );
    }

    #[test]
    fn accepts_skip_level_methods() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(foo) && defined?(foo.bar.baz)\n",
        );
    }

    #[test]
    fn accepts_single_defined() {
        test::<CombinableDefined>().expect_no_offenses("defined?(Foo)\n");
    }

    #[test]
    fn accepts_unrelated_and() {
        test::<CombinableDefined>().expect_no_offenses("a && b\n");
    }

    #[test]
    fn accepts_non_defined_term_in_chain() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(Foo) && defined?(Foo::Bar) && bar\n",
        );
    }

    #[test]
    fn accepts_different_base_names_constants() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(Foo) && defined?(FooBar)\n",
        );
    }

    #[test]
    fn accepts_different_base_names_methods() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(foo) && defined?(foo_bar)\n",
        );
    }

    #[test]
    fn accepts_unrelated_defineds() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(A) && defined?(B) && defined?(C)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(CombinableDefined);
