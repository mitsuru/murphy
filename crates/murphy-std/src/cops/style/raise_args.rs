//! `Style/RaiseArgs` — checks the args passed to `raise` and `fail`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RaiseArgs
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Supports both `EnforcedStyle` values:
//!     - `exploded` (default): flags `raise Foo.new(msg)` → `raise Foo, msg`.
//!     - `compact`: flags `raise Foo, msg` → `raise Foo.new(msg)`.
//!
//!   Supports `AllowedCompactTypes` (array of type-name strings) in exploded
//!   mode to exempt specific exception classes from the exploded requirement.
//!
//!   Exploded style allows:
//!     - `raise Foo` (no args to new)  — wait, zero args IS flagged → `raise Foo`
//!     - `raise Foo.new(a, b)` (multi-arg new)
//!     - `raise Foo.new(*args)`, `raise Foo.new(**kw)`, `raise Foo.new(...)`
//!     - `raise Foo.new(key: val)` (hash args to new)
//!
//!   Compact style allows:
//!     - `raise Foo.new(msg)` (single-argument constructor)
//!     - `raise Foo` (no args)
//!     - `raise msg` (not an exception class)
//!     - `raise FooError.new, message` (new with no args + message — not flagged;
//!       exception has hash-type first argument guard)
//!     - `raise Foo, msg, caller` (3 args) — flagged but no autocorrect
//!
//!   Autocorrect: whole-node replacement (the raise call changes structure).
//!
//!   Gaps:
//!     - Ruby >= 3.2 anonymous splat/kwsplat args are accepted (Splat(None)
//!       maps to acceptable args).
//! ```
//!
//! ## Enforcement logic
//!
//! ### Exploded (default)
//! Flag when `raise` has exactly 1 argument that is `Foo.new(single_msg)`
//! where `single_msg` is not an "acceptable" type (hash, splat, forwarded_args).
//! Zero args to `new` IS flagged (becomes `raise Foo`). Multi-arg `new` is allowed.
//!
//! ### Compact
//! Flag when `raise` has 2+ arguments (the exception-class + message form).
//! Exception: `raise FooError.new, message` where the exception is a `Send`
//! with a hash-type first arg is NOT flagged.
//! 3-arg form is flagged but has no autocorrect.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RaiseArgs;

const EXPLODED_MSG: &str =
    "Provide an exception class and message as arguments to `%<method>s`.";
const COMPACT_MSG: &str = "Provide an exception object as an argument to `%<method>s`.";

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "exploded")]
    Exploded,
    #[option(value = "compact")]
    Compact,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "exploded",
        description = "When `exploded` (default), require `raise Foo, msg` form. \
                       When `compact`, require `raise Foo.new(msg)` form."
    )]
    pub enforced_style: EnforcedStyle,

    #[option(
        name = "AllowedCompactTypes",
        default = [],
        description = "Exception class names that are exempt from the exploded \
                       requirement (exploded style only)."
    )]
    pub allowed_compact_types: Vec<String>,
}

#[cop(
    name = "Style/RaiseArgs",
    description = "Checks the args passed to `raise` and `fail`.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl RaiseArgs {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Only bare `raise` / `fail` (no receiver).
        if !cx.is_command(node, "raise") && !cx.is_command(node, "fail") {
            return;
        }

        let opts = cx.options_or_default::<Options>();
        match opts.enforced_style {
            EnforcedStyle::Compact => check_compact(node, cx),
            EnforcedStyle::Exploded => check_exploded(node, cx, &opts.allowed_compact_types),
        }
    }
}

// ---------------------------------------------------------------------------
// Compact checks
// ---------------------------------------------------------------------------

