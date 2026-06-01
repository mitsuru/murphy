//! `Style/RedundantParentheses` — flags parentheses that serve no purpose.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantParentheses
//! upstream_version_checked: 1.86.2
//! status: blocked
//! gap_issues:
//!   - murphy-wojl
//! notes: >
//!   ABI blocker: prism's ParenthesesNode is translated to the opaque `Unknown`
//!   sentinel in Murphy's AST (tag 37). `Unknown` is explicitly excluded from
//!   `#[on_node]` dispatch (see murphy-ast/src/kinds.rs line 78: "tag 37 =
//!   `Unknown` — excluded (fallback sentinel, not matchable)"), so there is no
//!   way to subscribe to parenthesized expressions via the standard cop API.
//!
//!   RuboCop's `RedundantParentheses` anchors on `on_begin(node)` — the `begin`
//!   node represents `(...)` in the parser gem's AST. Murphy's `Begin` node
//!   represents `begin...end` blocks (explicit keyword), not grouping parens.
//!   Grouping parens produce `Unknown` with no accessible inner expression.
//!
//!   A token-based approach using `#[on_new_investigation]` is NOT feasible
//!   because disambiguating grouping parens from method-call parens, array
//!   literals, and multi-line expressions requires the AST context that `Unknown`
//!   deliberately withholds.
//!
//!   Resolution path: translate prism's `ParenthesesNode` to a subscribable
//!   AST node (e.g. `Kwbegin` repurposed, or a new `Parens` kind) with the
//!   inner expression accessible as a child. This is a murphy-core/translator
//!   change, not a cop change.
//! ```

// This file is intentionally empty pending resolution of the translator gap
// documented above. The parity metadata block above is the Phase 4 deliverable
// for murphy-wojl.
