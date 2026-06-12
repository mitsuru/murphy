//! `Lint/AmbiguousAssignment` — flag mistyped shorthand assignments such as
//! `x =- y` (probably meant `x -= y`).
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousAssignment
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/AmbiguousAssignment. RuboCop inspects the
//!   two-character window `range_between(operator.end_pos - 1,
//!   rhs.source_range.begin_pos + 1)` and matches it against
//!   MISTAKES = { "=-" => "-=", "=+" => "+=", "=*" => "*=", "=!" => "!=" }.
//!   Murphy's NodeLoc has no operator range, so the window is reconstructed
//!   from the value node: the byte before the value start must be `=` and the
//!   pair `=<first-char-of-value>` is looked up in MISTAKES. A space between
//!   `=` and the RHS (e.g. `x = -y`) yields a non-`=` predecessor byte and is
//!   correctly ignored, matching RuboCop where the window would then be three
//!   characters and never match.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, OptNodeId, Range, cop};

#[derive(Default)]
pub struct AmbiguousAssignment;

/// `(mistaken-window, suggested-operator)` pairs, mirroring RuboCop's MISTAKES.
const MISTAKES: &[(&str, &str)] = &[("=-", "-="), ("=+", "+="), ("=*", "*="), ("=!", "!=")];

#[cop(
    name = "Lint/AmbiguousAssignment",
    description = "Flag mistyped shorthand assignments such as `x =- y`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl AmbiguousAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        self.check(value, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ivasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        self.check(value, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Cvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        self.check(value, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        self.check(value, cx);
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
            return;
        };
        self.check(value, cx);
    }
}

impl AmbiguousAssignment {
    fn check(&self, value: OptNodeId, cx: &Cx<'_>) {
        // `return unless (rhs = node.expression)` — no RHS, no offense.
        let Some(rhs) = value.get() else {
            return;
        };

        let rhs_start = cx.range(rhs).start as usize;
        // Need at least one byte before the RHS to form the `=<rhs[0]>` window.
        if rhs_start == 0 {
            return;
        }

        let source = cx.source().as_bytes();
        // The window is `range_between(operator.end_pos - 1, rhs.begin_pos + 1)`:
        // exactly the byte before the RHS plus the first byte of the RHS.
        // `.get` keeps us panic-free if the RHS starts on a multibyte boundary;
        // every MISTAKES key is two ASCII bytes, so a multibyte RHS never
        // matches anyway.
        let (Some(&before), Some(&first)) = (source.get(rhs_start - 1), source.get(rhs_start)) else {
            return;
        };
        let window = [before, first];

        for (mistake, suggested) in MISTAKES {
            if window[..] == *mistake.as_bytes() {
                let range = Range {
                    start: (rhs_start - 1) as u32,
                    end: (rhs_start + 1) as u32,
                };
                let msg = format!("Suspicious assignment detected. Did you mean `{suggested}`?");
                cx.emit_offense(range, &msg, None);
                return;
            }
        }
    }
}

murphy_plugin_api::submit_cop!(AmbiguousAssignment);

#[cfg(test)]
mod tests {
    use super::AmbiguousAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_minus_mistake() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            x =- y
              ^^ Suspicious assignment detected. Did you mean `-=`?
        "#});
    }

    #[test]
    fn flags_plus_mistake() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            x =+ y
              ^^ Suspicious assignment detected. Did you mean `+=`?
        "#});
    }

    #[test]
    fn flags_times_mistake() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            x =* y
              ^^ Suspicious assignment detected. Did you mean `*=`?
        "#});
    }

    #[test]
    fn flags_bang_mistake() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            x =! y
              ^^ Suspicious assignment detected. Did you mean `!=`?
        "#});
    }

    #[test]
    fn accepts_correct_shorthand() {
        test::<AmbiguousAssignment>().expect_no_offenses("x -= y\n");
    }

    #[test]
    fn accepts_assignment_of_negative() {
        test::<AmbiguousAssignment>().expect_no_offenses("x = -y\n");
    }

    #[test]
    fn accepts_plain_assignment() {
        test::<AmbiguousAssignment>().expect_no_offenses("x = y\n");
    }

    #[test]
    fn flags_instance_variable_assignment() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            @x =- y
               ^^ Suspicious assignment detected. Did you mean `-=`?
        "#});
    }

    #[test]
    fn flags_class_variable_assignment() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            @@x =- y
                ^^ Suspicious assignment detected. Did you mean `-=`?
        "#});
    }

    #[test]
    fn flags_global_variable_assignment() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            $x =- y
               ^^ Suspicious assignment detected. Did you mean `-=`?
        "#});
    }

    #[test]
    fn flags_constant_assignment() {
        test::<AmbiguousAssignment>().expect_offense(indoc! {r#"
            X =- y
              ^^ Suspicious assignment detected. Did you mean `-=`?
        "#});
    }

    #[test]
    fn accepts_assignment_with_no_rhs_in_multiple_assignment() {
        test::<AmbiguousAssignment>().expect_no_offenses("x, y = 1, 2\n");
    }
}
