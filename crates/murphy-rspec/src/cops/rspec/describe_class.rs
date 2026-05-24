//! `RSpec/DescribeClass` — the first argument of an `RSpec.describe`
//! (or top-level `describe`) block should be the class or module under
//! test, not a string or symbol. Mirrors RuboCop-RSpec's cop of the
//! same name.
//!
//! ## Matched shapes
//!
//! Dispatched on `NodeKind::Send` via `#[on_node(kind = "send")]` and
//! gates on:
//!
//! - **method == `describe`**.
//! - **receiver** is either:
//!     - `OptNodeId::NONE` — bare `describe "x"` (RSpec's top-level
//!       monkey-patch in spec files), or
//!     - `Const { scope: None, name: "RSpec" }` — explicit
//!       `RSpec.describe "x"`.
//!
//!   Any other receiver (e.g. `Other.describe "x"`) is intentionally
//!   skipped — it belongs to some other DSL.
//!
//! ## First-argument classification
//!
//! - **`NodeKind::Str` / `NodeKind::Dstr` / `NodeKind::Sym`** → emit.
//!   These are literal forms that always describe a non-class subject.
//! - **`NodeKind::Const { .. }`** → OK (single-name `Foo` and scoped
//!   `Foo::Bar` both encode here; nested scope ids are walked
//!   transparently because `Const { scope }` chains).
//! - **Anything else** (variable read, method call, expression, …) →
//!   skip. Static analysis cannot tell whether the runtime value is a
//!   class, and a false-positive on a domain DSL is worse than a
//!   tolerated miss.
//!
//! Empty arg lists are also skipped — `describe` with no positional
//! arguments is some other unrelated DSL.
//!
//! ## No autocorrect
//!
//! Synthesising a class identifier from a free-form string is unsafe
//! (the right class may not exist; the user may genuinely want to
//! describe a scenario rather than a class). The cop reports and lets
//! the user fix by hand.
//!
//! ## Known v1 limitation
//!
//! RuboCop only runs RSpec cops on `*_spec.rb` files; Murphy has no
//! per-cop file-pattern gating yet, so this cop fires on bare
//! `describe "foo"` outside spec files too. Users on non-spec
//! codebases can disable the cop via
//! `[cops.rules."RSpec/DescribeClass"] enabled = false`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribeClass;

#[cop(
    name = "RSpec/DescribeClass",
    description = "The first argument to `describe` should be the class or module under test, not a string.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DescribeClass {
    #[on_node(kind = "send", methods = ["describe"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // The `methods = ["describe"]` filter on `#[on_node]` (murphy-34d)
        // gates dispatch on the method symbol before this body runs;
        // we only reach here for `Send { method == "describe" }`. The
        // `let-else` is still defensive against a future kind-aliasing
        // accident (same posture as `Murphy/NoReceiverPuts`) but is
        // statically unreachable today.
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !receiver_is_rspec_or_bare(cx, receiver) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(first) = arg_ids.first() else {
            return;
        };
        if !first_arg_is_string_like(cx, *first) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "The first argument to describe should be the class or module under test",
            None,
        );
    }
}

/// `true` when `receiver` is the empty-receiver bare-`describe` form or
/// explicit top-level `RSpec`.
fn receiver_is_rspec_or_bare(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true; // bare `describe "x"`
    };
    match *cx.kind(rid) {
        // `RSpec` as a top-level constant (scope == None). A scoped
        // `Other::RSpec` (scope == Some(_)) is some other namespace's
        // RSpec and is skipped.
        NodeKind::Const { scope, name } => {
            scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
        }
        _ => false,
    }
}

/// `true` when the first positional argument is a string-like literal —
/// the exact shape this cop wants to flag.
fn first_arg_is_string_like(cx: &Cx<'_>, arg: NodeId) -> bool {
    matches!(
        cx.kind(arg),
        NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Sym(_)
    )
}
