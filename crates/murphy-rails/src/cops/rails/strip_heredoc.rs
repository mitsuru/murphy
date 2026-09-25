//! `Rails/StripHeredoc` — use squiggly heredoc over `strip_heredoc`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/StripHeredoc
//! upstream_version_checked: 2.35.0
//! version_added: "2.15"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (`RESTRICT_ON_SEND =
//!   [:strip_heredoc]`): flags `strip_heredoc` called on a heredoc `str`
//!   / `dstr` receiver (detected via a `HeredocStart` token inside the
//!   receiver range, mirroring `receiver.heredoc?`). Offense is the whole
//!   send; autocorrect rewrites the opener to `<<~` and removes
//!   `.strip_heredoc` (dot through selector), so chained calls such as
//!   `<<-EOS.strip_heredoc.do_something` become `<<~EOS.do_something`.
//!   Not gated: upstream `minimum_target_ruby_version 2.3` (murphy parses
//!   with a modern grammar, so squiggly heredocs always parse).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct StripHeredoc;

#[cop(
    name = "Rails/StripHeredoc",
    description = "Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl StripHeredoc {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[strip_heredoc]`.
    #[on_node(kind = "send", methods = ["strip_heredoc"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(node) != Some("strip_heredoc") {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    // Upstream `receiver.type?(:str, :dstr)` + `receiver.heredoc?`.
    if !matches!(*cx.kind(recv), NodeKind::Str(_) | NodeKind::Dstr(_)) {
        return;
    }
    let Some(opener) = heredoc_opener(recv, cx) else {
        return;
    };
    cx.emit_offense(
        cx.range(node),
        "Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.",
        None,
    );
    // `heredoc.source.sub(/\A<<(-|~)?/, '<<~')`
    let opener_src = cx.raw_source(opener).to_owned();
    let rest = opener_src
        .strip_prefix("<<~")
        .or_else(|| opener_src.strip_prefix("<<-"))
        .or_else(|| opener_src.strip_prefix("<<"))
        .unwrap_or(&opener_src);
    cx.emit_edit(opener, &format!("<<~{rest}"));
    // Remove `.strip_heredoc`: dot through selector.
    let dot = cx.loc(node).dot();
    let selector = cx.selector(node);
    if dot != murphy_plugin_api::Range::ZERO && selector != murphy_plugin_api::Range::ZERO {
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: dot.start,
                end: selector.end,
            },
            "",
        );
    }
}

/// The `<<EOS` / `<<-EOS` / `<<~EOS` opener token of a heredoc string node.
fn heredoc_opener(node: NodeId, cx: &Cx<'_>) -> Option<murphy_plugin_api::Range> {
    cx.tokens_in(cx.range(node))
        .iter()
        .find(|t| t.kind == SourceTokenKind::HeredocStart)
        .map(|t| t.range)
}

#[cfg(test)]
mod tests {
    use super::StripHeredoc;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_plain_opener() {
        test::<StripHeredoc>().expect_correction(
            indoc! {r#"
                <<EOS.strip_heredoc
                ^^^^^^^^^^^^^^^^^^^ Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.
                  some text
                EOS
            "#},
            indoc! {r#"
                <<~EOS
                  some text
                EOS
            "#},
        );
    }

    #[test]
    fn flags_dash_opener() {
        test::<StripHeredoc>().expect_correction(
            indoc! {r#"
                <<-EOS.strip_heredoc
                ^^^^^^^^^^^^^^^^^^^^ Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.
                  some text
                EOS
            "#},
            indoc! {r#"
                <<~EOS
                  some text
                EOS
            "#},
        );
    }

    #[test]
    fn flags_chained_call() {
        test::<StripHeredoc>().expect_correction(
            indoc! {r#"
                <<-EOS.strip_heredoc.do_something
                ^^^^^^^^^^^^^^^^^^^^ Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.
                  some text
                EOS
            "#},
            indoc! {r#"
                <<~EOS.do_something
                  some text
                EOS
            "#},
        );
    }

    #[test]
    fn flags_squiggly_opener_with_strip() {
        test::<StripHeredoc>().expect_correction(
            indoc! {r#"
                <<~EOS.strip_heredoc
                ^^^^^^^^^^^^^^^^^^^^ Use squiggly heredoc (`<<~`) instead of `strip_heredoc`.
                  some text
                EOS
            "#},
            indoc! {r#"
                <<~EOS
                  some text
                EOS
            "#},
        );
    }

    #[test]
    fn allows_plain_squiggly() {
        test::<StripHeredoc>().expect_no_offenses(indoc! {r#"
            <<~EOS
              some text
            EOS
        "#});
    }

    #[test]
    fn allows_non_heredoc_receiver() {
        test::<StripHeredoc>().expect_no_offenses(indoc! {r#"
            <<-EOS.do_something.strip_heredoc
              some text
            EOS
        "#});
    }

    #[test]
    fn allows_bare_strip_heredoc() {
        test::<StripHeredoc>().expect_no_offenses("strip_heredoc
");
    }
}
murphy_plugin_api::submit_cop!(StripHeredoc);
