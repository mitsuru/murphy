//! `Style/ParenthesesAroundCondition` — flags superfluous parentheses around
//! the condition of `if`/`unless`/`while`/`until`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ParenthesesAroundCondition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Dispatch is on `if`/`while`/`until` nodes (all subscribable), with
//!   detection based on the condition child. A parenthesized condition is
//!   represented as `NodeKind::Begin` with a `LeftParen` token at `range.start`
//!   (distinguished from `begin...end` via `cops::util::is_parenthesized`).
//!
//!   Implemented guards:
//!   - Ternary: skipped (not a block-form if).
//!   - AllowSafeAssignment (default: true): skipped when the condition is an
//!     assignment node (AST-based, covers all assignment variants).
//!   - AllowInMultilineConditions (default: false): skipped for multiline
//!     conditions when the option is set to true.
//!   - Multiple statements inside parens: detected via AST child count.
//!
//!   Gaps vs RuboCop:
//!   - modifier_op? guard (condition is itself a modifier conditional or
//!     rescue expression) is NOT implemented due to AST opacity.
//!   - Modifier-form `while (cond)` / `x until (cond)` are not checked
//!     (modifier-form control flow has the keyword after the body).
//!     Block-form `while (cond) do ... end` IS checked.
//! ```
//!
//! ## Detection
//!
//! Subscribes to `if` (for both `if` and `unless`) and `while`/`until`.
//! For modifier-form nodes, the cop returns early (no offense).
//! For block-form nodes, it inspects the condition child: if it is
//! `NodeKind::Begin` with a `LeftParen` token at `range.start`, it is a
//! parenthesized condition.
//!
//! ## Autocorrect
//!
//! Remove the outer `(` and `)` from the condition.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};
use crate::cops::util::{is_parenthesized, unwrap_parenthesized};

#[derive(Clone, Debug)]
pub struct ParenthesesAroundConditionOptions {
    /// When `true` (default), allow `if (x = y)` — the parentheses signal
    /// intentional assignment in a condition.
    pub allow_safe_assignment: bool,
    /// When `true`, allow parenthesized multiline conditions. Default: false.
    pub allow_in_multiline_conditions: bool,
}

impl Default for ParenthesesAroundConditionOptions {
    fn default() -> Self {
        Self {
            allow_safe_assignment: true,
            allow_in_multiline_conditions: false,
        }
    }
}

impl CopOptions for ParenthesesAroundConditionOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, murphy_plugin_api::ConfigError> {
        use murphy_plugin_api::ConfigError;
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let allow_safe = if let Some(v) = obj.get("AllowSafeAssignment") {
            v.as_bool()
                .ok_or_else(|| ConfigError::type_mismatch("AllowSafeAssignment", "bool"))?
        } else {
            true
        };
        let allow_multiline = if let Some(v) = obj.get("AllowInMultilineConditions") {
            v.as_bool()
                .ok_or_else(|| ConfigError::type_mismatch("AllowInMultilineConditions", "bool"))?
        } else {
            false
        };
        Ok(Self {
            allow_safe_assignment: allow_safe,
            allow_in_multiline_conditions: allow_multiline,
        })
    }

    fn to_config_json(&self) -> String {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "AllowSafeAssignment".to_string(),
            serde_json::Value::Bool(self.allow_safe_assignment),
        );
        obj.insert(
            "AllowInMultilineConditions".to_string(),
            serde_json::Value::Bool(self.allow_in_multiline_conditions),
        );
        serde_json::Value::Object(obj).to_string()
    }
}

/// Stateless unit struct.
#[derive(Default)]
pub struct ParenthesesAroundCondition;

#[cop(
    name = "Style/ParenthesesAroundCondition",
    description = "Don't use parentheses around the condition of an if/unless/while/until.",
    default_severity = "warning",
    default_enabled = true,
    options = ParenthesesAroundConditionOptions,
)]
impl ParenthesesAroundCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip ternary.
        if cx.is_ternary(node) {
            return;
        }
        // Skip modifier-form (the paren check is for block-form conditions).
        if cx.is_modifier_form(node) {
            return;
        }
        let NodeKind::If { cond, .. } = *cx.kind(node) else {
            return;
        };
        check_condition(node, cond, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.is_modifier_form(node) {
            return;
        }
        let NodeKind::While { cond, .. } = *cx.kind(node) else {
            return;
        };
        check_condition(node, cond, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.is_modifier_form(node) {
            return;
        }
        let NodeKind::Until { cond, .. } = *cx.kind(node) else {
            return;
        };
        check_condition(node, cond, cx);
    }
}

