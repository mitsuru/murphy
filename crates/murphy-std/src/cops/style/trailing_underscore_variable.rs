//! `Style/TrailingUnderscoreVariable` — flags unnecessary trailing underscores
//! at the end of parallel variable assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingUnderscoreVariable
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   AllowNamedUnderscoreVariables: honored (default true — only bare `_` fires;
//!   `_foo` is allowed unless option is false).
//!   Splat-before guard: if any variable before the trailing underscore run is a
//!   splat, the whole offense is suppressed (removing the underscores would produce
//!   a syntax error — e.g. `a, *b, _ = foo` and `*a, b, _ = foo`).
//!   Nested mlhs (e.g. `(a, b), _ = foo`) — nested mlhs children are not checked
//!   (conservative v1 gap; no false positives).
//!   All-underscore collapse (e.g. `_, _ = foo`) — offense emitted for trailing
//!   underscores; the good code strips them all.
//!   Parenthesized mlhs — not implemented (conservative gap).
//! ```
//!
//! ## Matched shape
//!
//! `Masgn` nodes where the mlhs ends with one or more `Lvasgn` nodes whose name
//! starts with `_`, after skipping any trailing bare-splat (`Splat(None)`).
//!
//! ## Offense range and message
//!
//! Range: from the start of the first unneeded underscore to the start of the
//! `=` operator. Message embeds the corrected source.
//!
//! ## Autocorrect
//!
//! Delete the offense range.

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, cop};
use serde_json::Value;

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingUnderscoreVariable;

/// Configuration options for `Style/TrailingUnderscoreVariable`.
#[derive(Clone, Debug)]
pub struct TrailingUnderscoreVariableOptions {
    /// When `true` (default), named underscore variables like `_foo` are allowed.
    /// When `false`, any variable starting with `_` in a trailing position fires.
    pub allow_named_underscore_variables: bool,
}

impl Default for TrailingUnderscoreVariableOptions {
    fn default() -> Self {
        Self {
            allow_named_underscore_variables: true,
        }
    }
}

impl CopOptions for TrailingUnderscoreVariableOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let allow = match obj.get("AllowNamedUnderscoreVariables") {
            None => true, // default
            Some(v) => v.as_bool().ok_or_else(|| {
                ConfigError::type_mismatch("AllowNamedUnderscoreVariables", "bool")
            })?,
        };

        Ok(Self {
            allow_named_underscore_variables: allow,
        })
    }

    fn to_config_json(&self) -> String {
        format!(
            r#"{{"AllowNamedUnderscoreVariables":{}}}"#,
            self.allow_named_underscore_variables
        )
    }
}

#[cop(
    name = "Style/TrailingUnderscoreVariable",
    description = "Do not use trailing `_`s in parallel assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingUnderscoreVariableOptions,
)]
impl TrailingUnderscoreVariable {
    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<TrailingUnderscoreVariableOptions>();
        check(node, cx, &opts);
    }
}

/// Returns `true` if `name` is an underscore variable under current options.
fn is_underscore_var(name: &str, allow_named: bool) -> bool {
    name == "_" || (!allow_named && name.starts_with('_'))
}

/// Find the leftmost variable in the trailing run of underscore lvasgn nodes
/// (scanning from the right). Bare splat (`Splat(None)`) nodes are skipped.
/// Returns `(NodeId, index)` of the first (leftmost) offense in the trailing
/// run, or `None` if there is no trailing underscore run.
fn find_first_trailing_offense(
    vars: &[NodeId],
    cx: &Cx<'_>,
    allow_named: bool,
) -> Option<(NodeId, usize)> {
    let mut first_offense: Option<(NodeId, usize)> = None;

    for (rev_i, &var_id) in vars.iter().enumerate().rev() {
        match cx.kind(var_id) {
            NodeKind::Lvasgn { name, .. } => {
                let name_str = cx.symbol_str(*name);
                if is_underscore_var(name_str, allow_named) {
                    first_offense = Some((var_id, rev_i));
                } else {
                    break;
                }
            }
            NodeKind::Splat(inner) if inner.get().is_none() => {
                // bare `*` — skip over it (not an offense itself)
                continue;
            }
            _ => break,
        }
    }

    first_offense
}

