//! `Style/DisableCopsWithinSourceCodeDirective` — forbid disabling/enabling cops
//! within source code via inline directives.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DisableCopsWithinSourceCodeDirective
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `# rubocop:disable`, `# rubocop:enable`, `# rubocop:todo`,
//!   `# murphy:disable`, `# murphy:enable`, and `# murphy:todo` comments
//!   that are not exempted by `AllowedCops`. Both `all` and named-cop directives
//!   are flagged. When `AllowedCops` is non-empty, `disable all` / `enable all`
//!   / `todo all` is always disallowed.
//!   Autocorrect removes the disallowed cop names from the directive, or the
//!   entire comment if all cops are disallowed.
//!   Gap vs RuboCop: RuboCop only checks `rubocop:` prefixed directives;
//!   Murphy also checks `murphy:` prefixed directives which are Murphy-specific.
//!   RuboCop's directive parsing uses `DirectiveComment`; Murphy reimplements
//!   this with a byte-level parser.
//! ```
//!
//! ## Matched shapes
//!
//! Any comment that starts with `# rubocop:` or `# murphy:` followed by
//! `disable`, `enable`, or `todo`, and contains cop names that are not in
//! `AllowedCops`.

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

const MSG: &str = "RuboCop disable/enable directives are not permitted.";
const MSG_FOR_COPS: &str = "RuboCop disable/enable directives for %s are not permitted.";

#[derive(Default)]
pub struct DisableCopsWithinSourceCodeDirective;

#[derive(Default, Debug)]
pub struct DisableCopsOptions {
    pub allowed_cops: Vec<String>,
}

impl CopOptions for DisableCopsOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, murphy_plugin_api::ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(murphy_plugin_api::ConfigError::parse)?;
        let obj = value
            .as_object()
            .ok_or_else(murphy_plugin_api::ConfigError::not_an_object)?;

        let allowed_cops = if let Some(v) = obj.get("AllowedCops") {
            let arr = v
                .as_array()
                .ok_or_else(|| {
                    murphy_plugin_api::ConfigError::type_mismatch("AllowedCops", "array")
                })?;
            let mut result = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                let s = item.as_str().ok_or_else(|| {
                    murphy_plugin_api::ConfigError::type_mismatch(
                        format!("AllowedCops[{i}]"),
                        "string",
                    )
                })?;
                result.push(s.to_owned());
            }
            result
        } else {
            Vec::new()
        };

        Ok(DisableCopsOptions { allowed_cops })
    }

    fn to_config_json(&self) -> String {
        let items: Vec<String> = self
            .allowed_cops
            .iter()
            .map(|s| format!("{s:?}"))
            .collect();
        format!("{{\"AllowedCops\":[{}]}}", items.join(","))
    }
}

#[cop(
    name = "Style/DisableCopsWithinSourceCodeDirective",
    description = "Forbids disabling/enabling cops within source code.",
    default_severity = "warning",
    default_enabled = false,
    options = DisableCopsOptions,
)]
impl DisableCopsWithinSourceCodeDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let options = cx.options_or_default::<DisableCopsOptions>();
        let any_allowed = !options.allowed_cops.is_empty();

        for &comment in cx.comments() {
            let text = cx.raw_source(comment.range);
            let Some(cops) = parse_directive_cops(text) else {
                continue;
            };

            // Determine which cops are disallowed.
            let disallowed: Vec<&str> = cops
                .iter()
                .filter(|cop_name| {
                    !options
                        .allowed_cops
                        .iter()
                        .any(|a| a.as_str() == cop_name.as_str())
                })
                .map(|s| s.as_str())
                .collect();

            if disallowed.is_empty() {
                continue;
            }

            let msg = if any_allowed {
                let names = disallowed
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                MSG_FOR_COPS.replace("%s", &names)
            } else {
                MSG.to_owned()
            };

            cx.emit_offense(comment.range, &msg, None);

            // Autocorrect: remove disallowed cops or the entire comment.
            if cops.len() == disallowed.len() {
                // All cops are disallowed — remove the entire comment line
                // (including the trailing newline) to avoid leaving a blank line.
                let source_bytes = cx.source().as_bytes();
                let line_end = source_bytes.get(comment.range.end as usize).copied();
                let delete_range = Range {
                    start: comment.range.start,
                    end: if line_end == Some(b'\n') {
                        comment.range.end + 1
                    } else {
                        comment.range.end
                    },
                };
                cx.emit_edit(delete_range, "");
            } else {
                // Some cops are allowed — rebuild the directive without disallowed ones.
                let remaining: Vec<&str> = cops
                    .iter()
                    .filter(|cop_name| {
                        options
                            .allowed_cops
                            .iter()
                            .any(|a| a.as_str() == cop_name.as_str())
                    })
                    .map(|s| s.as_str())
                    .collect();
                // Reconstruct the directive prefix from original text.
                let new_text = rebuild_directive(text, &remaining);
                cx.emit_edit(comment.range, &new_text);
            }
        }
    }
}