fn check_condition(node: NodeId, cond: NodeId, cx: &Cx<'_>) {
    if !is_parenthesized(cond, cx) {
        return;
    }

    // Multiple statements inside parens change meaning when removed:
    // `(foo; bar)` evaluates `bar`, but `foo; bar` would evaluate `foo`.
    // Detect via the AST child count of the Begin node.
    // INVARIANT: is_parenthesized guarantees NodeKind::Begin — else branch unreachable.
    let NodeKind::Begin(list) = cx.kind(cond) else {
        return;
    };
    if cx.list(*list).len() != 1 {
        return;
    }

    let opts = cx.options_or_default::<ParenthesesAroundConditionOptions>();

    // AllowSafeAssignment: skip when the condition is an assignment node
    // (e.g. `(x = y)`). Detected via AST node kind.
    if opts.allow_safe_assignment {
        let inner = unwrap_parenthesized(cond, cx);
        if is_assignment_node(inner, cx) {
            return;
        }
    }

    // AllowInMultilineConditions: skip if body spans multiple lines.
    if opts.allow_in_multiline_conditions {
        let src = cx.raw_source(cx.range(cond));
        if src.contains('\n') {
            return;
        }
    }

    // Build the message. RuboCop uses `article = kw == 'while' ? 'a' : 'an'`.
    let kw = node_keyword(node, cx);
    let article = if kw == "while" { "a" } else { "an" };
    let message = format!("Don't use parentheses around the condition of {article} `{kw}`.");

    let cond_range = cx.range(cond);
    cx.emit_offense(cond_range, &message, None);

    // Autocorrect: remove the outer `(` and `)`.
    let open_range = Range {
        start: cond_range.start,
        end: cond_range.start + 1,
    };
    let close_range = Range {
        start: cond_range.end - 1,
        end: cond_range.end,
    };
    cx.emit_edit(open_range, "");
    cx.emit_edit(close_range, "");
}

/// Returns the keyword (`"if"`, `"unless"`, `"while"`, `"until"`) for a node.
fn node_keyword(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match cx.kind(node) {
        NodeKind::While { .. } => "while",
        NodeKind::Until { .. } => "until",
        NodeKind::If { .. } => {
            // `if_keyword` reads the actual token text; use it to distinguish
            // `if` from `unless` (both are `NodeKind::If`).
            match cx.if_keyword(node) {
                "unless" => "unless",
                _ => "if",
            }
        }
        _ => "if",
    }
}

