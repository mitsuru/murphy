//! `Style/RescueStandardError` — flags inconsistent use of bare `rescue` vs
//! `rescue StandardError`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RescueStandardError
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: explicit (default) -- flags bare `rescue` (no exception
//!   class), suggesting `rescue StandardError`.
//!   EnforcedStyle: implicit -- flags `rescue StandardError` when it is the
//!   sole exception class, suggesting bare `rescue`.
//!
//!   The modifier rescue form (`foo rescue bar`) is NOT flagged. The
//!   containing `Rescue` node is detected as modifier-form by the absence
//!   of an `end` keyword token, and the `Resbody` check is skipped.
//!
//!   Implicit style: only flags when there is exactly ONE exception class
//!   and it is a bare `StandardError` constant (scope == None). Multi-class
//!   rescues (`rescue StandardError, OtherError`) and namespaced constants
//!   (`rescue Module::StandardError`) are not flagged.
//!
//!   `::StandardError` (cbase scope): Murphy's AST translator renders
//!   `::StandardError` as `Const { scope: None, name: :StandardError }` --
//!   same as bare `StandardError` -- so both are treated identically.
//!
//!   Gaps:
//!     - `rescue => e` (empty exception list, variable binding) is flagged
//!       under the explicit style, matching RuboCop behaviour.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RescueStandardError;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "explicit")]
    Explicit,
    #[option(value = "implicit")]
    Implicit,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "explicit",
        description = "When `explicit`, require `rescue StandardError` instead of bare `rescue`. When `implicit`, require bare `rescue` instead of `rescue StandardError`."
    )]
    pub enforced_style: EnforcedStyle,
}

const MSG_EXPLICIT: &str = "Avoid rescuing without specifying an error class.";
const MSG_IMPLICIT: &str = "Omit the error class when rescuing `StandardError` by itself.";

#[cop(
    name = "Style/RescueStandardError",
    description = "Checks for rescuing `StandardError` explicitly vs implicitly.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl RescueStandardError {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let NodeKind::Resbody { exceptions, .. } = *cx.kind(node) else {
            return;
        };

        // Skip modifier-form rescue (`foo rescue bar`).
        // The containing Rescue node has no `end` keyword for modifier-form.
        if let Some(parent) = cx.parent(node).get()
            && matches!(cx.kind(parent), NodeKind::Rescue { .. })
                && cx.loc(parent).end_keyword() == Range::ZERO {
                    return;
                }

        let exception_list = cx.list(exceptions);

        match opts.enforced_style {
            EnforcedStyle::Explicit => {
                // Flag bare rescue: no exception classes specified.
                if exception_list.is_empty() {
                    let kw_range = rescue_keyword_range(node, cx);
                    cx.emit_offense(kw_range, MSG_EXPLICIT, None);
                    // Autocorrect: insert ` StandardError` after the rescue keyword.
                    let insert_pos = Range { start: kw_range.end, end: kw_range.end };
                    cx.emit_edit(insert_pos, " StandardError");
                }
            }
            EnforcedStyle::Implicit => {
                // Flag `rescue StandardError` when it is the sole exception class.
                if exception_list.len() == 1 {
                    let exc_id = exception_list[0];
                    if cx.is_global_const(exc_id, "StandardError") {
                        // Offense range: from rescue keyword start to end of
                        // the StandardError constant (inclusive).
                        let kw_range = rescue_keyword_range(node, cx);
                        let exc_range = cx.range(exc_id);
                        let offense_range = Range { start: kw_range.start, end: exc_range.end };
                        cx.emit_offense(offense_range, MSG_IMPLICIT, None);
                        // Autocorrect: remove from keyword-end to const-end
                        // (the " StandardError" suffix).
                        let remove_range = Range { start: kw_range.end, end: exc_range.end };
                        cx.emit_edit(remove_range, "");
                    }
                }
            }
        }
    }
}

/// Get the range of the `rescue` keyword token for a `Resbody` node.
///
/// The resbody's expression starts at `rescue`. We use `token_after` on the
/// node's start offset to find the `rescue` token (which is `SourceTokenKind::Other`
/// with text "rescue").
fn rescue_keyword_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_start = cx.range(node).start;
    if let Some(tok) = cx.token_after(node_start) {
        // The first token at/after node_start should be `rescue`.
        // Confirm the token text for safety.
        let source = cx.source().as_bytes();
        let tok_bytes = &source[tok.range.start as usize..tok.range.end as usize];
        if tok_bytes == b"rescue" {
            return tok.range;
        }
    }
    // Fallback: construct a 6-byte range from node_start (len("rescue") == 6).
    Range { start: node_start, end: node_start + 6 }
}