/// Parses a directive comment and returns the list of cop names.
/// Returns `None` if the comment is not a `rubocop:`/`murphy:` directive.
/// Returns `Some(vec!["all"])` for `disable all`.
/// Returns `Some(vec!["CopA", "CopB"])` for `disable CopA, CopB`.
fn parse_directive_cops(text: &str) -> Option<Vec<String>> {
    let rest = text.strip_prefix('#')?;
    let trimmed = rest.trim_start_matches([' ', '\t']);

    // Check for directive prefix.
    let after_prefix = trimmed
        .strip_prefix("rubocop:")
        .or_else(|| trimmed.strip_prefix("murphy:"))?;

    // Check for action keyword.
    let after_action = after_prefix
        .strip_prefix("disable")
        .or_else(|| after_prefix.strip_prefix("enable"))
        .or_else(|| after_prefix.strip_prefix("todo"))?;

    // Parse cop names: space-separated or comma-separated.
    let cop_list = after_action.trim();
    if cop_list.is_empty() {
        // Bare `disable` with no cops — treat as `all`.
        return Some(vec!["all".to_owned()]);
    }

    let cops: Vec<String> = cop_list
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    if cops.is_empty() {
        Some(vec!["all".to_owned()])
    } else {
        Some(cops)
    }
}

