//! Cop trait + single-pass visitor dispatch (design §4).
//!
//! A [`Cop`] is a **read-only** rule: it inspects AST nodes and pushes
//! [`Offense`]s into a sink. Cops never mutate the AST — the trait only
//! exposes a `&node` and an offense `sink`, deliberately giving no surface
//! for mutation (design §4 "read-only traversal + text-edit suggestions").
//!
//! [`run_cops`] walks the shared immutable AST **once** ([ADR 0001]: one
//! prism parse, one traversal) and, for every node a hook covers, dispatches
//! to *every* cop. The single pass — not re-walking per cop — is the
//! load-bearing performance property.
//!
//! Phase 1 exposes exactly one visitor hook (`on_call_node`); more hooks are
//! added when a cop needs them (YAGNI).

use crate::Offense;
use crate::parse::Ast;
use ruby_prism::Visit;

/// Per-file context handed to a cop on each visit.
///
/// Intentionally minimal (YAGNI): the file path is needed for
/// [`Offense::file`], and the source bytes let a real cop (Task 5) compute
/// and slice byte ranges ([ADR 0001]: offsets index into exactly these bytes).
pub struct CopContext<'a> {
    /// Path of the file being linted (for [`Offense::file`]).
    pub file: &'a str,
    /// The source bytes the AST was parsed from (offense byte offsets index
    /// into exactly these bytes).
    pub source: &'a [u8],
}

/// A read-only lint rule (design §4).
///
/// A cop inspects nodes and pushes [`Offense`]s into `sink`. It is given an
/// immutable borrow of the node and no means to mutate the tree, by design.
///
/// `Send + Sync` (ADR 0002 phase-2 flag) so a cop can be fanned across OS
/// threads for all-core parallel dispatch (design §3; Task 5 wires the actual
/// parallelism). This is the *minimal* bound — just the two auto-markers, no
/// `'static`/`Clone`/etc. — so a future Phase-3 mruby-backed cop wrapper that
/// moves to a worker thread (ADR 0003) can still satisfy it. Phase-1 cops are
/// stateless unit structs and auto-satisfy it with no impl change.
///
/// Phase-3 trap: a Phase-3 mruby cop satisfies this bound only because the
/// wrapper holds `Send + Sync` data (config, script path, `Arc<AstContext>`);
/// the `mrb_state` is created on the per-cop worker thread (ADR 0003) and MUST
/// NOT be stored in a cop struct field — `mrb_state` is not `Sync`, so storing
/// it would silently break this bound (and the ADR 0002 drop-order rule).
pub trait Cop: Send + Sync {
    /// The cop's name (e.g. used for [`Offense::cop_name`]).
    fn name(&self) -> &str;

    /// Called once per call node during the single AST traversal.
    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    );
}

/// Internal visitor that performs the single AST pass and fans every visited
/// node out to every cop.
struct Dispatcher<'a> {
    cops: &'a [Box<dyn Cop>],
    ctx: CopContext<'a>,
    sink: &'a mut Vec<Offense>,
}

impl<'pr> Visit<'pr> for Dispatcher<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // Single pass, all cops per node: every cop sees this node before we
        // move on (no re-walking the tree per cop).
        for cop in self.cops {
            cop.on_call_node(node, &self.ctx, self.sink);
        }
        // REQUIRED: descend into nested calls (e.g. `foo.bar(baz)`); without
        // this only top-level calls are visited (see spikes/prism_poc).
        ruby_prism::visit_call_node(self, node);
    }
}

