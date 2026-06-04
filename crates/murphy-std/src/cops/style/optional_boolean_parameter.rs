//! `Style/OptionalBooleanParameter` — suggests using keyword arguments instead
//! of positional boolean default arguments in method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OptionalBooleanParameter
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects method definitions (`def`/`defs`) with optional positional
//!   arguments whose default value is `true` or `false`.
//!   Suggests converting `bar = false` to `bar: false` (keyword argument form).
//!   AllowedMethods is supported (Vec<String>); defaults to
//!   ["respond_to_missing?"] matching RuboCop's upstream default.
//!   AllowedPatterns is supported (Vec<String> of regex patterns, unanchored
//!   matching via the shared `cx.matches_any_pattern` helper). Parity gap:
//!   Rust regex does not support some Ruby regex features (look-ahead and
//!   back-references). Invalid patterns are diagnosed once via stderr and
//!   skipped (no exemption, no panic).
//!   No autocorrect (RuboCop marks it unsafe; method signature changes
//!   implicitly change behavior).
//! ```
//!
//! ## Matched shapes
//!
//! Method definitions (`def`/`def self.x`) with optional positional arguments
//! that default to `true` or `false`:
//!
//! ```ruby
//! # bad
//! def some_method(bar = false)
//!   puts bar
//! end
//!
//! # good
//! def some_method(bar: false)
//!   puts bar
//! end
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str =
    "Prefer keyword arguments for arguments with a boolean default value; \
     use `%replacement%` instead of `%original%`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct OptionalBooleanParameter;

/// Options for `Style/OptionalBooleanParameter`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = ["respond_to_missing?"],
        description = "Methods that are allowed to use positional boolean parameters."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns for method names that are allowed to use positional boolean parameters."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Style/OptionalBooleanParameter",
    description = "Prefer keyword arguments for arguments with a boolean default value.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl OptionalBooleanParameter {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }
}

fn check_def(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();

    // Check AllowedMethods and AllowedPatterns.
    if let Some(name) = cx.method_name(node) {
        if opts.allowed_methods.iter().any(|m| m == name) {
            return;
        }
        // AllowedPatterns: unanchored regex match (mirrors RuboCop's
        // `pattern.match?(name)`), via the shared cached helper.
        if cx.matches_any_pattern(name, &opts.allowed_patterns) {
            return;
        }
    }

    let Some(args_id) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return;
    };
    let args = cx.list(list);

    for &arg_id in args {
        let NodeKind::Optarg { name, default } = *cx.kind(arg_id) else {
            continue;
        };
        // Only flag when default value is a boolean literal (true/false).
        if !cx.is_boolean_type(default) {
            continue;
        }

        let arg_source = cx.raw_source(cx.range(arg_id));
        let arg_name = cx.symbol_str(name);
        let default_source = cx.raw_source(cx.range(default));
        let replacement = format!("{arg_name}: {default_source}");
        let msg = MSG
            .replace("%replacement%", &replacement)
            .replace("%original%", arg_source);
        cx.emit_offense(cx.range(arg_id), &msg, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{OptionalBooleanParameter, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Bad cases -----

    #[test]
    fn flags_boolean_false_default() {
        test::<OptionalBooleanParameter>().expect_offense(indoc! {"
            def some_method(bar = false)
                            ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
              puts bar
            end
        "});
    }

    #[test]
    fn flags_boolean_true_default() {
        test::<OptionalBooleanParameter>().expect_offense(indoc! {"
            def some_method(bar = true)
                            ^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: true` instead of `bar = true`.
              puts bar
            end
        "});
    }

    #[test]
    fn flags_multiple_boolean_params() {
        test::<OptionalBooleanParameter>().expect_offense(indoc! {"
            def some_method(foo = true, bar = false)
                            ^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `foo: true` instead of `foo = true`.
                                        ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
            end
        "});
    }

    #[test]
    fn flags_singleton_def_boolean_param() {
        test::<OptionalBooleanParameter>().expect_offense(indoc! {"
            def self.some_method(bar = false)
                                 ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
            end
        "});
    }

    // ----- Good cases -----

    #[test]
    fn accepts_keyword_boolean_argument() {
        test::<OptionalBooleanParameter>().expect_no_offenses(indoc! {"
            def some_method(bar: false)
            end
        "});
    }

    #[test]
    fn accepts_non_boolean_default() {
        test::<OptionalBooleanParameter>().expect_no_offenses(indoc! {"
            def some_method(bar = nil)
            end
        "});
    }

    #[test]
    fn accepts_non_boolean_default_hash() {
        test::<OptionalBooleanParameter>().expect_no_offenses(indoc! {"
            def some_method(options = {})
            end
        "});
    }

    #[test]
    fn accepts_no_arguments() {
        test::<OptionalBooleanParameter>().expect_no_offenses(indoc! {"
            def some_method
            end
        "});
    }

    // ----- respond_to_missing? is allowed by default -----

    #[test]
    fn accepts_respond_to_missing_by_default() {
        test::<OptionalBooleanParameter>().expect_no_offenses(indoc! {"
            def respond_to_missing?(name, include_private = false)
              super
            end
        "});
    }

    // ----- AllowedMethods option -----

    #[test]
    fn accepts_method_in_allowed_list() {
        let opts = Options {
            allowed_methods: vec!["some_method".to_string()],
            allowed_patterns: vec![],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {"
                def some_method(bar = false)
                end
            "});
    }

    #[test]
    fn flags_when_not_in_allowed_list() {
        let opts = Options {
            allowed_methods: vec!["other_method".to_string()],
            allowed_patterns: vec![],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                def some_method(bar = false)
                                ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
                end
            "});
    }

    #[test]
    fn respond_to_missing_flagged_when_removed_from_allowed() {
        // When AllowedMethods is explicitly set, respond_to_missing? is not
        // automatically allowed unless it's in the list.
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec![],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                def respond_to_missing?(name, include_private = false)
                                              ^^^^^^^^^^^^^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `include_private: false` instead of `include_private = false`.
                end
            "});
    }

    // ----- AllowedPatterns option -----

    #[test]
    fn accepts_method_matching_allowed_pattern() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["^respond_to".to_string()],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {"
                def respond_to_missing?(name, include_private = false)
                end
            "});
    }

    #[test]
    fn accepts_method_matching_unanchored_pattern() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["_missing".to_string()],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {"
                def respond_to_missing?(name, include_private = false)
                end
            "});
    }

    #[test]
    fn flags_method_not_matching_pattern() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["^respond_to".to_string()],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                def some_method(bar = false)
                                ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
                end
            "});
    }

    #[test]
    fn skips_invalid_pattern() {
        // An invalid regex pattern is diagnosed (stderr) and skipped: the
        // method gets no exemption but the cop still runs normally (no panic).
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["[invalid".to_string()],
        };
        test::<OptionalBooleanParameter>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                def some_method(bar = false)
                                ^^^^^^^^^^^ Prefer keyword arguments for arguments with a boolean default value; use `bar: false` instead of `bar = false`.
                end
            "});
    }
}
murphy_plugin_api::submit_cop!(OptionalBooleanParameter);
