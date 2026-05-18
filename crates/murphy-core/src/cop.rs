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
/// Phase 2 will add `Send + Sync` for all-core parallel dispatch (design §3); Phase-1 cops are stateless and will satisfy it.
pub trait Cop {
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