/// Walk `ast` **once** and dispatch every call node to every cop.
///
/// Read-only: cops only push [`Offense`]s into `sink` (design §4).
pub fn run_cops(ast: &Ast<'_>, file: &str, cops: &[Box<dyn Cop>], sink: &mut Vec<Offense>) {
    let mut dispatcher = Dispatcher {
        cops,
        ctx: CopContext {
            file,
            source: ast.source(),
        },
        sink,
    };
    dispatcher.visit(&ast.root());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::{Range, Severity};

    /// Test-only cop that pushes one trivial offense per call node. It exists
    /// solely to prove dispatch fires once per call node, for every cop.
    #[derive(Default)]
    struct CountingStubCop;

    impl Cop for CountingStubCop {
        fn name(&self) -> &str {
            "Murphy/CountingStub"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            _ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense {
                file: "t.rb".into(),
                cop_name: self.name().into(),
                range: Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                severity: Severity::Warning,
                message: "stub".into(),
            });
        }
    }

    #[test]
    fn dispatch_invokes_cop_per_call_node() {
        // ADR 0001 Ruby semantics: bare `foo`, `bar` parse as receiver-less
        // CallNodes, so this source has exactly 2 call nodes.
        let ast = parse("foo; bar\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 2);
    }

    #[test]
    fn dispatch_fans_every_node_out_to_every_cop() {
        // 2 call nodes (`foo`, `bar`) × 2 cops, one offense each → 4.
        // Fails if multi-cop fan-out regresses to dispatching a single cop.
        let ast = parse("foo; bar\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop), Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 4);
    }

    /// Test-only stub cop "A": one offense per call node at the selector
    /// (`message_loc`) range. Distinct `name()` from [`StubCopB`] so the
    /// Task-2 total-order `cop_name` tiebreak yields a deterministic
    /// interleave. Never compiled into the binary (`#[cfg(test)]` only).
    #[derive(Default)]
    struct StubCopA;

    impl Cop for StubCopA {
        fn name(&self) -> &str {
            "Test/StubA"
        }

        fn on_call_node(
            &self,
            node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            let Some(loc) = node.message_loc() else {
                return;
            };
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range::from_prism_location(&loc),
                Severity::Warning,
                "stub",
            ));
        }
    }

    /// Test-only stub cop "B": identical shape to [`StubCopA`] but a distinct
    /// `name()`, so two-cop fan-out over a multi-call source produces a fully
    /// deterministic aggregated `Vec` (`Test/StubA` < `Test/StubB`).
    #[derive(Default)]
    struct StubCopB;

    impl Cop for StubCopB {
        fn name(&self) -> &str {
            "Test/StubB"
        }

        fn on_call_node(
            &self,
            node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            let Some(loc) = node.message_loc() else {
                return;
            };
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range::from_prism_location(&loc),
                Severity::Warning,
                "stub",
            ));
        }
    }

    #[test]
    fn two_distinct_cops_dispatch_and_total_order_is_deterministic() {
        // SCOPE: this proves SEQUENTIAL input-order independence only — a
        // single-threaded `run_cops` whose output is made deterministic by
        // `aggregate`'s Task-2 total order. It does NOT exercise Task 5's
        // scenario (offenses from multiple files merged across rayon threads),
        // so it is NOT evidence of parallel-dispatch determinism. Task 5
        // (murphy-aom) MUST add its own parallel-dispatch determinism test;
        // this test does not cover that.
        //
        // `foo; bar\n`: `foo` selector = bytes 0..3, `bar` selector = 5..8
        // (ADR 0001 bare-identifier CallNodes). 2 cops × 2 nodes = 4
        // offenses; `aggregate`'s Task-2 total order
        // `(file, start, end, cop_name, message, severity)` makes the
        // combined Vec fully deterministic: per offset, `Test/StubA` sorts
        // before `Test/StubB`.
        let src = "foo; bar\n";
        let ast = parse(src).unwrap();
        let mut sink = Vec::new();
        // ADR 0002 phase-2 flag: `Cop` is `Send + Sync` so cops can be
        // fanned across OS threads (Task 5 parallel dispatch). This static
        // assertion fails to compile until the supertrait bound is added.
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Cop>();

        // Constructed directly (not `::default()`) to match this file's
        // existing `CountingStubCop` convention and clippy's
        // `default_constructed_unit_structs` lint; `#[derive(Default)]` on
        // each stub still satisfies the ADR 0002 forward-flag requirement.
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(StubCopA), Box::new(StubCopB)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        let out = crate::aggregate(sink);

        let foo = Range {
            start_offset: 0,
            end_offset: 3,
        };
        let bar = Range {
            start_offset: 5,
            end_offset: 8,
        };
        let expected = vec![
            Offense::new("t.rb", "Test/StubA", foo, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubB", foo, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubA", bar, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubB", bar, Severity::Warning, "stub"),
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn dispatch_recurses_into_nested_calls() {
        // `foo(bar(baz))` is 3 nested CallNodes; all must be visited.
        // Fails if the `ruby_prism::visit_call_node` recurse line is removed.
        let ast = parse("foo(bar(baz))\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 3);
    }
}
