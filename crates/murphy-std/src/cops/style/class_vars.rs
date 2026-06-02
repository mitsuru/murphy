//! `Style/ClassVars` — avoid the use of class variables.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassVars
//! upstream_version_checked: 1.86.2
//! version_added: "0.13"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Handles cvasgn (class variable assignment, @@foo = ...) and send with
//!   class_variable_set. Cvar (read-only access) is deliberately not flagged,
//!   matching RuboCop which only offenses on assignment. Safe-navigation
//!   `a&.class_variable_set(...)` is a csend node and is not flagged, consistent
//!   with RuboCop's RESTRICT_ON_SEND / on_send-only hook.
//!   No autocorrect — matches RuboCop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! class A
//!   @@test = 10
//! end
//!
//! class A
//!   def self.test(name, value)
//!     class_variable_set("@@#{name}", value)
//!   end
//! end
//!
//! class A; end
//! A.class_variable_set(:@@test, 10)
//!
//! # good (not flagged)
//! class A
//!   @test = 10
//! end
//!
//! class A
//!   def test
//!     @@test  # read-only access is allowed
//!   end
//! end
//!
//! class A
//!   def self.test(name)
//!     class_variable_get("@@#{name}")  # read-only access is allowed
//!   end
//! end
//! ```
//!
//! ## No autocorrect
//!
//! There is no safe general replacement for a class variable assignment;
//! the appropriate refactor (class instance variable, `attr_reader`, etc.)
//! depends on context. Matches RuboCop's no-autocorrect stance.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Replace class var %s with a class instance var.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ClassVars;

#[cop(
    name = "Style/ClassVars",
    description = "Avoid the use of class variables.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ClassVars {
    /// Flag class-variable assignments: `@@foo = expr`, `@@foo += expr`, etc.
    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Cvasgn { name, .. } = *cx.kind(node) else {
            return;
        };
        let var_name = cx.symbol_str(name);
        // loc.name is Range::ZERO for Cvasgn (same situation as Gvasgn).
        // Compute the variable name range from the node start and symbol length.
        let node_start = cx.range(node).start;
        let name_range = Range {
            start: node_start,
            end: node_start + var_name.len() as u32,
        };
        let message = MSG.replace("%s", var_name);
        cx.emit_offense(name_range, &message, None);
    }

    /// Flag `class_variable_set(...)` calls (on any receiver or bare).
    #[on_node(kind = "send", methods = ["class_variable_set"])]
    fn check_class_variable_set(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        // Only flag if there is at least one argument (matches RuboCop's
        // `return unless node.first_argument`).
        let arg_list = cx.list(args);
        let Some(&first_arg) = arg_list.first() else {
            return;
        };
        // Use the verbatim source of the first argument as the class var name
        // in the message (e.g. `:@@test`, `"@@test"`, `"@@#{name}"`).
        let arg_source = cx.raw_source(cx.range(first_arg));
        let message = MSG.replace("%s", arg_source);
        cx.emit_offense(cx.range(first_arg), &message, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- cvasgn: class variable assignment ---

    #[test]
    fn flags_cvasgn_inside_class() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            class A
              @@test = 10
              ^^^^^^ Replace class var @@test with a class instance var.
            end
        "#});
    }

    #[test]
    fn flags_cvasgn_op_assign() {
        // @@test += 1 translates to OpAsgn { target: Cvasgn { @@test, None }, ... }
        test::<ClassVars>().expect_offense(indoc! {r#"
            @@test += 1
            ^^^^^^ Replace class var @@test with a class instance var.
        "#});
    }

    #[test]
    fn flags_cvasgn_at_top_level() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            @@top = 1
            ^^^^^ Replace class var @@top with a class instance var.
        "#});
    }

    // --- cvar: read-only access — not flagged ---

    #[test]
    fn accepts_cvar_read_inside_def() {
        test::<ClassVars>().expect_no_offenses(indoc! {r#"
            class A
              def test
                @@test
              end
            end
        "#});
    }

    // --- class_variable_set: bare call ---

    #[test]
    fn flags_class_variable_set_bare_with_sym_arg() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            class_variable_set(:@@test, 10)
                               ^^^^^^^ Replace class var :@@test with a class instance var.
        "#});
    }

    #[test]
    fn flags_class_variable_set_bare_with_string_arg() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            class_variable_set("@@test", 10)
                               ^^^^^^^^ Replace class var "@@test" with a class instance var.
        "#});
    }

    #[test]
    fn flags_class_variable_set_bare_with_interpolated_string() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            class_variable_set("@@#{name}", value)
                               ^^^^^^^^^^^ Replace class var "@@#{name}" with a class instance var.
        "#});
    }

    // --- class_variable_set: with receiver ---

    #[test]
    fn flags_class_variable_set_with_receiver() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            A.class_variable_set(:@@test, 10)
                                 ^^^^^^^ Replace class var :@@test with a class instance var.
        "#});
    }

    #[test]
    fn flags_class_variable_set_inside_self_method() {
        test::<ClassVars>().expect_offense(indoc! {r#"
            class A
              def self.test(name, value)
                class_variable_set("@@#{name}", value)
                                   ^^^^^^^^^^^ Replace class var "@@#{name}" with a class instance var.
              end
            end
        "#});
    }

    // --- class_variable_set with no args — no offense ---

    #[test]
    fn accepts_class_variable_set_no_args() {
        // Bare call with no arguments: should not panic or flag.
        test::<ClassVars>().expect_no_offenses("class_variable_set\n");
    }

    // --- class_variable_get — not flagged ---

    #[test]
    fn accepts_class_variable_get() {
        test::<ClassVars>().expect_no_offenses(indoc! {r#"
            class A
              def self.test(name)
                class_variable_get("@@#{name}")
              end
            end
        "#});
    }

    // --- instance variable assignment — not flagged ---

    #[test]
    fn accepts_ivar_assignment() {
        test::<ClassVars>().expect_no_offenses(indoc! {r#"
            class A
              @test = 10
            end
        "#});
    }

    // --- safe-navigation not flagged (csend) ---

    #[test]
    fn accepts_safe_nav_class_variable_set() {
        // a&.class_variable_set is a csend node; on_send handler does not fire.
        test::<ClassVars>().expect_no_offenses("a&.class_variable_set(:@@test, 10)\n");
    }
}

murphy_plugin_api::submit_cop!(ClassVars);
