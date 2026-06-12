//! `Lint/DuplicateRescueException` — flag an exception class rescued more than
//! once in the same `begin … rescue … end` structure.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateRescueException
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Iterates the `Resbody` branches of a `Rescue` node, tracking exceptions
//!   seen so far in a single set spanning the whole structure (so both
//!   `rescue A; rescue A` across branches and `rescue A, A` within one branch
//!   fire). Exception identity is compared by trimmed `raw_source`, mirroring
//!   the precedent in `Style/IdenticalConditionalBranches`; this is
//!   whitespace-sensitive and a documented divergence from RuboCop's structural
//!   AST node equality (e.g. `Foo::Bar` vs `::Foo::Bar` compare unequal here,
//!   matching RuboCop, while `Foo ::Bar` style whitespace differences would
//!   not — a corner case not exercised in practice).
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct DuplicateRescueException;

#[cop(
    name = "Lint/DuplicateRescueException",
    description = "Checks that there are no repeated exceptions used in rescue expressions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateRescueException {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Rescue { resbodies, .. } = *cx.kind(node) else {
            return;
        };
        // One seen-set across every resbody branch: a duplicate fires whether
        // the repeat is in a later `rescue` clause or the same one.
        let mut seen: HashSet<&str> = HashSet::new();
        for &resbody in cx.list(resbodies) {
            let NodeKind::Resbody { exceptions, .. } = *cx.kind(resbody) else {
                continue;
            };
            for &exception in cx.list(exceptions) {
                let src = cx.raw_source(cx.range(exception));
                if !seen.insert(src) {
                    cx.emit_offense(
                        cx.range(exception),
                        "Duplicate `rescue` exception detected.",
                        None,
                    );
                }
            }
        }
    }
}

murphy_plugin_api::submit_cop!(DuplicateRescueException);

#[cfg(test)]
mod tests {
    use super::DuplicateRescueException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_across_branches() {
        test::<DuplicateRescueException>().expect_offense(indoc! {r#"
            begin
              work
            rescue FooError
              a
            rescue FooError
                   ^^^^^^^^ Duplicate `rescue` exception detected.
              b
            end
        "#});
    }

    #[test]
    fn flags_duplicate_within_one_branch() {
        test::<DuplicateRescueException>().expect_offense(indoc! {r#"
            begin
              work
            rescue FooError, FooError
                             ^^^^^^^^ Duplicate `rescue` exception detected.
              a
            end
        "#});
    }

    #[test]
    fn flags_duplicate_in_mixed_list() {
        test::<DuplicateRescueException>().expect_offense(indoc! {r#"
            begin
              work
            rescue FooError, BarError
              a
            rescue BazError, FooError
                             ^^^^^^^^ Duplicate `rescue` exception detected.
              b
            end
        "#});
    }

    #[test]
    fn accepts_distinct_exceptions() {
        test::<DuplicateRescueException>().expect_no_offenses(indoc! {r#"
            begin
              work
            rescue FooError
              a
            rescue BarError
              b
            end
        "#});
    }

    #[test]
    fn accepts_namespaced_vs_bare() {
        // `Foo::Error` and `Error` are distinct — different source text.
        test::<DuplicateRescueException>().expect_no_offenses(indoc! {r#"
            begin
              work
            rescue Foo::Error
              a
            rescue Error
              b
            end
        "#});
    }

    #[test]
    fn ignores_rescue_modifier() {
        // A modifier rescue (`x rescue y`) has no exception list — no offense.
        test::<DuplicateRescueException>().expect_no_offenses("value = compute rescue nil\n");
    }
}