fn check(node: NodeId, cx: &Cx<'_>, opts: &TrailingUnderscoreVariableOptions) {
    let NodeKind::Masgn { lhs, rhs } = *cx.kind(node) else {
        return;
    };
    let NodeKind::Mlhs(list) = *cx.kind(lhs) else {
        return;
    };

    let vars = cx.list(list);
    if vars.is_empty() {
        return;
    }

    let Some((first_offense, first_offense_idx)) =
        find_first_trailing_offense(vars, cx, opts.allow_named_underscore_variables)
    else {
        return;
    };

    // Guard: if any var before the trailing run is a splat, skip.
    // e.g. `a, *b, _ = foo` → removing trailing _ would give syntax error.
    let splat_before = vars[..first_offense_idx]
        .iter()
        .any(|&id| matches!(cx.kind(id), NodeKind::Splat(_)));
    if splat_before {
        return;
    }

    let first_offense_start = cx.range(first_offense).start;
    let rhs_start = cx.range(rhs).start;

    // `unused_variables_only?`: when the offense is the very first variable
    // (i.e., ALL variables in the mlhs are unneeded underscores), the
    // autocorrect should replace the entire `lhs = ` with just the rhs value.
    // Example: `_, _ = foo` → `foo`.
    // For partial cases like `a, b, _ = foo`, the offense range is from the
    // first underscore to the `=` operator, leaving `a, b, = foo`.
    let all_underscores = first_offense_idx == 0;

    let (offense_range, good_code) = if all_underscores {
        // Delete the entire LHS + `=` + leading space before rhs.
        // offense_range = [lhs_start, rhs_start)
        let lhs_start = cx.range(lhs).start;
        let offense_range = Range {
            start: lhs_start,
            end: rhs_start,
        };
        let rhs_src = cx.raw_source(cx.range(rhs));
        let good_code = rhs_src.to_string();
        (offense_range, good_code)
    } else {
        // Find the `=` operator token between the last lhs var and rhs.
        let eq_start = find_eq_token_start(cx, first_offense_start, rhs_start).unwrap_or(rhs_start);

        let offense_range = Range {
            start: first_offense_start,
            end: eq_start,
        };

        // Build the "good code" message by deleting offense range from full source.
        let node_range = cx.range(node);
        let full_src = cx.raw_source(node_range);
        let offset = (first_offense_start - node_range.start) as usize;
        let len = (eq_start - first_offense_start) as usize;
        let mut good_code = full_src.to_string();
        if offset + len <= good_code.len() {
            good_code.replace_range(offset..offset + len, "");
        }
        (offense_range, good_code)
    };

    let msg = format!("Do not use trailing `_`s in parallel assignment. Prefer `{good_code}`.");

    cx.emit_offense(offense_range, &msg, None);
    cx.emit_edit(offense_range, "");
}

/// Find the start offset of the `=` assignment operator between `search_start`
/// and `rhs_start` by scanning tokens.
fn find_eq_token_start(cx: &Cx<'_>, search_start: u32, rhs_start: u32) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < rhs_start)
        .find(|t| &source[t.range.start as usize..t.range.end as usize] == b"=")
        .map(|t| t.range.start)
}