fn check_compact(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.len() <= 1 {
        return;
    }

    // `raise FooError.new, message` — exception is a Send with hash-type first arg.
    // RuboCop skips this case.
    let exception = args[0];
    if let NodeKind::Send { args: new_args, .. } = *cx.kind(exception) {
        let new_arg_list = cx.list(new_args);
        if !new_arg_list.is_empty()
            && matches!(cx.kind(new_arg_list[0]), NodeKind::Hash(..))
        {
            return;
        }
    }

    let method_name = cx.method_name(node).unwrap_or("raise");
    let msg = COMPACT_MSG.replace("%<method>s", method_name);
    cx.emit_offense(cx.range(node), &msg, None);

    // Autocorrect only for 2-arg form (3-arg has no safe correction).
    if args.len() == 2 {
        let replacement = correction_exploded_to_compact(node, cx);
        if let Some(replacement) = replacement {
            cx.emit_edit(cx.range(node), &replacement);
        }
    }
}

fn correction_exploded_to_compact(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let args = cx.call_arguments(node);
    let exception_node = args[0];
    let message_node = args[1];

    let argument = cx.raw_source(cx.range(message_node));

    // The exception class: if it's a `Foo.new(...)` call, use the receiver.
    // Otherwise use the exception node itself.
    // Guard: if exception is already `Foo.new(args...)` with non-empty constructor
    // args, skip autocorrection — merging existing constructor args with the
    // separate message argument requires semantic knowledge we don't have.
    let exception_class =
        if let NodeKind::Send { receiver, method, args: new_args } = *cx.kind(exception_node) {
            if cx.symbol_str(method) == "new" {
                // If the existing `.new(...)` call already has constructor args,
                // we cannot safely merge them with `message_node` — skip correction.
                if !cx.list(new_args).is_empty() {
                    return None;
                }
                if let Some(recv_id) = receiver.get() {
                    cx.raw_source(cx.range(recv_id))
                } else {
                    cx.raw_source(cx.range(exception_node))
                }
            } else {
                cx.raw_source(cx.range(exception_node))
            }
        } else {
            cx.raw_source(cx.range(exception_node))
        };

    let method_name = cx.method_name(node).unwrap_or("raise");
    let inner = format!("{}.new({})", exception_class, argument);

    let needs_parens = requires_parens(node, cx);
    Some(if needs_parens {
        format!("{}({})", method_name, inner)
    } else {
        format!("{} {}", method_name, inner)
    })
}

// ---------------------------------------------------------------------------
// Exploded checks
// ---------------------------------------------------------------------------

fn check_exploded(node: NodeId, cx: &Cx<'_>, allowed_compact_types: &[String]) {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let first_arg = args[0];

    // Must be `Foo.new(...)` with a receiver.
    if !use_new_method(first_arg, cx) {
        return;
    }

    // Get the args to `new`.
    let NodeKind::Send { args: new_args, .. } = *cx.kind(first_arg) else {
        return;
    };
    let new_arg_list = cx.list(new_args);

    // Check acceptable_exploded_args.
    if acceptable_exploded_args(new_arg_list, cx) {
        return;
    }

    // Check AllowedCompactTypes.
    if is_allowed_compact_type(first_arg, allowed_compact_types, cx) {
        return;
    }

    let method_name = cx.method_name(node).unwrap_or("raise");
    let msg = EXPLODED_MSG.replace("%<method>s", method_name);
    cx.emit_offense(cx.range(node), &msg, None);

    let replacement = correction_compact_to_exploded(node, cx);
    cx.emit_edit(cx.range(node), &replacement);
}

fn use_new_method(arg: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(arg) else {
        return false;
    };
    receiver.get().is_some() && cx.symbol_str(method) == "new"
}

fn acceptable_exploded_args(args: &[NodeId], cx: &Cx<'_>) -> bool {
    // Allow multi-arg new: `raise Foo.new(a, b, c)`.
    if args.len() > 1 {
        return true;
    }
    // Disallow zero args (will be flagged: `raise Foo.new` → `raise Foo`).
    if args.is_empty() {
        return false;
    }
    // Single arg: allow if it's a type that may forward multiple arguments.
    let arg = args[0];
    matches!(
        cx.kind(arg),
        NodeKind::Hash(..)
            | NodeKind::Splat(_)
            | NodeKind::ForwardedArgs
    )
}

