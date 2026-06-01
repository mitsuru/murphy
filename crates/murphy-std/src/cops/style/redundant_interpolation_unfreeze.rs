//! `Style/RedundantInterpolationUnfreeze` — flags redundant unfreezing of
//! interpolated strings.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantInterpolationUnfreeze
//! upstream_version_checked: 1.68.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Since Ruby 3.0, interpolated strings are always mutable (unfrozen),
//!   so explicit unfreezing operations are redundant.
//!   Three patterns are detected:
//!     - Unary plus:   `+"#{foo}"` -> `"#{foo}"`
//!     - dup method:   `"#{foo}".dup` -> `"#{foo}"`
//!     - String.new:   `String.new("#{foo}")` -> `"#{foo}"`
//!   Autocorrect removes the unfreezing wrapper for all three patterns
//!   using surgical edits per autocorrect-pattern.md.
//!   Gaps:
//!     - Heredoc forms (e.g. `<<~HEREDOC.dup`) are skipped for autocorrect
//!       because the body content is on separate lines; offenses are still
//!       flagged but autocorrect is suppressed for heredocs.
//!     - minimum_target_ruby_version = "3.0" gates the entire cop.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, SourceTokenKind, cop};

const MSG: &str = "Don't unfreeze interpolated strings as they are already unfrozen.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantInterpolationUnfreeze;

#[cop(
    name = "Style/RedundantInterpolationUnfreeze",
    description = "Checks for redundant unfreezing of interpolated strings.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "3.0",
    options = NoOptions,
)]
impl RedundantInterpolationUnfreeze {
    #[on_node(kind = "send", methods = ["+@", "dup", "new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return;
    };
    let method_name = cx.symbol_str(method);

    match method_name {
        "+@" | "dup" => {
            let Some(recv_id) = receiver.get() else {
                return;
            };
            if !cx.list(args).is_empty() {
                return;
            }
            if !is_dstr_with_interpolation(recv_id, cx) {
                return;
            }
            let offense_range = cx.node(node).loc.name;
            let is_heredoc = is_heredoc_dstr(recv_id, cx);
            cx.emit_offense(offense_range, MSG, None);
            if !is_heredoc {
                emit_prefix_suffix_correction(node, recv_id, method_name, cx);
            }
        }
        "new" => {
            let Some(recv_id) = receiver.get() else {
                return;
            };
            if !is_string_const_no_scope(recv_id, cx) {
                return;
            }
            let arg_list = cx.list(args);
            if arg_list.len() != 1 {
                return;
            }
            let arg_id = arg_list[0];
            if !is_dstr_with_interpolation(arg_id, cx) {
                return;
            }
            let send_start = cx.range(node).start;
            let selector_end = cx.node(node).loc.name.end;
            let offense_range = Range {
                start: send_start,
                end: selector_end,
            };
            let is_heredoc = is_heredoc_dstr(arg_id, cx);
            cx.emit_offense(offense_range, MSG, None);
            if !is_heredoc {
                emit_string_new_correction(node, arg_id, cx);
            }
        }
        _ => {}
    }
}

/// Returns `true` if `node` is a `Const` with name `String` and nil scope.
fn is_string_const_no_scope(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Const { scope, name } => {
            cx.symbol_str(*name) == "String" && scope.get().is_none()
        }
        _ => false,
    }
}

/// Returns `true` if `node` is a `Dstr` with at least one `Begin` child
/// (real interpolation, not adjacent-literal concatenation).
fn is_dstr_with_interpolation(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Dstr(children) = cx.kind(node) else {
        return false;
    };
    cx.list(*children)
        .iter()
        .any(|&child| matches!(cx.kind(child), NodeKind::Begin(_)))
}

/// Returns `true` if the dstr is a heredoc.
fn is_heredoc_dstr(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range.start);
    if let Some(tok) = toks.get(idx) {
        tok.kind == SourceTokenKind::HeredocStart && tok.range.start == range.start
    } else {
        false
    }
}

