//! `Lint/NumberedParameterAssignment` — flags assignment to numbered parameters like `_1`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NumberedParameterAssignment
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/NumberedParameterAssignment. Flags `_1`..`_9`
//!   as reserved for numbered parameters, and `_0` / `_10` / etc. as similar
//!   to numbered parameters. The cop dispatches on `lvasgn` (local variable
//!   assignment) which naturally excludes index-assignment shapes like
//!   `_1[_2] = :value` (those produce `indexasgn`, not `lvasgn`).
//! ```
//!
//! ## Matched shapes
//! - `_1 = value` — assignment to `_1` (numbered parameter)
//! - `_2, _3 = value1, value2` — multi-assignment naming `_2`, `_3`
//!
//! ## Why this shape
//!
//! RuboCop's `on_lvasgn` maps directly to Murphy's `#[on_node(kind = "lvasgn")]`.
//! The cop inspects the variable name and checks whether it matches the
//! `_<digits>` pattern that Ruby reserves for numbered block parameters.
//!
//! ## No autocorrect
//!
//! There is no safe, general-purpose autocorrect — the user must choose a
//! replacement name.

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

#[derive(Default)]
pub struct NumberedParameterAssignment;

/// The range of numbered parameters in Ruby: `_1` through `_9`.
const NUMBERED_PARAMETER_RANGE: std::ops::RangeInclusive<u32> = 1..=9;

#[cop(
    name = "Lint/NumberedParameterAssignment",
    description = "Flags assignment to numbered parameters like `_1`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NumberedParameterAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvasgn { name, .. } = *cx.kind(node) else {
            return;
        };
        let name_str = cx.symbol_str(name);
        let Some(number) = extract_number(name_str) else {
            return;
        };

        let msg = if NUMBERED_PARAMETER_RANGE.contains(&number) {
            format!("`{name_str}` is reserved for numbered parameter; consider another name.")
        } else {
            format!("`{name_str}` is similar to numbered parameter; consider another name.")
        };

        cx.emit_offense(cx.range(node), &msg, None);
    }
}

fn extract_number(name: &str) -> Option<u32> {
    let digits = name.strip_prefix('_')?;
    if digits.is_empty() {
        return None;
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

murphy_plugin_api::submit_cop!(NumberedParameterAssignment);

#[cfg(test)]
mod tests {
    use super::NumberedParameterAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_1_through_9() {
        test::<NumberedParameterAssignment>()
            .expect_offense(indoc! {r#"
                _1 = :value
                ^^^^^^^^^^^ `_1` is reserved for numbered parameter; consider another name.
            "#})
            .expect_offense(indoc! {r#"
                _5 = :value
                ^^^^^^^^^^^ `_5` is reserved for numbered parameter; consider another name.
            "#})
            .expect_offense(indoc! {r#"
                _9 = :value
                ^^^^^^^^^^^ `_9` is reserved for numbered parameter; consider another name.
            "#});
    }

    #[test]
    fn flags_0() {
        test::<NumberedParameterAssignment>().expect_offense(indoc! {r#"
            _0 = :value
            ^^^^^^^^^^^ `_0` is similar to numbered parameter; consider another name.
        "#});
    }

    #[test]
    fn flags_10() {
        test::<NumberedParameterAssignment>().expect_offense(indoc! {r#"
            _10 = :value
            ^^^^^^^^^^^^ `_10` is similar to numbered parameter; consider another name.
        "#});
    }

    #[test]
    fn does_not_flag_non_numbered_parameter() {
        test::<NumberedParameterAssignment>()
            .expect_no_offenses("non_numbered_parameter_name = :value\n");
    }

    #[test]
    fn does_not_flag_index_assignment() {
        test::<NumberedParameterAssignment>()
            .expect_no_offenses("Hash.new { _1[_2] = :value }\n");
    }
}
