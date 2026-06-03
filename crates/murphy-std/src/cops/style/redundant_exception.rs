//! `Style/RedundantException` — checks for redundant `RuntimeError` in
//! `raise`/`fail`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantException
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Two patterns are flagged:
//!     1. Exploded: `raise RuntimeError, 'message'` → `raise 'message'`
//!     2. Compact: `raise RuntimeError.new('message')` → `raise 'message'`
//!
//!   When the message is not a string literal (str/dstr/xstr), `.to_s` is
//!   appended in the autocorrect.
//!
//!   Both `raise` and `fail` are handled.
//!   `::RuntimeError` (cbase-scoped) is treated the same as bare `RuntimeError`.
//!   `Foo::RuntimeError` (namespaced) is NOT flagged.
//!
//!   Gaps: none.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (exploded)
//! raise RuntimeError, 'message'
//! fail RuntimeError, 'message'
//! raise RuntimeError, Object.new   # => raise Object.new.to_s
//!
//! # bad (compact)
//! raise RuntimeError.new('message')
//! raise RuntimeError.new(Object.new)   # => raise Object.new.to_s
//!
//! # good
//! raise 'message'
//! raise RuntimeError  # no message arg -- not flagged
//! raise RuntimeError.new  # no message arg -- not flagged
//! raise Foo::RuntimeError, 'message'  # namespaced -- not flagged
//! raise RuntimeError, 'msg', caller   # 3 args -- not flagged
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantException;

const MSG_1: &str = "Redundant `RuntimeError` argument can be removed.";
const MSG_2: &str = "Redundant `RuntimeError.new` call can be replaced with just the message.";

#[cop(
    name = "Style/RedundantException",
    description = "Checks for redundant `RuntimeError` argument in `raise`/`fail`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantException {
    #[on_node(kind = "send", methods = ["raise", "fail"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only match implicit receiver (nil).
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return;
    };
    if receiver.get().is_some() {
        return;
    }

    let args = cx.list(args);

    // Try exploded pattern: raise RuntimeError, message (exactly 2 args)
    if args.len() == 2 && cx.is_global_const(args[0], "RuntimeError") {
        let message = args[1];
        cx.emit_offense(cx.range(node), MSG_1, None);
        autocorrect_exploded(args[0], message, cx);
        return;
    }

    // Try compact pattern: raise RuntimeError.new(message) (exactly 1 arg = send :new)
    if args.len() == 1 {
        let arg = args[0];
        if let NodeKind::Send { receiver: new_recv, args: new_args, .. } = *cx.kind(arg)
            && let Some(recv_id) = new_recv.get()
                && cx.is_global_const(recv_id, "RuntimeError")
                    && cx.method_name(arg) == Some("new")
                {
                    let new_args_list = cx.list(new_args);
                    if new_args_list.len() == 1 {
                        let message = new_args_list[0];
                        cx.emit_offense(cx.range(node), MSG_2, None);
                        autocorrect_compact(arg, message, cx);
                    }
                }
    }
}

/// Returns true if the node is a string literal (str, dstr, xstr).
fn is_string_message(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(id), NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Xstr(_))
}

/// Autocorrect for exploded form: `raise RuntimeError, message`
/// => `raise message` or `raise message.to_s`
///
/// Strategy: delete from const-start to message-start (removes `RuntimeError, `),
/// then optionally append `.to_s` if message is not a string.
fn autocorrect_exploded(const_node: NodeId, message: NodeId, cx: &Cx<'_>) {
    // Delete the const and the comma+space between it and the message.
    // Range: from const.start to message.start
    let delete_range = Range {
        start: cx.range(const_node).start,
        end: cx.range(message).start,
    };
    cx.emit_edit(delete_range, "");

    // If message is not a string, append `.to_s` after the message.
    if !is_string_message(message, cx) {
        let msg_end = cx.range(message).end;
        let insert_pos = Range { start: msg_end, end: msg_end };
        cx.emit_edit(insert_pos, ".to_s");
    }
}

