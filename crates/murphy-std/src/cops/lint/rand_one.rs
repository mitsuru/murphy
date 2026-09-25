//! `Lint/RandOne` — `rand(1)` always returns 0; use `rand(2)` or `rand` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RandOne
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/RandOne. Covers all shapes: rand(1), rand(-1),
//!   rand(1.0), rand(-1.0) with bare, Kernel, and ::Kernel receivers.
//! ```
//!
//! ## Matched shapes
//! - `rand(1)` — rand with integer argument 1
//! - `rand(-1)` — rand with integer argument -1
//! - `rand(1.0)` — rand with float argument 1.0
//! - `rand(-1.0)` — rand with float argument -1.0
//!
//! All forms accept bare `rand`, `Kernel.rand`, or `::Kernel.rand` receivers.
//! Calls on other receivers (e.g. `foo.rand(1)`) are not flagged.
//!
//! ## Autocorrect
//! None. The user must decide the appropriate replacement.
//!

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, def_node_matcher};

// RuboCop parity: `Lint/RandOne` receiver guards are bare (`nil?`) and
// `Kernel` top-level (`(const {nil? cbase} :Kernel)`), method `:rand`.
// In Murphy `::Kernel` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` for `Kernel` (namespaced `Foo::Kernel` still accepts — pinned by
// `boundary_accepts_namespaced_kernel_rand`). `send` covers `Send` only (not
// `Csend`), matching the `Send`-only dispatch below (pinned by
// `boundary_accepts_csend_kernel_rand`). Argument value guards
// (`{(int {-1 1}) (float {-1.0 1.0})}`) stay hand-rolled as source checks
// below (preserving the exact `1`/`-1`/`1.0`/`-1.0` contract).
def_node_matcher!(rand_bare, "(send nil? :rand ...)");
def_node_matcher!(rand_kernel, "(send (const nil? :Kernel) :rand ...)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RandOne;

#[cop(
    name = "Lint/RandOne",
    description = "`rand(1)` always returns 0. Use another range instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RandOne {
    #[on_node(kind = "send", methods = ["rand"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };

        // `(send nil? :rand ...)` (bare) or
        // `(send (const nil? :Kernel) :rand ...)` (`Kernel` / `::Kernel`,
        // top-level only). `send` covers `Send` only (not `Csend`).
        if !rand_bare(node, cx) && !rand_kernel(node, cx) {
            return;
        }

        // Gate: single argument
        let args_list = cx.list(args);
        if args_list.len() != 1 {
            return;
        }

        // Gate: argument is 1, -1, 1.0, or -1.0
        let arg_src = cx.raw_source(cx.range(args_list[0]));
        if !matches!(arg_src, "1" | "-1" | "1.0" | "-1.0") {
            return;
        }

        let node_src = cx.raw_source(cx.range(node));
        let msg =
            format!("`{node_src}` always returns `0`. Perhaps you meant `rand(2)` or `rand`?");
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

murphy_plugin_api::submit_cop!(RandOne);

#[cfg(test)]
mod tests {
    use super::RandOne;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── bare rand with integer args ────────────────────────────────────

    #[test]
    fn flags_rand_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            rand 1
            ^^^^^^ `rand 1` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_rand_paren_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            rand(1)
            ^^^^^^^ `rand(1)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_rand_neg_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            rand(-1)
            ^^^^^^^^ `rand(-1)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    // ── bare rand with float args ──────────────────────────────────────

    #[test]
    fn flags_rand_1_0() {
        test::<RandOne>().expect_offense(indoc! {r#"
            rand(1.0)
            ^^^^^^^^^ `rand(1.0)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_rand_neg_1_0() {
        test::<RandOne>().expect_offense(indoc! {r#"
            rand(-1.0)
            ^^^^^^^^^^ `rand(-1.0)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    // ── Kernel.rand ────────────────────────────────────────────────────

    #[test]
    fn flags_kernel_rand_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            Kernel.rand(1)
            ^^^^^^^^^^^^^^ `Kernel.rand(1)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_kernel_rand_neg_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            Kernel.rand(-1)
            ^^^^^^^^^^^^^^^ `Kernel.rand(-1)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_kernel_rand_1_0() {
        test::<RandOne>().expect_offense(indoc! {r#"
            Kernel.rand 1.0
            ^^^^^^^^^^^^^^^ `Kernel.rand 1.0` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    #[test]
    fn flags_kernel_rand_neg_1_0() {
        test::<RandOne>().expect_offense(indoc! {r#"
            Kernel.rand(-1.0)
            ^^^^^^^^^^^^^^^^^ `Kernel.rand(-1.0)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    // ── ::Kernel.rand ──────────────────────────────────────────────────

    #[test]
    fn flags_cbase_kernel_rand_1() {
        test::<RandOne>().expect_offense(indoc! {r#"
            ::Kernel.rand(1)
            ^^^^^^^^^^^^^^^^ `::Kernel.rand(1)` always returns `0`. Perhaps you meant `rand(2)` or `rand`?
        "#});
    }

    // ── no-offense cases ───────────────────────────────────────────────

    #[test]
    fn accepts_rand_no_args() {
        test::<RandOne>().expect_no_offenses("rand\n");
    }

    #[test]
    fn accepts_rand_2() {
        test::<RandOne>().expect_no_offenses("rand(2)\n");
    }

    #[test]
    fn accepts_rand_range() {
        test::<RandOne>().expect_no_offenses("rand(-1..1)\n");
    }

    #[test]
    fn accepts_kernel_rand_no_args() {
        test::<RandOne>().expect_no_offenses("Kernel.rand\n");
    }

    #[test]
    fn accepts_kernel_rand_2() {
        test::<RandOne>().expect_no_offenses("Kernel.rand 2\n");
    }

    #[test]
    fn accepts_kernel_rand_range() {
        test::<RandOne>().expect_no_offenses("Kernel.rand(-1..1)\n");
    }

    #[test]
    fn accepts_cbase_kernel_rand_no_args() {
        test::<RandOne>().expect_no_offenses("::Kernel.rand\n");
    }

    #[test]
    fn accepts_foo_rand_1() {
        test::<RandOne>().expect_no_offenses("foo.rand(1)\n");
    }

    #[test]
    fn accepts_foo_rand_neg_1() {
        test::<RandOne>().expect_no_offenses("obj.rand(-1)\n");
    }

    // --- Boundary characterization (murphy-ft88.5): pin the exact node set
    // the hand-rolled `Kernel` guard matches, so the verbatim
    // `(send nil? :rand ...)` / `(send (const nil? :Kernel) :rand ...)`
    // refactor can be proven equivalent. `::Kernel` collapses to
    // `Const{scope:None}` in Murphy: `nil?` covers bare + `::` for `Kernel`
    // (namespaced `Foo::Kernel` still accepts — pinned by
    // `boundary_accepts_namespaced_kernel_rand`). `send` covers `Send` only
    // (not `Csend`), matching the `Send`-only dispatch below (pinned by
    // `boundary_accepts_csend_kernel_rand`).

    #[test]
    fn boundary_accepts_namespaced_kernel_rand() {
        // Upstream `(const {nil? cbase} :Kernel)` matches top-level only, so
        // `Foo::Kernel` still accepts (not flagged).
        test::<RandOne>().expect_no_offenses("Foo::Kernel.rand(1)\n");
    }

    #[test]
    fn boundary_accepts_csend_kernel_rand() {
        // `&.` is a `csend` node; `send` covers `Send` only, so it stays
        // accepted (not flagged).
        test::<RandOne>().expect_no_offenses("Kernel&.rand(1)\n");
    }

}