/// Rebuilds the directive with only the allowed cops, preserving the prefix.
fn rebuild_directive(original: &str, remaining: &[&str]) -> String {
    // Extract prefix: `# rubocop:disable` or `# murphy:disable`.
    let rest = original.strip_prefix('#').unwrap_or(original);
    let trimmed = rest.trim_start_matches([' ', '\t']);

    let prefix_end = if let Some(pos) = trimmed
        .find("rubocop:")
        .or_else(|| trimmed.find("murphy:"))
    {
        // Find the end of the action keyword.
        let action_start = pos + trimmed[pos..].find(':').unwrap_or(0) + 1;
        let action_portion = &trimmed[action_start..];
        let action_end = action_start
            + if action_portion.starts_with("disable") {
                7
            } else if action_portion.starts_with("enable") {
                6
            } else if action_portion.starts_with("todo") {
                4
            } else {
                0
            };
        let indentation_len = rest.len() - trimmed.len();
        1 + indentation_len + action_end
    } else {
        // Fallback: just use the original
        original.len()
    };

    let prefix = &original[..prefix_end];
    format!("{prefix} {}", remaining.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{DisableCopsOptions, DisableCopsWithinSourceCodeDirective, parse_directive_cops};
    use murphy_plugin_api::{CopOptions, test_support::{indoc, test}};

    // --- directive parsing ---

    #[test]
    fn parses_rubocop_disable_single_cop() {
        let cops = parse_directive_cops("# rubocop:disable Metrics/AbcSize");
        assert_eq!(cops, Some(vec!["Metrics/AbcSize".to_owned()]));
    }

    #[test]
    fn parses_rubocop_disable_multiple_cops() {
        let cops = parse_directive_cops("# rubocop:disable Metrics/AbcSize, Metrics/MethodLength");
        assert_eq!(
            cops,
            Some(vec![
                "Metrics/AbcSize".to_owned(),
                "Metrics/MethodLength".to_owned()
            ])
        );
    }

    #[test]
    fn parses_rubocop_enable() {
        let cops = parse_directive_cops("# rubocop:enable Metrics/AbcSize");
        assert_eq!(cops, Some(vec!["Metrics/AbcSize".to_owned()]));
    }

    #[test]
    fn parses_murphy_disable() {
        let cops = parse_directive_cops("# murphy:disable Style/Dir");
        assert_eq!(cops, Some(vec!["Style/Dir".to_owned()]));
    }

    #[test]
    fn returns_none_for_regular_comment() {
        let cops = parse_directive_cops("# This is a regular comment");
        assert_eq!(cops, None);
    }

    // --- offense detection ---

    #[test]
    fn flags_rubocop_disable() {
        test::<DisableCopsWithinSourceCodeDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Metrics/AbcSize
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives are not permitted.
            def foo
            end
            # rubocop:enable Metrics/AbcSize
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives are not permitted.
        "#});
    }

    #[test]
    fn flags_murphy_disable() {
        test::<DisableCopsWithinSourceCodeDirective>().expect_offense(indoc! {r#"
            # murphy:disable Style/Dir
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives are not permitted.
        "#});
    }

    #[test]
    fn accepts_no_directives() {
        test::<DisableCopsWithinSourceCodeDirective>()
            .expect_no_offenses("# This is a regular comment\ndef foo\nend\n");
    }

    // --- AllowedCops ---

    #[test]
    fn accepts_allowed_cop() {
        test::<DisableCopsWithinSourceCodeDirective>()
            .with_options(&DisableCopsOptions {
                allowed_cops: vec!["Metrics/AbcSize".to_owned()],
            })
            .expect_no_offenses("# rubocop:disable Metrics/AbcSize\ndef foo\nend\n# rubocop:enable Metrics/AbcSize\n");
    }

    #[test]
    fn flags_non_allowed_cop_with_allowed_cops_configured() {
        test::<DisableCopsWithinSourceCodeDirective>()
            .with_options(&DisableCopsOptions {
                allowed_cops: vec!["Metrics/AbcSize".to_owned()],
            })
            .expect_offense(indoc! {r#"
                # rubocop:disable Metrics/MethodLength
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives for `Metrics/MethodLength` are not permitted.
                def foo
                end
            "#});
    }

    // --- autocorrect tests ---

    #[test]
    fn autocorrects_all_disallowed_single_directive() {
        // When all cops in the directive are disallowed, the entire comment is removed.
        test::<DisableCopsWithinSourceCodeDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable Metrics/AbcSize
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives are not permitted.
                def foo
                end
            "#},
            "def foo
end
",
        );
    }

    #[test]
    fn autocorrects_partial_allowed_cops() {
        // When some cops are allowed, only the disallowed cop names are removed.
        test::<DisableCopsWithinSourceCodeDirective>()
            .with_options(&DisableCopsOptions {
                allowed_cops: vec!["Metrics/AbcSize".to_owned()],
            })
            .expect_correction(
                indoc! {r#"
                    # rubocop:disable Metrics/AbcSize, Metrics/MethodLength
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ RuboCop disable/enable directives for `Metrics/MethodLength` are not permitted.
                    def foo
                    end
                "#},
                "# rubocop:disable Metrics/AbcSize
def foo
end
",
            );
    }

    // --- config error tests ---

    #[test]
    fn config_error_allowed_cops_not_array() {
        let err = <DisableCopsOptions as CopOptions>::from_config_json(
            br#"{"AllowedCops": "Metrics/AbcSize"}"#,
        )
        .expect_err("wrong shape is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowedCops");
        assert_eq!(expected, &"array");
    }
}

murphy_plugin_api::submit_cop!(DisableCopsWithinSourceCodeDirective);
