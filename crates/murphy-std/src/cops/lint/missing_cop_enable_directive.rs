//! `Lint/MissingCopEnableDirective` — require re-enabling disabled cops.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MissingCopEnableDirective
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-3bod
//! notes: >
//!   Mirrors RuboCop's common own-line disable/enable range checks and
//!   MaxRangeSize option. Registry-backed exemptions for cops disabled in
//!   config, department expansion, and rubocop:push/pop stack handling are not
//!   available in the current v1 comment directive surface; those cases are
//!   conservative v1 limitations.
//! ```

use murphy_plugin_api::{CommentDirectiveKind, CommentDirectiveScope, CopOptions, Cx, cop};

#[derive(Default)]
pub struct MissingCopEnableDirective;

#[derive(CopOptions)]
pub struct Options {
    #[option(name = "MaximumRangeSize", default = 2147483647, description = "Maximum disabled range size in lines.")]
    pub max_range_size: i64,
}

#[cop(
    name = "Lint/MissingCopEnableDirective",
    description = "Require rubocop:disable directives to be re-enabled.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MissingCopEnableDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let max_range = cx.options_or_default::<Options>().max_range_size;
        let directives = cx.comment_directives();

        for (idx, directive) in directives.iter().enumerate() {
            if directive.kind != CommentDirectiveKind::Disable
                || directive.scope == CommentDirectiveScope::SameLine
                || !is_rubocop_directive(directive.comment_range, cx)
            {
                continue;
            }
            let Some(cop) = directive.cop else { continue; };
            let enable = directives[idx + 1..].iter().find(|candidate| {
                candidate.kind == CommentDirectiveKind::Enable
                    && candidate.scope != CommentDirectiveScope::SameLine
                    && candidate.cop == Some(cop)
                    && is_rubocop_directive(candidate.comment_range, cx)
            });
            let end = enable.map_or(cx.source().len() as u32, |d| d.line_range.start);
            if acceptable_range(directive.line_range.start, end, max_range, cx.source()) {
                continue;
            }
            let kind = if cop.contains('/') { "cop" } else { "department" };
            let shown = if kind == "department" {
                cop.split('/').next().unwrap_or(cop)
            } else {
                cop
            };
            let message = if max_range >= i64::from(i32::MAX) {
                format!("Re-enable {shown} {kind} with `# rubocop:enable` after disabling it.")
            } else {
                format!(
                    "Re-enable {shown} {kind} within {max_range} lines after disabling it."
                )
            };
            cx.emit_offense(directive.comment_range, &message, None);
        }
    }
}

fn is_rubocop_directive(range: murphy_plugin_api::Range, cx: &Cx<'_>) -> bool {
    cx.raw_source(range).trim_start().starts_with("# rubocop:")
}

fn acceptable_range(start: u32, end: u32, max_range: i64, source: &str) -> bool {
    if max_range >= i64::from(i32::MAX) {
        return end < source.len() as u32;
    }
    let disabled_lines = line_number(end, source).saturating_sub(line_number(start, source));
    i64::from(disabled_lines) <= max_range + 1
}

fn line_number(offset: u32, source: &str) -> u32 {
    let end = (offset as usize).min(source.len());
    source
        .as_bytes()
        .get(..end)
        .unwrap_or_default()
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
}

murphy_plugin_api::submit_cop!(MissingCopEnableDirective);

#[cfg(test)]
mod tests {
    use super::MissingCopEnableDirective;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_cop_disabled_until_eof() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Layout/SpaceAroundOperators
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn accepts_disable_followed_by_enable() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Layout/SpaceAroundOperators
            x =   0
            # rubocop:enable Layout/SpaceAroundOperators
        "#});
    }

    #[test]
    fn flags_department_disabled_until_eof() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Layout
            ^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout department with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn flags_finite_range_exceeded() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_offense(indoc! {r#"
                # rubocop:disable Layout/SpaceAroundOperators
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop within 2 lines after disabling it.
                x =   0
                y = 1
                # Some other code
                # rubocop:enable Layout/SpaceAroundOperators
            "#});
    }

    #[test]
    fn line_number_accepts_non_char_boundary_offsets() {
        assert_eq!(super::line_number(1, "é\n# rubocop:disable Lint/Foo"), 0);
    }

    #[test]
    fn ignores_murphy_disable_directives() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # murphy:disable Lint/Debugger
            debugger
        "#});
    }
}