#[cfg(test)]
mod tests {
    use super::{TrailingUnderscoreVariable, TrailingUnderscoreVariableOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense: single trailing `_` ---

    #[test]
    fn flags_single_trailing_underscore() {
        test::<TrailingUnderscoreVariable>().expect_offense(indoc! {r#"
            a, b, _ = foo
                  ^^ Do not use trailing `_`s in parallel assignment. Prefer `a, b, = foo`.
        "#});
    }

    #[test]
    fn flags_two_trailing_underscores() {
        test::<TrailingUnderscoreVariable>().expect_offense(indoc! {r#"
            a, _, _ = foo
               ^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `a, = foo`.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_trailing_underscore() {
        test::<TrailingUnderscoreVariable>().expect_correction(
            indoc! {r#"
                a, b, _ = foo
                      ^^ Do not use trailing `_`s in parallel assignment. Prefer `a, b, = foo`.
            "#},
            "a, b, = foo\n",
        );
    }

    #[test]
    fn corrects_two_trailing_underscores() {
        test::<TrailingUnderscoreVariable>().expect_correction(
            indoc! {r#"
                a, _, _ = foo
                   ^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `a, = foo`.
            "#},
            "a, = foo\n",
        );
    }

    // --- All underscores case: offense deletes entire LHS = ---

    #[test]
    fn flags_all_underscores() {
        test::<TrailingUnderscoreVariable>().expect_offense(indoc! {r#"
            _, _ = foo
            ^^^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `foo`.
        "#});
    }

    #[test]
    fn corrects_all_underscores() {
        test::<TrailingUnderscoreVariable>().expect_correction(
            indoc! {r#"
                _, _ = foo
                ^^^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `foo`.
            "#},
            "foo
",
        );
    }

    // --- No offense: splat before underscore ---

    #[test]
    fn no_offense_splat_before_trailing_underscore() {
        test::<TrailingUnderscoreVariable>().expect_no_offenses("a, *b, _ = foo\n");
    }

    #[test]
    fn no_offense_leading_splat_before_trailing_underscore() {
        test::<TrailingUnderscoreVariable>().expect_no_offenses("*a, b, _ = foo\n");
    }

    // --- No offense: named underscore allowed by default ---

    #[test]
    fn no_offense_named_underscore_variable_by_default() {
        test::<TrailingUnderscoreVariable>().expect_no_offenses("a, b, _something = foo\n");
    }

    // --- No offense: no trailing underscore ---

    #[test]
    fn no_offense_no_trailing_underscore() {
        test::<TrailingUnderscoreVariable>().expect_no_offenses("a, b, c = foo\n");
    }

    #[test]
    fn no_offense_underscore_not_at_end() {
        test::<TrailingUnderscoreVariable>().expect_no_offenses("a, _, b = foo\n");
    }

    // --- AllowNamedUnderscoreVariables: false ---

    #[test]
    fn flags_named_underscore_when_option_false() {
        test::<TrailingUnderscoreVariable>()
            .with_options(&TrailingUnderscoreVariableOptions {
                allow_named_underscore_variables: false,
            })
            .expect_offense(indoc! {r#"
                a, b, _something = foo
                      ^^^^^^^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `a, b, = foo`.
            "#});
    }

    #[test]
    fn corrects_named_underscore_when_option_false() {
        test::<TrailingUnderscoreVariable>()
            .with_options(&TrailingUnderscoreVariableOptions {
                allow_named_underscore_variables: false,
            })
            .expect_correction(
                indoc! {r#"
                    a, b, _something = foo
                          ^^^^^^^^^^^ Do not use trailing `_`s in parallel assignment. Prefer `a, b, = foo`.
                "#},
                "a, b, = foo\n",
            );
    }

    // --- CopOptions from_config_json tests ---

    #[test]
    fn config_parse_error() {
        let err =
            <TrailingUnderscoreVariableOptions as murphy_plugin_api::CopOptions>::from_config_json(
                b"not json",
            )
            .expect_err("invalid json");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::Parse { .. }
        ));
    }

    #[test]
    fn config_not_object_error() {
        let err =
            <TrailingUnderscoreVariableOptions as murphy_plugin_api::CopOptions>::from_config_json(
                b"true",
            )
            .expect_err("not an object");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::NotAnObject
        ));
    }

    #[test]
    fn config_type_mismatch_error() {
        let err =
            <TrailingUnderscoreVariableOptions as murphy_plugin_api::CopOptions>::from_config_json(
                br#"{"AllowNamedUnderscoreVariables":"yes"}"#,
            )
            .expect_err("string is not bool");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowNamedUnderscoreVariables");
        assert_eq!(*expected, "bool");
    }
}

murphy_plugin_api::submit_cop!(TrailingUnderscoreVariable);
