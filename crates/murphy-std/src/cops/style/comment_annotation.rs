//! `Style/CommentAnnotation` — check annotation keywords are properly formatted.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CommentAnnotation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   RequireColon option supported. Checks annotation keywords
//!   (TODO, FIXME, OPTIMIZE, HACK, REVIEW) are uppercase and properly formatted.
//!   Multiline comment heuristics are a v1 gap.
//!   Autocorrect is a v1 gap.
//! ```

use murphy_plugin_api::{CopOptions, Cx, cop};

const MSG_COLON: &str = "should be all upper case, followed by a colon, and a space.";
const MSG_SPACE: &str = "should be all upper case, followed by a space.";

#[derive(Default)]
pub struct CommentAnnotation;

#[derive(CopOptions)]
pub struct CommentAnnotationOptions {
    #[option(default = true, description = "Require colon after annotation keyword.")]
    pub require_colon: bool,
}

#[cop(
    name = "Style/CommentAnnotation",
    description = "Check annotation keyword formatting.",
    default_severity = "warning",
    default_enabled = true,
    options = CommentAnnotationOptions
)]
impl CommentAnnotation {
    #[on_new_investigation]
    fn check_investigation(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CommentAnnotationOptions>();

        for comment in cx.comments() {
            let src = cx.raw_source(comment.range);
            let text = src.trim();
            if !text.starts_with('#') {
                continue;
            }
            let body = text.trim_start_matches('#').trim();
            for kw in &["TODO", "FIXME", "OPTIMIZE", "HACK", "REVIEW"] {
                if !body.to_uppercase().starts_with(kw) {
                    continue;
                }
                let actual_kw = &body[..kw.len().min(body.len())];
                if *actual_kw != **kw {
                    continue;
                }
                let after_kw = &body[kw.len()..];
                if opts.require_colon {
                    if after_kw.starts_with(": ") {
                        continue;
                    }
                } else {
                    if after_kw.starts_with(' ') {
                        continue;
                    }
                }
                let msg = if opts.require_colon { MSG_COLON } else { MSG_SPACE };
                cx.emit_offense(comment.range, &format!("Annotation keywords like `{}` {}", kw, msg), None);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommentAnnotation, CommentAnnotationOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn colon_style_flags_missing_colon() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_offense(indoc! {"
                # TODO make better
                ^^^^^^^^^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a colon, and a space.
            "});
    }

    #[test]
    fn colon_style_accepts_proper_format() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_no_offenses("# TODO: make better\n");
    }

    #[test]
    fn space_style_flags_colon() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: false,
            })
            .expect_offense(indoc! {"
                # TODO: make better
                ^^^^^^^^^^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a space.
            "});
    }

    #[test]
    fn space_style_accepts_proper_format() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: false,
            })
            .expect_no_offenses("# TODO make better\n");
    }
}
murphy_plugin_api::submit_cop!(CommentAnnotation);