/// Returns `true` if `node_id` is an assignment node (any assignment variant).
fn is_assignment_node(node_id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node_id),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
            | NodeKind::OpAsgn { .. }
            | NodeKind::OrAsgn { .. }
            | NodeKind::AndAsgn { .. }
            | NodeKind::Masgn { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::{ParenthesesAroundCondition, ParenthesesAroundConditionOptions};
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::{ConfigErrorKind, CopOptions};

    // ── `if` ─────────────────────────────────────────────────────────────────

    #[test]
    fn flags_if_with_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_offense(indoc! {r#"
            if (x > 10)
               ^^^^^^^^ Don't use parentheses around the condition of an `if`.
              y
            end
        "#});
    }

    #[test]
    fn no_offense_if_without_parens() {
        test::<ParenthesesAroundCondition>().expect_no_offenses(indoc! {"
            if x > 10
              y
            end
        "});
    }

    #[test]
    fn no_offense_begin_end_condition() {
        // `begin...end` produces NodeKind::Begin but NOT is_parenthesized — must not flag.
        // This guards the token-based discriminator: `begin` keyword at range.start, not `(`.
        test::<ParenthesesAroundCondition>().expect_no_offenses(indoc! {"
            if begin x end
              y
            end
        "});
    }

    #[test]
    fn autocorrects_if_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_correction(
            indoc! {r#"
                if (x > 10)
                   ^^^^^^^^ Don't use parentheses around the condition of an `if`.
                  y
                end
            "#},
            indoc! {"
                if x > 10
                  y
                end
            "},
        );
    }

    // ── `unless` ─────────────────────────────────────────────────────────────

    #[test]
    fn flags_unless_with_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_offense(indoc! {r#"
            unless (bar || baz)
                   ^^^^^^^^^^^^ Don't use parentheses around the condition of an `unless`.
              foo
            end
        "#});
    }

    #[test]
    fn autocorrects_unless_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_correction(
            indoc! {r#"
                unless (bar || baz)
                       ^^^^^^^^^^^^ Don't use parentheses around the condition of an `unless`.
                  foo
                end
            "#},
            indoc! {"
                unless bar || baz
                  foo
                end
            "},
        );
    }

    // ── `while` ──────────────────────────────────────────────────────────────

    #[test]
    fn flags_while_with_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_offense(indoc! {r#"
            while (x < 10)
                  ^^^^^^^^ Don't use parentheses around the condition of a `while`.
              x += 1
            end
        "#});
    }

    #[test]
    fn autocorrects_while_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_correction(
            indoc! {r#"
                while (x < 10)
                      ^^^^^^^^ Don't use parentheses around the condition of a `while`.
                  x += 1
                end
            "#},
            indoc! {"
                while x < 10
                  x += 1
                end
            "},
        );
    }

    // ── `until` ──────────────────────────────────────────────────────────────

    #[test]
    fn flags_until_with_paren_condition() {
        test::<ParenthesesAroundCondition>().expect_offense(indoc! {r#"
            until (x >= 10)
                  ^^^^^^^^^ Don't use parentheses around the condition of an `until`.
              x += 1
            end
        "#});
    }

    // ── no-offense cases ─────────────────────────────────────────────────────

    #[test]
    fn no_offense_ternary() {
        test::<ParenthesesAroundCondition>().expect_no_offenses("x = (a > 0) ? b : c\n");
    }

    #[test]
    fn no_offense_modifier_if() {
        test::<ParenthesesAroundCondition>().expect_no_offenses("foo if bar\n");
    }

    #[test]
    fn no_offense_modifier_while() {
        test::<ParenthesesAroundCondition>().expect_no_offenses("x += 1 while x < 10\n");
    }

    // ── AllowSafeAssignment ──────────────────────────────────────────────────

    #[test]
    fn allows_safe_assignment_by_default() {
        test::<ParenthesesAroundCondition>().expect_no_offenses(indoc! {"
            if (x = foo)
              bar
            end
        "});
    }

    #[test]
    fn flags_safe_assignment_when_option_disabled() {
        test::<ParenthesesAroundCondition>()
            .with_options(&ParenthesesAroundConditionOptions {
                allow_safe_assignment: false,
                allow_in_multiline_conditions: false,
            })
            .expect_offense(indoc! {r#"
                if (x = foo)
                   ^^^^^^^^^ Don't use parentheses around the condition of an `if`.
                  bar
                end
            "#});
    }

    #[test]
    fn flags_match_operator_not_treated_as_assignment() {
        // =~ is the match operator, not an assignment; should still be flagged.
        test::<ParenthesesAroundCondition>().expect_offense(indoc! {r#"
            if (name =~ /foo/)
               ^^^^^^^^^^^^^^^ Don't use parentheses around the condition of an `if`.
              bar
            end
        "#});
    }

    // ── AllowInMultilineConditions ────────────────────────────────────────────

    #[test]
    fn flags_multiline_condition_by_default() {
        // Multiline parenthesized conditions are flagged with AllowInMultilineConditions:false.
        // The offense range spans the full condition (multiline); we only verify an offense exists.
        let src = "if (x > 10 &&
   y > 10)
  z
end
";
        let offenses = murphy_plugin_api::test_support::run_cop::<ParenthesesAroundCondition>(src);
        assert!(!offenses.is_empty(), "expected at least one offense for multiline paren condition");
        assert!(offenses[0].message.contains("if"), "message should mention keyword");
    }

    #[test]
    fn allows_multiline_condition_when_option_enabled() {
        test::<ParenthesesAroundCondition>()
            .with_options(&ParenthesesAroundConditionOptions {
                allow_safe_assignment: true,
                allow_in_multiline_conditions: true,
            })
            .expect_no_offenses(indoc! {"
                if (x > 10 &&
                   y > 10)
                  z
                end
            "});
    }

    // ── CopOptions round-trip ─────────────────────────────────────────────────

    #[test]
    fn cop_options_round_trip() {
        let opts = ParenthesesAroundConditionOptions::default();
        let json = opts.to_config_json();
        let decoded = <ParenthesesAroundConditionOptions as CopOptions>::from_config_json(
            json.as_bytes(),
        )
        .expect("round-trip");
        assert_eq!(decoded.allow_safe_assignment, opts.allow_safe_assignment);
        assert_eq!(
            decoded.allow_in_multiline_conditions,
            opts.allow_in_multiline_conditions
        );
    }

    #[test]
    fn cop_options_not_object() {
        let err =
            <ParenthesesAroundConditionOptions as CopOptions>::from_config_json(b"\"bad\"")
                .expect_err("non-object is invalid");
        let ConfigErrorKind::NotAnObject = err.kind() else {
            panic!("expected NotAnObject, got {:?}", err.kind());
        };
    }

    #[test]
    fn cop_options_allow_safe_not_bool() {
        let err = <ParenthesesAroundConditionOptions as CopOptions>::from_config_json(
            br#"{"AllowSafeAssignment":"yes"}"#,
        )
        .expect_err("non-bool is invalid");
        let ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowSafeAssignment");
        assert_eq!(*expected, "bool");
    }
}

murphy_plugin_api::submit_cop!(ParenthesesAroundCondition);