fn is_allowed_compact_type(arg: NodeId, allowed: &[String], cx: &Cx<'_>) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let NodeKind::Send { receiver, .. } = *cx.kind(arg) else {
        return false;
    };
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    let Some(name) = cx.const_name(recv_id) else {
        return false;
    };
    allowed.iter().any(|a| a == &name)
}

fn correction_compact_to_exploded(node: NodeId, cx: &Cx<'_>) -> String {
    let args = cx.call_arguments(node);
    let first_arg = args[0];

    let NodeKind::Send { receiver, args: new_args, .. } = *cx.kind(first_arg) else {
        return cx.raw_source(cx.range(node)).to_owned();
    };
    let new_arg_list = cx.list(new_args);

    let method_name = cx.method_name(node).unwrap_or("raise");
    let needs_parens = requires_parens(node, cx);

    // exception_node: the receiver of `.new` (the class itself).
    let exception_class = if let Some(recv_id) = receiver.get() {
        cx.raw_source(cx.range(recv_id))
    } else {
        cx.raw_source(cx.range(first_arg))
    };

    // Build arguments string: exception class [, message if present].
    let arguments = if let Some(msg_node) = new_arg_list.first() {
        let msg_src = cx.raw_source(cx.range(*msg_node));
        format!("{}, {}", exception_class, msg_src)
    } else {
        exception_class.to_owned()
    };

    if needs_parens {
        format!("{}({})", method_name, arguments)
    } else {
        format!("{} {}", method_name, arguments)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Whether the parent of `node` is an `and`/`or` (operator keyword) or a
/// ternary `if` — in which case the raise needs parens in the correction.
fn requires_parens(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    cx.is_operator_keyword(parent)
        || (matches!(cx.kind(parent), NodeKind::If { .. }) && cx.is_ternary(parent))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Options, RaiseArgs};
    use murphy_plugin_api::test_support::{indoc, test};

    fn compact_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::Compact,
            allowed_compact_types: vec![],
        }
    }

    fn exploded_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::Exploded,
            allowed_compact_types: vec![],
        }
    }

    // =========================================================================
    // Compact style
    // =========================================================================

    #[test]
    fn compact_flags_raise_with_2_args() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    raise RuntimeError, msg
                    ^^^^^^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "raise RuntimeError.new(msg)\n",
            );
    }

    #[test]
    fn compact_flags_raise_with_local_var_exception() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    raise error_class, msg
                    ^^^^^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "raise error_class.new(msg)\n",
            );
    }

    #[test]
    fn compact_flags_raise_foo_new_message() {
        // `raise FooError.new, message` — 2 args where exception.new has no
        // args but the second argument is the message.
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    raise FooError.new, message
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "raise FooError.new(message)\n",
            );
    }

    #[test]
    fn compact_flags_raise_new_with_args_and_message_no_correction() {
        // `raise FooError.new(context), message` — exception is a `.new(args...)`
        // call with non-empty constructor args; cannot safely merge with the
        // message argument, so only an offense is emitted without autocorrect.
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_offense(indoc! {"
                raise FooError.new(context), message
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
            "});
    }

    #[test]
    fn compact_flags_raise_3_args_no_correction() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_offense(
                indoc! {"
                    raise RuntimeError, msg, caller
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
            );
    }

    #[test]
    fn compact_accepts_raise_with_1_arg() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_no_offenses("raise Ex.new(msg)\n");
    }

    #[test]
    fn compact_accepts_raise_with_msg_only() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_no_offenses("raise msg\n");
    }

    #[test]
    fn compact_accepts_kw_arg_exception_with_message() {
        // `raise MyKwArgError.new(a: 1, b: 2), message` — exception is a Send
        // with hash first-arg, so not flagged.
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_no_offenses("raise MyKwArgError.new(a: 1, b: 2), message\n");
    }

    #[test]
    fn compact_flags_when_used_in_ternary() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    foo ? raise(Ex, 'error') : bar
                          ^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "foo ? raise(Ex.new('error')) : bar\n",
            );
    }

    #[test]
    fn compact_flags_when_used_in_logical_and() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    bar && raise(Ex, 'error')
                           ^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "bar && raise(Ex.new('error'))\n",
            );
    }

    #[test]
    fn compact_flags_when_used_in_logical_or() {
        test::<RaiseArgs>()
            .with_options(&compact_opts())
            .expect_correction(
                indoc! {"
                    bar || raise(Ex, 'error')
                           ^^^^^^^^^^^^^^^^^^ Provide an exception object as an argument to `raise`.
                "},
                "bar || raise(Ex.new('error'))\n",
            );
    }

    // =========================================================================
    // Exploded style (default)
    // =========================================================================

    #[test]
    fn exploded_flags_raise_new_with_msg() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    raise Ex.new(msg)
                    ^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "raise Ex, msg\n",
            );
    }

    #[test]
    fn exploded_flags_raise_new_no_args() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    raise Ex.new
                    ^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "raise Ex\n",
            );
    }

    #[test]
    fn exploded_accepts_raise_with_2_args() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise RuntimeError, msg\n");
    }

    #[test]
    fn exploded_accepts_raise_with_3_args() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise RuntimeError, msg, caller\n");
    }

    #[test]
    fn exploded_accepts_raise_with_msg_only() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise msg\n");
    }

    #[test]
    fn exploded_accepts_raise_new_multi_args() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise MyCustomError.new(a1, a2, a3)\n");
    }

    #[test]
    fn exploded_accepts_raise_new_keyword_args() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise MyKwArgError.new(a: 1, b: 2)\n");
    }

    #[test]
    fn exploded_accepts_raise_new_splat() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise MyCustomError.new(*args)\n");
    }

    #[test]
    fn exploded_accepts_raise_new_with_receiver_and_extra_args() {
        // `raise Ex.new(entity), message` — 2 args to `raise` itself → ignored.
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise Ex.new(entity), message\n");
    }

    #[test]
    fn exploded_accepts_raise_bare_new_no_receiver() {
        // `raise new` — `new` has no receiver → not a class constructor.
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_no_offenses("raise new\n");
    }

    #[test]
    fn exploded_flags_when_used_in_ternary() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    foo ? raise(Ex.new('error')) : bar
                          ^^^^^^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "foo ? raise(Ex, 'error') : bar\n",
            );
    }

    #[test]
    fn exploded_flags_when_used_in_logical_and() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    bar && raise(Ex.new('error'))
                           ^^^^^^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "bar && raise(Ex, 'error')\n",
            );
    }

    #[test]
    fn exploded_flags_when_used_in_logical_or() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    bar || raise(Ex.new('error'))
                           ^^^^^^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "bar || raise(Ex, 'error')\n",
            );
    }

    #[test]
    fn exploded_allowed_compact_types_accepted() {
        let opts = Options {
            enforced_style: EnforcedStyle::Exploded,
            allowed_compact_types: vec!["Ex1".to_owned()],
        };
        test::<RaiseArgs>()
            .with_options(&opts)
            .expect_no_offenses("raise Ex1.new(msg)\n");
    }

    #[test]
    fn exploded_allowed_compact_types_still_flags_others() {
        let opts = Options {
            enforced_style: EnforcedStyle::Exploded,
            allowed_compact_types: vec!["Ex1".to_owned()],
        };
        test::<RaiseArgs>()
            .with_options(&opts)
            .expect_offense(indoc! {"
                raise Ex2.new(msg)
                ^^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
            "});
    }

    #[test]
    fn exploded_flags_local_var_receiver_new() {
        test::<RaiseArgs>()
            .with_options(&exploded_opts())
            .expect_correction(
                indoc! {"
                    raise klass.new('hi')
                    ^^^^^^^^^^^^^^^^^^^^^ Provide an exception class and message as arguments to `raise`.
                "},
                "raise klass, 'hi'\n",
            );
    }
}
murphy_plugin_api::submit_cop!(RaiseArgs);
