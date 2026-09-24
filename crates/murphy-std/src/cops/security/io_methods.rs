//! `Security/IoMethods` — flag IO path methods that can execute commands.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/IoMethods
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's on_send/RESTRICT_ON_SEND behavior for `read`,
//!   `binread`, `write`, `binwrite`, `foreach`, and `readlines`, only when the
//!   receiver's source is exactly `IO`. A first argument whose String/Dstr
//!   value, after Ruby `String#strip` whitespace removal, begins with `|` is
//!   allowed; other arguments are flagged. Dstr pipe checks use the leading
//!   literal segment, matching RuboCop's DstrNode#value. The full send range is
//!   reported and trimmed before an attached block. `Safe: false` and unsafe
//!   autocorrect metadata match RuboCop; this issue remains offense-only even
//!   though RuboCop offers an `IO`-to-`File` fix under `--autocorrect-all`.
//!   RuboCop also raises on non-String literal values whose `.value` lacks
//!   `String#strip`; Murphy flags those values instead of propagating the
//!   upstream exception.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, SourceTokenKind, cop};

const METHODS: &[&str] = &["read", "binread", "write", "binwrite", "foreach", "readlines"];

#[derive(Default)]
pub struct IoMethods;

#[cop(
    name = "Security/IoMethods",
    description = "Checks for the first argument to `IO.read`, `IO.binread`, `IO.write`, `IO.binwrite`, `IO.foreach`, and `IO.readlines`.",
    default_severity = "warning",
    default_enabled = false,
    safe = false,
    safe_autocorrect = false,
    options = NoOptions,
)]
impl IoMethods {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if !METHODS.contains(&method) {
            return;
        }

        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if cx.raw_source(cx.range(receiver)) != "IO" {
            return;
        }

        if cx
            .call_arguments(node)
            .first()
            .is_some_and(|&argument| is_pipe_command(argument, cx))
        {
            return;
        }

        let message = format!("`File.{method}` is safer than `IO.{method}`.");
        cx.emit_offense(send_range(node, cx), &message, None);
    }
}

/// RuboCop checks `argument.value.strip.start_with?('|')` when the argument
/// exposes a string value. Dstr values concatenate literal segments with the
/// source of interpolation expressions; only a leading literal segment can
/// make that value begin with a pipe.
fn is_pipe_command(argument: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(argument) {
        NodeKind::Str(value) => starts_with_pipe_after_strip(cx.string_str(value)),
        NodeKind::Dstr(parts) => cx
            .list(parts)
            .first()
            .is_some_and(|&first| match *cx.kind(first) {
                NodeKind::Str(value) => starts_with_pipe_after_strip(cx.string_str(value)),
                _ => false,
            }),
        _ => false,
    }
}

/// Ruby `String#strip` removes NUL and ASCII whitespace, but not Unicode
/// whitespace such as NBSP. Keep this explicit instead of Rust's broader
/// Unicode-aware `str::trim`.
fn starts_with_pipe_after_strip(value: &str) -> bool {
    value
        .trim_matches(|ch| matches!(ch, '\0' | ' ' | '\t' | '\n' | '\r' | '\x0C' | '\x0B'))
        .starts_with('|')
}

/// RuboCop reports the Send expression. Parenthesized calls end at the
/// matching `)`, and `do/end` blocks end at the last argument. For
/// unparenthesized calls with arguments, RuboCop's parser includes a brace
/// block in the send range; Prism includes every attached block, so keep that
/// brace range but trim `do/end` blocks. No-argument calls end at the selector.
fn send_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let expression = cx.range(node);
    let is_attached_block = cx.parent(node).get().is_some_and(|parent| {
        match *cx.kind(parent) {
            NodeKind::Block { call, .. } => call == node,
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send == node,
            _ => false,
        }
    });
    if !is_attached_block {
        return expression;
    }

    let loc = cx.loc(node);
    let selector = loc.name;
    let opening_paren = loc.begin();
    let closing_paren = if selector != Range::ZERO && opening_paren.start == selector.end {
        loc.end()
    } else {
        Range::ZERO
    };
    let arguments = cx.call_arguments(node);
    let end = if closing_paren != Range::ZERO {
        closing_paren.end
    } else if let Some(last_argument) = arguments.last() {
        let argument_end = cx.range(*last_argument).end;
        if first_token_after_is_left_brace(argument_end, expression.end, cx) {
            expression.end
        } else {
            argument_end
        }
    } else {
        selector.end
    };

    Range {
        start: expression.start,
        end,
    }
}