#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Options, RescueStandardError};
    use murphy_plugin_api::test_support::{indoc, test};

    fn implicit_opts() -> Options {
        Options { enforced_style: EnforcedStyle::Implicit }
    }

    // -------------------------------------------------------------------------
    // EnforcedStyle: explicit (default) -- flags bare rescue
    // -------------------------------------------------------------------------

    #[test]
    fn flags_bare_rescue_explicit() {
        test::<RescueStandardError>().expect_offense(indoc! {"
            begin
              foo
            rescue
            ^^^^^^ Avoid rescuing without specifying an error class.
              bar
            end
        "});
    }

    #[test]
    fn no_offense_rescue_standard_error_explicit() {
        test::<RescueStandardError>().expect_no_offenses(indoc! {"
            begin
              foo
            rescue StandardError
              bar
            end
        "});
    }

    #[test]
    fn no_offense_rescue_other_error_explicit() {
        test::<RescueStandardError>().expect_no_offenses(indoc! {"
            begin
              foo
            rescue OtherError
              bar
            end
        "});
    }

    #[test]
    fn no_offense_rescue_multiple_errors_explicit() {
        test::<RescueStandardError>().expect_no_offenses(indoc! {"
            begin
              foo
            rescue StandardError, SecurityError
              bar
            end
        "});
    }

    #[test]
    fn corrects_bare_rescue_to_rescue_standard_error() {
        test::<RescueStandardError>().expect_correction(
            indoc! {"
                begin
                  foo
                rescue
                ^^^^^^ Avoid rescuing without specifying an error class.
                  bar
                end
            "},
            indoc! {"
                begin
                  foo
                rescue StandardError
                  bar
                end
            "},
        );
    }

    #[test]
    fn flags_bare_rescue_with_binding_explicit() {
        // `rescue => e` is also bare rescue (no exception class).
        test::<RescueStandardError>().expect_offense(indoc! {"
            begin
              foo
            rescue => e
            ^^^^^^ Avoid rescuing without specifying an error class.
              bar
            end
        "});
    }

    // -------------------------------------------------------------------------
    // EnforcedStyle: implicit -- flags `rescue StandardError`
    // -------------------------------------------------------------------------

    #[test]
    fn flags_rescue_standard_error_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_offense(indoc! {"
                begin
                  foo
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Omit the error class when rescuing `StandardError` by itself.
                  bar
                end
            "});
    }

    #[test]
    fn no_offense_bare_rescue_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_no_offenses(indoc! {"
                begin
                  foo
                rescue
                  bar
                end
            "});
    }

    #[test]
    fn no_offense_rescue_other_error_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_no_offenses(indoc! {"
                begin
                  foo
                rescue OtherError
                  bar
                end
            "});
    }

    #[test]
    fn no_offense_rescue_multiple_errors_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_no_offenses(indoc! {"
                begin
                  foo
                rescue StandardError, SecurityError
                  bar
                end
            "});
    }

    #[test]
    fn corrects_rescue_standard_error_to_bare_rescue() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_correction(
                indoc! {"
                    begin
                      foo
                    rescue StandardError
                    ^^^^^^^^^^^^^^^^^^^^ Omit the error class when rescuing `StandardError` by itself.
                      bar
                    end
                "},
                indoc! {"
                    begin
                      foo
                    rescue
                      bar
                    end
                "},
            );
    }

    #[test]
    fn flags_rescue_standard_error_in_method_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_offense(indoc! {"
                def foo
                  bar
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Omit the error class when rescuing `StandardError` by itself.
                  baz
                end
            "});
    }

    // -------------------------------------------------------------------------
    // Modifier-form rescue must not fire
    // -------------------------------------------------------------------------

    #[test]
    fn no_offense_rescue_modifier_explicit() {
        // `foo rescue nil` is modifier-form — RescueStandardError must not flag it.
        test::<RescueStandardError>().expect_no_offenses("foo rescue nil\n");
    }

    #[test]
    fn no_offense_rescue_modifier_implicit() {
        test::<RescueStandardError>()
            .with_options(&implicit_opts())
            .expect_no_offenses("foo rescue nil\n");
    }
}

murphy_plugin_api::submit_cop!(RescueStandardError);
