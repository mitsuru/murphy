//! `Lint/FlipFlop` — flag usage of flip-flop operators.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FlipFlop
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-28xr
//! notes: >
//!   Murphy's AST does not translate FlipFlop nodes from prism
//!   (murphy-translate has no PM_FLIP_FLOP mapping). The cop cannot
//!   dispatch until the translator and NodeKind enum are extended.
//! ```
//!
//! ## Known v1 limitation: no flip-flop node kind
//!
//! `on_iflipflop` / `on_eflipflop` have no Murphy equivalent. The
//! `NodeKind` enum and `murphy-translate` both lack flip-flop support.
//! See the gap issue for tracking.
//!
//! ## Defaults that mirror RuboCop
//!
//! (none — cop emits no offenses in v1)

use murphy_plugin_api::{Cx, NoOptions, cop};

#[derive(Default)]
pub struct FlipFlop;

#[cop(
    name = "Lint/FlipFlop",
    description = "Flag usage of flip-flop operators.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FlipFlop {
    // No-op: flip-flop operators are not translated by the current AST.
    #[on_new_investigation]
    fn noop(&self, _cx: &Cx<'_>) {}
}

murphy_plugin_api::submit_cop!(FlipFlop);