/// Emit autocorrect for `+dstr` and `dstr.dup`.
fn emit_prefix_suffix_correction(
    node: NodeId,
    recv_id: NodeId,
    method_name: &str,
    cx: &Cx<'_>,
) {
    let send_range = cx.range(node);
    let recv_range = cx.range(recv_id);
    match method_name {
        "+@" => {
            // Delete from send start to recv start (removes `+`).
            cx.emit_edit(
                Range {
                    start: send_range.start,
                    end: recv_range.start,
                },
                "",
            );
        }
        "dup" => {
            // Delete from recv end to send end (removes `.dup`).
            cx.emit_edit(
                Range {
                    start: recv_range.end,
                    end: send_range.end,
                },
                "",
            );
        }
        _ => {}
    }
}

/// Emit autocorrect for `String.new(dstr)`.
fn emit_string_new_correction(node: NodeId, arg_id: NodeId, cx: &Cx<'_>) {
    let send_range = cx.range(node);
    let arg_range = cx.range(arg_id);
    // Delete `String.new(` prefix.
    cx.emit_edit(
        Range {
            start: send_range.start,
            end: arg_range.start,
        },
        "",
    );
    // Delete `)` suffix.
    cx.emit_edit(
        Range {
            start: arg_range.end,
            end: send_range.end,
        },
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::RedundantInterpolationUnfreeze;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_plain_string_unary_plus() {
        test::<RedundantInterpolationUnfreeze>().expect_no_offenses(r#"+"plain string""#);
    }

    #[test]
    fn no_offense_plain_string_dup() {
        test::<RedundantInterpolationUnfreeze>().expect_no_offenses(r#""plain string".dup"#);
    }

    #[test]
    fn no_offense_string_new_plain_string() {
        test::<RedundantInterpolationUnfreeze>().expect_no_offenses(r#"String.new("plain")"#);
    }

    #[test]
    fn no_offense_string_new_no_args() {
        test::<RedundantInterpolationUnfreeze>().expect_no_offenses("String.new");
    }

    #[test]
    fn no_offense_string_new_two_args() {
        // String.new with encoding arg is not flagged.
        test::<RedundantInterpolationUnfreeze>()
            .expect_no_offenses(r##"String.new("#{foo}", encoding: "utf-8")"##);
    }

    #[test]
    fn no_offense_namespaced_string_const() {
        // `Foo::String.new(dstr)` -- namespaced constant, not flagged.
        test::<RedundantInterpolationUnfreeze>()
            .expect_no_offenses(r##"Foo::String.new("#{foo}")"##);
    }

    // --- Offense: unary plus ---

    #[test]
    fn flags_unary_plus_on_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_offense(indoc! {r##"
            +"#{foo} bar"
            ^ Don't unfreeze interpolated strings as they are already unfrozen.
        "##});
    }

    #[test]
    fn flags_unary_plus_on_dstr_only_interpolation() {
        test::<RedundantInterpolationUnfreeze>().expect_offense(indoc! {r##"
            +"#{foo}"
            ^ Don't unfreeze interpolated strings as they are already unfrozen.
        "##});
    }

    // --- Offense: dup ---

    #[test]
    fn flags_dup_on_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_offense(indoc! {r##"
            "#{foo} bar".dup
                         ^^^ Don't unfreeze interpolated strings as they are already unfrozen.
        "##});
    }

    // --- Offense: String.new ---

    #[test]
    fn flags_string_new_with_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_offense(indoc! {r##"
            String.new("#{foo} bar")
            ^^^^^^^^^^ Don't unfreeze interpolated strings as they are already unfrozen.
        "##});
    }

    // --- Autocorrect: unary plus ---

    #[test]
    fn corrects_unary_plus_on_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_correction(
            indoc! {r##"
                +"#{foo} bar"
                ^ Don't unfreeze interpolated strings as they are already unfrozen.
            "##},
            indoc! {r##"
                "#{foo} bar"
            "##},
        );
    }

    // --- Autocorrect: dup ---

    #[test]
    fn corrects_dup_on_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_correction(
            indoc! {r##"
                "#{foo} bar".dup
                             ^^^ Don't unfreeze interpolated strings as they are already unfrozen.
            "##},
            indoc! {r##"
                "#{foo} bar"
            "##},
        );
    }

    // --- Autocorrect: String.new ---

    #[test]
    fn corrects_string_new_with_dstr() {
        test::<RedundantInterpolationUnfreeze>().expect_correction(
            indoc! {r##"
                String.new("#{foo} bar")
                ^^^^^^^^^^ Don't unfreeze interpolated strings as they are already unfrozen.
            "##},
            indoc! {r##"
                "#{foo} bar"
            "##},
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantInterpolationUnfreeze);