/// A command-style call with arguments followed by `{ ... }` has a parser-gem
/// Send source range that includes the braces. Skip comments/newlines between
/// the last argument and block opener so multiline command calls match too.
fn first_token_after_is_left_brace(start: u32, end: u32, cx: &Cx<'_>) -> bool {
    if start >= end {
        return false;
    }
    cx.tokens_in(Range { start, end })
        .iter()
        .find(|token| {
            !matches!(
                token.kind,
                SourceTokenKind::Comment
                    | SourceTokenKind::Newline
                    | SourceTokenKind::IgnoredNewline
            )
        })
        .is_some_and(|token| token.kind == SourceTokenKind::LeftBrace)
}

murphy_plugin_api::submit_cop!(IoMethods);

#[cfg(test)]
mod tests {
    use super::IoMethods;
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::Cop;

    #[test]
    fn mirrors_rubocop_pending_unsafe_metadata() {
        assert_eq!(<IoMethods as Cop>::DEFAULT_ENABLED, Some(false));
        assert_eq!(<IoMethods as Cop>::SAFE, Some(false));
        assert_eq!(<IoMethods as Cop>::SAFE_AUTOCORRECT, Some(false));
    }

    #[test]
    fn flags_io_file_methods_with_rubocop_send_ranges() {
        test::<IoMethods>().expect_offense(indoc! {r#"
            IO.read(path)
            ^^^^^^^^^^^^^ `File.read` is safer than `IO.read`.
            IO::read(path)
            ^^^^^^^^^^^^^^ `File.read` is safer than `IO.read`.
            IO.read
            ^^^^^^^ `File.read` is safer than `IO.read`.
            IO.binread(path)
            ^^^^^^^^^^^^^^^^ `File.binread` is safer than `IO.binread`.
            IO.write(path, data)
            ^^^^^^^^^^^^^^^^^^^^ `File.write` is safer than `IO.write`.
            IO.binwrite(path, data)
            ^^^^^^^^^^^^^^^^^^^^^^^ `File.binwrite` is safer than `IO.binwrite`.
            IO.foreach(path)
            ^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
            IO.readlines(path)
            ^^^^^^^^^^^^^^^^^^ `File.readlines` is safer than `IO.readlines`.
            IO.foreach(path) do |line|
            ^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
              line
            end
            IO.foreach path do |line|
            ^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
              line
            end
        "#});
    }

    #[test]
    fn matches_rubocop_ranges_for_brace_and_nested_argument_blocks() {
        test::<IoMethods>().expect_offense(indoc! {r#"
            IO.foreach(path) { |line| line }
            ^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
            IO.foreach path { |line| line }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
            IO.foreach(path(foo)) do |line|
            ^^^^^^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
              line
            end
            IO.foreach() do
            ^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
            end
            IO.foreach { |line| line }
            ^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
            IO.foreach path(foo) do |line|
            ^^^^^^^^^^^^^^^^^^^^ `File.foreach` is safer than `IO.foreach`.
              line
            end
        "#});
    }

    #[test]
    fn flags_non_string_arguments_without_propagating_rubocop_value_errors() {
        // RuboCop calls `.strip` on any literal node exposing `value`, which
        // raises for non-String values such as integers and symbols. Murphy
        // treats these untrusted paths as unsafe instead of propagating that
        // upstream exception.
        test::<IoMethods>().expect_offense(indoc! {r#"
            IO.read(42)
            ^^^^^^^^^^^ `File.read` is safer than `IO.read`.
        "#});
    }

    #[test]
    fn ignores_command_pipe_paths_after_ruby_strip() {
        test::<IoMethods>().expect_no_offenses(indoc! {r#"
            IO.read("|command")
            IO.read(" \t|command")
            IO.read("\x7c command")
            IO.read("\0|command")
            IO.read("\v|command")
            IO.read("|#{command}")
        "#});
    }

    #[test]
    fn interpolated_value_and_unicode_strip_match_rubocop() {
        test::<IoMethods>().expect_offense(indoc! {r##"
            IO.read("#{command}|")
            ^^^^^^^^^^^^^^^^^^^^^^ `File.read` is safer than `IO.read`.
            IO.read("\u00A0|command")
            ^^^^^^^^^^^^^^^^^^^^^^^^^ `File.read` is safer than `IO.read`.
        "##});
    }

    #[test]
    fn ignores_other_receivers_methods_and_safe_navigation() {
        test::<IoMethods>().expect_no_offenses(indoc! {r#"
            File.read(path)
            IO.open(path)
            IO.send(:read, path)
            ::IO.read(path)
            Other::IO.read(path)
            IO&.read(path)
        "#});
    }
}