/// Autocorrect for compact form: `raise RuntimeError.new(message)`
/// => replaces `RuntimeError.new(message)` with `message` or `message.to_s`
fn autocorrect_compact(new_call: NodeId, message: NodeId, cx: &Cx<'_>) {
    let replacement = if is_string_message(message, cx) {
        cx.raw_source(cx.range(message)).to_owned()
    } else {
        format!("{}.to_s", cx.raw_source(cx.range(message)))
    };
    cx.emit_edit(cx.range(new_call), &replacement);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantException;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- Exploded form: raise RuntimeError, 'message' ----

    #[test]
    fn flags_raise_runtime_error_exploded() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            raise RuntimeError, 'message'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
        "#});
    }

    #[test]
    fn corrects_raise_runtime_error_exploded() {
        test::<RedundantException>().expect_correction(
            indoc! {r#"
                raise RuntimeError, 'message'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
            "#},
            "raise 'message'\n",
        );
    }

    #[test]
    fn flags_fail_runtime_error_exploded() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            fail RuntimeError, 'message'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
        "#});
    }

    #[test]
    fn corrects_fail_runtime_error_exploded() {
        test::<RedundantException>().expect_correction(
            indoc! {r#"
                fail RuntimeError, 'message'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
            "#},
            "fail 'message'\n",
        );
    }

    #[test]
    fn flags_raise_runtime_error_non_string_message() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            raise RuntimeError, Object.new
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
        "#});
    }

    #[test]
    fn corrects_raise_runtime_error_non_string_message() {
        test::<RedundantException>().expect_correction(
            indoc! {r#"
                raise RuntimeError, Object.new
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
            "#},
            "raise Object.new.to_s\n",
        );
    }

    // ---- Exploded form with ::RuntimeError ----

    #[test]
    fn flags_raise_cbase_runtime_error() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            raise ::RuntimeError, 'message'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError` argument can be removed.
        "#});
    }

    // ---- Compact form: raise RuntimeError.new('message') ----

    #[test]
    fn flags_raise_runtime_error_compact() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            raise RuntimeError.new('message')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError.new` call can be replaced with just the message.
        "#});
    }

    #[test]
    fn corrects_raise_runtime_error_compact() {
        test::<RedundantException>().expect_correction(
            indoc! {r#"
                raise RuntimeError.new('message')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError.new` call can be replaced with just the message.
            "#},
            "raise 'message'\n",
        );
    }

    #[test]
    fn flags_raise_runtime_error_compact_non_string() {
        test::<RedundantException>().expect_offense(indoc! {r#"
            raise RuntimeError.new(Object.new)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError.new` call can be replaced with just the message.
        "#});
    }

    #[test]
    fn corrects_raise_runtime_error_compact_non_string() {
        test::<RedundantException>().expect_correction(
            indoc! {r#"
                raise RuntimeError.new(Object.new)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Redundant `RuntimeError.new` call can be replaced with just the message.
            "#},
            "raise Object.new.to_s\n",
        );
    }

    // ---- Negative cases ----

    #[test]
    fn no_offense_raise_string_directly() {
        test::<RedundantException>().expect_no_offenses("raise 'message'\n");
    }

    #[test]
    fn no_offense_raise_runtime_error_alone() {
        // Only 1 arg (the const), no message -- not flagged
        test::<RedundantException>().expect_no_offenses("raise RuntimeError\n");
    }

    #[test]
    fn no_offense_raise_runtime_error_new_no_args() {
        // .new with 0 args -- not flagged
        test::<RedundantException>().expect_no_offenses("raise RuntimeError.new\n");
    }

    #[test]
    fn no_offense_raise_runtime_error_three_args() {
        // 3 args (including backtrace) -- not flagged
        test::<RedundantException>().expect_no_offenses("raise RuntimeError, 'msg', caller\n");
    }

    #[test]
    fn no_offense_raise_namespaced_runtime_error() {
        // Foo::RuntimeError is not global const
        test::<RedundantException>().expect_no_offenses("raise Foo::RuntimeError, 'message'\n");
    }

    #[test]
    fn no_offense_raise_other_error() {
        test::<RedundantException>().expect_no_offenses("raise ArgumentError, 'message'\n");
    }

    #[test]
    fn no_offense_raise_with_receiver() {
        // `obj.raise RuntimeError, 'message'` -- has a receiver, not flagged
        test::<RedundantException>().expect_no_offenses("obj.raise RuntimeError, 'message'\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantException);
