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

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

const MSG_COLON: &str = "should be all upper case, followed by a colon, and a space.";
const MSG_SPACE: &str = "should be all upper case, followed by a space.";

#[derive(Default)]
pub struct CommentAnnotation;

#[derive(CopOptions)]
pub struct CommentAnnotationOptions {
    #[option(name = "RequireColon", default = true, description = "Require colon after annotation keyword.")]
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
                // `get(..kw.len())` is char-boundary-safe: it yields `None` when
                // the comment is shorter than the keyword OR when `kw.len()`
                // lands inside a multibyte char (e.g. a Japanese comment), so a
                // bare `body[..kw.len()]` byte slice cannot panic here.
                let Some(actual_kw) = body.get(..kw.len()) else {
                    continue;
                };
                if !actual_kw.eq_ignore_ascii_case(kw) {
                    continue;
                }
                // `kw.len()` is a confirmed char boundary, so this slice is safe.
                let after_kw = &body[kw.len()..];
                if let Some(&next) = after_kw.as_bytes().first()
                    && (next.is_ascii_alphanumeric() || next == b'_')
                {
                    continue;
                }
                if actual_kw == *kw {
                    if opts.require_colon {
                        if after_kw.starts_with(": ") {
                            continue;
                        }
                    } else {
                        if after_kw.starts_with(' ') {
                            continue;
                        }
                    }
                }
                let msg = if opts.require_colon { MSG_COLON } else { MSG_SPACE };
                cx.emit_offense(trim_line_end(comment.range, cx.source()), &format!("Annotation keywords like `{}` {}", kw, msg), None);
                break;
            }
        }
    }
}

fn trim_line_end(range: Range, source: &str) -> Range {
    let mut end = range.end as usize;
    let bytes = source.as_bytes();
    while end > range.start as usize && matches!(bytes.get(end - 1), Some(b'\n' | b'\r')) {
        end -= 1;
    }
    Range { start: range.start, end: end as u32 }
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
                ^^^^^^^^^^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a colon, and a space.
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
                ^^^^^^^^^^^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a space.
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

    #[test]
    fn does_not_flag_keyword_embedded_in_word() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_no_offenses("# TODO_LIST is a constant\n");
    }

    #[test]
    fn flags_lowercase_keyword() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_offense(indoc! {"
                # todo: something
                ^^^^^^^^^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a colon, and a space.
            "});
    }

    #[test]
    fn flags_keyword_without_note() {
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_offense(indoc! {"
                # TODO
                ^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a colon, and a space.
            "});
    }

    #[test]
    fn does_not_panic_on_multibyte_comment() {
        // A multibyte comment whose bytes at a keyword length land inside a
        // UTF-8 char must not panic on a byte-boundary slice.
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_no_offenses("# 警告がある場合のみ改行付きで追加\n");
    }

    #[test]
    fn does_not_panic_on_multibyte_comment_after_keyword() {
        // Keyword immediately followed by a multibyte char (no separator) must
        // not panic and must not be treated as a properly formatted annotation.
        test::<CommentAnnotation>()
            .with_options(&CommentAnnotationOptions {
                require_colon: true,
            })
            .expect_offense(indoc! {"
                # TODO日本語
                ^^^^^^^^^ Annotation keywords like `TODO` should be all upper case, followed by a colon, and a space.
            "});
    }
}
murphy_plugin_api::submit_cop!(CommentAnnotation);
