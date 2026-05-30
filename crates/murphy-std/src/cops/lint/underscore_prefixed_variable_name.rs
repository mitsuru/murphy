//! `Lint/UnderscorePrefixedVariableName` — flag `_`-prefixed local
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnderscorePrefixedVariableName
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers the core case: a `_foo`-prefixed local variable that is both
//!   assigned (via `Lvasgn`) and read (via `Lvar`) in the same file.
//!   Block/method arguments declared as `_foo` but not assigned via
//!   `Lvasgn` are out of scope (no `Lvasgn` node is emitted for them).
//!   Scope-crossing false positives (same `_foo` name in an inner scope
//!   read, outer scope assigned) are unlikely in practice; a future
//!   scope-aware pass can refine if needed.
//! ```
//!
//! variables (`_foo`) that are actually referenced (defeating the
//! "intentionally unused" convention).
//!
//! ## Convention
//!
//! In Ruby, prefixing a local variable with `_` (e.g. `_x`, `_unused`)
//! signals "intentionally unused". If such a variable is actually read,
//! the prefix becomes misleading -- it tells readers the value is not
//! consumed, but the code does consume it. This cop flags that mismatch.
//!
//! ## Matched shapes
//!
//! - Any `Lvasgn(_foo, ...)` where `_foo` is also read via `Lvar(_foo)`
//!   somewhere in the same file.
//! - Bare `_` (single underscore) is excluded -- it is the conventional
//!   "discard" variable and is allowed to be read.
//!
//! ## Out of scope
//!
//! - Method / block parameters declared as `_foo` but not explicitly
//!   assigned. Ruby emits no `Lvasgn` for argument declarations, so
//!   they cannot be distinguished from intentional patterns here.
//!   `Lint/UnusedMethodArgument` / `Lint/UnusedBlockArgument` cover
//!   that surface.
//!
//! ## Note on VarSemanticModel
//!
//! `VarSemanticModel` intentionally skips `_`-prefixed names to support
//! the convention. This cop therefore scans the AST directly for `Lvasgn`
//! and `Lvar` nodes whose name starts with `_` (but is not bare `_`).

use std::collections::{HashMap, HashSet};

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct UnderscorePrefixedVariableName;

#[cop(
    name = "Lint/UnderscorePrefixedVariableName",
    description = "Flag underscore-prefixed local variables that are actually referenced.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnderscorePrefixedVariableName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // Collect Lvasgn and Lvar nodes for _-prefixed variable names.
        // VarSemanticModel skips _-prefixed names by design, so we scan directly.
        //
        // assignments: symbol-id -> list of Lvasgn NodeIds with that name.
        // reads: set of symbol-ids for _-prefixed names that appear as Lvar.
        let mut assignments: HashMap<u32, Vec<NodeId>> = HashMap::new();
        let mut reads: HashSet<u32> = HashSet::new();

        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            match *cx.kind(id) {
                // Only flag assignments that carry a value (not bare Lvasgn
                // targets inside Masgn/OpAsgn, which have value = None).
                NodeKind::Lvasgn { name, value } if value.get().is_some() => {
                    let name_str = cx.symbol_str(name);
                    if name_str.starts_with('_') && name_str != "_" {
                        assignments.entry(name.0).or_default().push(id);
                    }
                }
                NodeKind::Lvar(name) => {
                    let name_str = cx.symbol_str(name);
                    if name_str.starts_with('_') && name_str != "_" {
                        reads.insert(name.0);
                    }
                }
                _ => {}
            }
        }

        // Emit one offense per assignment node whose name is also read.
        for (sym_id, asgn_nodes) in &assignments {
            if reads.contains(sym_id) {
                for &asgn_node in asgn_nodes {
                    if let NodeKind::Lvasgn { name, .. } = *cx.kind(asgn_node) {
                        let name_str = cx.symbol_str(name);
                        // Offense range: just the name (matches RuboCop's behavior).
                        let name_range = var_name_range(cx, asgn_node, name_str);
                        cx.emit_offense(
                            name_range,
                            &format!("Do not use `{name_str}` as a local variable name."),
                            None,
                        );
                    }
                }
            }
        }
    }
}

/// Compute the byte range covering only the variable name inside an `Lvasgn`
/// node. The `Lvasgn` range starts at the name and extends to the end of the
/// value expression; slicing to `name.len()` bytes gives just the identifier.
fn var_name_range(cx: &Cx<'_>, node: NodeId, name: &str) -> Range {
    let start = cx.range(node).start;
    Range {
        start,
        end: start + name.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::UnderscorePrefixedVariableName;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_underscore_variable_that_is_read() {
        test::<UnderscorePrefixedVariableName>().expect_offense(indoc! {r#"
            def foo
              _x = 1
              ^^ Do not use `_x` as a local variable name.
              puts _x
            end
        "#});
    }

    #[test]
    fn no_offense_for_unreferenced_underscore_variable() {
        test::<UnderscorePrefixedVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              _x = 1
            end
        "#});
    }

    #[test]
    fn no_offense_for_bare_underscore_read() {
        test::<UnderscorePrefixedVariableName>().expect_no_offenses("_ = 1\nputs _\n");
    }

    #[test]
    fn flags_block_arg_explicitly_assigned_and_read() {
        // When a _-prefixed variable is explicitly assigned inside a block
        // AND read, it should be flagged.
        test::<UnderscorePrefixedVariableName>().expect_offense(indoc! {r#"
            [1].each do |n|
              _val = n
              ^^^^ Do not use `_val` as a local variable name.
              puts _val
            end
        "#});
    }

    #[test]
    fn no_offense_for_underscore_block_param_not_assigned() {
        // A _-prefixed block param that is only declared (no Lvasgn) is
        // out of scope -- this cop only flags explicit assignments.
        test::<UnderscorePrefixedVariableName>().expect_no_offenses(indoc! {r#"
            [1].each do |_x|
              puts 1
            end
        "#});
    }

    #[test]
    fn flags_multiple_assignments_of_same_underscore_variable() {
        // Both assignment sites are flagged when the name is also read.
        test::<UnderscorePrefixedVariableName>().expect_offense(indoc! {r#"
            _x = 1
            ^^ Do not use `_x` as a local variable name.
            _x = 2
            ^^ Do not use `_x` as a local variable name.
            puts _x
        "#});
    }

    #[test]
    fn no_offense_for_regular_variable() {
        // Variables without underscore prefix are not checked by this cop.
        test::<UnderscorePrefixedVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              x = 1
              puts x
            end
        "#});
    }

    #[test]
    fn flags_longer_underscore_variable() {
        test::<UnderscorePrefixedVariableName>().expect_offense(indoc! {r#"
            def process
              _result = compute
              ^^^^^^^ Do not use `_result` as a local variable name.
              log(_result)
            end
        "#});
    }
}
