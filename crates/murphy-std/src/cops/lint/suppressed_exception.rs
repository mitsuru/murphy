//! `Lint/SuppressedException` — checks `rescue` blocks with no body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SuppressedException
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: empty rescue bodies, method/singleton-method
//!   rescue, block rescue, modifier rescue nil, AllowComments, and AllowNil.
//! ```

use murphy_plugin_api::{cop, CopOptions, Cx, NodeId, NodeKind, Range};

const MSG: &str = "Do not suppress exceptions.";

#[derive(Default)]
pub struct SuppressedException;

#[derive(CopOptions)]
pub struct SuppressedExceptionOptions {
    #[option(name = "AllowComments", default = true, description = "Allow rescue bodies containing only comments.")]
    pub allow_comments: bool,
    #[option(name = "AllowNil", default = true, description = "Allow rescue bodies containing only nil.")]
    pub allow_nil: bool,
}

#[cop(
    name = "Lint/SuppressedException",
    description = "Checks rescue blocks with no body.",
    default_severity = "warning",
    default_enabled = true,
    options = SuppressedExceptionOptions,
)]
impl SuppressedException {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<SuppressedExceptionOptions>();
        let NodeKind::Resbody { body, .. } = *cx.kind(node) else {
            return;
        };
        let nil_body = body.get().is_some_and(|id| matches!(cx.kind(id), NodeKind::Nil));
        if body.get().is_some() && !nil_body {
            return;
        }
        if opts.allow_nil && nil_body {
            return;
        }
        if opts.allow_comments && comment_between_rescue_and_end(node, cx) {
            return;
        }
        cx.emit_offense(offense_range(node, cx), MSG, None);
    }
}

fn offense_range(node: NodeId, cx: &Cx<'_>) -> Range {
    if let Some(rescue) = cx
        .ancestors(node)
        .find(|&a| matches!(cx.kind(a), NodeKind::Rescue { .. }))
        && cx.loc(rescue).end_keyword() == Range::ZERO
    {
        let rescue_range = cx.range(rescue);
        if let Some(tok) = cx.tokens_in(rescue_range).iter().find(|&&tok| cx.token_text(tok) == "rescue") {
            return Range { start: tok.range.start, end: rescue_range.end };
        }
    }
    let r = cx.range(node);
    let source = cx.source();
    let line_end = source[r.start as usize..]
        .find('\n')
        .map(|i| r.start as usize + i)
        .unwrap_or(r.end as usize);
    Range { start: r.start, end: line_end as u32 }
}

fn comment_between_rescue_and_end(node: NodeId, cx: &Cx<'_>) -> bool {
    let r = cx.range(node);
    let source = cx.source();
    let end = current_resbody_end(node, cx).unwrap_or(r.end);
    source[r.start as usize..end as usize]
        .lines()
        .skip(1)
        .any(|line| line.trim_start().starts_with('#'))
}

fn current_resbody_end(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let rescue = cx
        .ancestors(node)
        .find(|&ancestor| matches!(cx.kind(ancestor), NodeKind::Rescue { .. }))?;
    let NodeKind::Rescue { resbodies, else_, .. } = *cx.kind(rescue) else {
        return None;
    };
    let resbodies = cx.list(resbodies);
    let idx = resbodies.iter().position(|&resbody| resbody == node)?;
    if let Some(&next_resbody) = resbodies.get(idx + 1) {
        return Some(cx.range(next_resbody).start);
    }
    if let Some(else_id) = else_.get() {
        return Some(cx.range(else_id).start);
    }
    let end_keyword = cx.loc(rescue).end_keyword();
    if end_keyword != Range::ZERO {
        return Some(end_keyword.start);
    }
    Some(cx.range(rescue).end)
}

murphy_plugin_api::submit_cop!(SuppressedException);

#[cfg(test)]
mod tests {
    use super::{SuppressedException, SuppressedExceptionOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_rescue_block() {
        test::<SuppressedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue
            ^^^^^^ Do not suppress exceptions.
            end
        "#});
    }

    #[test]
    fn later_rescue_comments_do_not_allow_empty_rescue() {
        test::<SuppressedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue Foo
            ^^^^^^^^^^ Do not suppress exceptions.
            rescue Bar
              # handled elsewhere
            end
        "#});
    }

    #[test]
    fn honors_allow_comments_and_allow_nil() {
        test::<SuppressedException>()
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue
                  # do nothing
                end
            "#})
            .expect_no_offenses("something rescue nil\n");

        test::<SuppressedException>()
            .with_options(&SuppressedExceptionOptions { allow_comments: false, allow_nil: false })
            .expect_offense(indoc! {r#"
                something rescue nil
                          ^^^^^^^^^^ Do not suppress exceptions.
            "#});
    }

    #[test]
    fn modifier_rescue_range_uses_rescue_keyword_token() {
        test::<SuppressedException>()
            .with_options(&SuppressedExceptionOptions { allow_nil: false, ..Default::default() })
            .expect_offense(indoc! {r#"
                rescue_me rescue nil
                          ^^^^^^^^^^ Do not suppress exceptions.
            "#});
    }
}
