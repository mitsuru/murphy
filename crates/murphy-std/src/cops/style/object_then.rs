//! `Style/ObjectThen` — enforces consistent use of `Object#then` or `Object#yield_self`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ObjectThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Both EnforcedStyle values (`then` and `yield_self`) are implemented.
//!   Block path: flags block/numblock/itblock where the inner send (or csend)
//!   method is the non-preferred name.
//!   Send path: flags bare send/csend with exactly one block-pass argument.
//!   Self.then special case: when style=then and receiver is nil (bare call),
//!   autocorrect emits `self.then` instead of bare `then` to avoid the keyword.
//! ```
//!
//! ## Matched shapes
//!
//! Block path (`block`, `numblock`, `itblock`): the inner send method is
//! `then` or `yield_self` and does not match the enforced style.
//!
//! Send path (`send`, `csend`): method is `then` or `yield_self`, exactly one
//! argument, and that argument is a block-pass node (`&method(:foo)`).
//!
//! ## Autocorrect
//!
//! Surgical rename of the selector (`loc.name`) to the preferred method.
//! Special case: when style=`then` and receiver is nil, emits `self.then`
//! to avoid colliding with Ruby's `then` keyword.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ObjectThen;

/// Enforced style: prefer `then` (default) or `yield_self`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ObjectThenStyle {
    #[default]
    #[option(value = "then")]
    Then,
    #[option(value = "yield_self")]
    YieldSelf,
}

#[derive(CopOptions)]
pub struct ObjectThenOptions {
    #[option(
        name = "EnforcedStyle",
        default = "then",
        description = "Which method name to enforce: `then` (default) or `yield_self`."
    )]
    pub enforced_style: ObjectThenStyle,
}

fn preferred_name(style: ObjectThenStyle) -> &'static str {
    match style {
        ObjectThenStyle::Then => "then",
        ObjectThenStyle::YieldSelf => "yield_self",
    }
}

fn non_preferred_name(style: ObjectThenStyle) -> &'static str {
    match style {
        ObjectThenStyle::Then => "yield_self",
        ObjectThenStyle::YieldSelf => "then",
    }
}

fn msg(prefer: &str, current: &str) -> String {
    format!("Prefer `{prefer}` over `{current}`.")
}

/// Returns the inner send/csend node of a block/numblock/itblock if its method
/// is `then` or `yield_self`; otherwise `None`.
fn block_send(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(node) {
        NodeKind::Block { call, .. } => {
            let name = cx.method_name(*call)?;
            if name == "then" || name == "yield_self" {
                Some(*call)
            } else {
                None
            }
        }
        NodeKind::Numblock { send, .. } => {
            let name = cx.method_name(*send)?;
            if name == "then" || name == "yield_self" {
                Some(*send)
            } else {
                None
            }
        }
        NodeKind::Itblock { send, .. } => {
            let name = cx.method_name(*send)?;
            if name == "then" || name == "yield_self" {
                Some(*send)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns `true` when `send` is a bare (no-receiver) `Send` node.
/// `Csend` always has a receiver, so this only fires for `Send`.
fn has_no_receiver(send: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Send { receiver, .. } = cx.kind(send) {
        return receiver.get().is_none();
    }
    false
}

/// Emit an offense + autocorrect for a send node that uses the non-preferred method.
fn check_method_node(send: NodeId, cx: &Cx<'_>, style: ObjectThenStyle) {
    let name = match cx.method_name(send) {
        Some(n) => n,
        None => return,
    };

    // Only flag the non-preferred method.
    if name != non_preferred_name(style) {
        return;
    }

    let prefer = preferred_name(style);
    let selector_range = cx.node(send).loc.name;
    cx.emit_offense(selector_range, &msg(prefer, name), None);

    // Autocorrect: rename the selector.
    // Special case: style=then + nil receiver → emit `self.then` to avoid
    // colliding with Ruby's `then` keyword (bare `then` is a keyword in if/unless).
    if style == ObjectThenStyle::Then && has_no_receiver(send, cx) {
        // Bare `yield_self { }` (no receiver) → `self.then { }`.
        cx.emit_edit(selector_range, "self.then");
    } else {
        cx.emit_edit(selector_range, prefer);
    }
}

#[cop(
    name = "Style/ObjectThen",
    description = "Enforce consistent use of `Object#then` or `Object#yield_self`.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "2.6",
    options = ObjectThenOptions,
)]
impl ObjectThen {
    /// Block path: `obj.yield_self { |x| ... }` or `obj.then { |x| ... }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ObjectThenOptions>();
        if let Some(send) = block_send(node, cx) {
            check_method_node(send, cx, opts.enforced_style);
        }
    }

    /// Numbered-parameter block: `obj.yield_self { _1.foo }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ObjectThenOptions>();
        if let Some(send) = block_send(node, cx) {
            check_method_node(send, cx, opts.enforced_style);
        }
    }

    /// `it`-parameter block: `obj.yield_self { it.foo }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ObjectThenOptions>();
        if let Some(send) = block_send(node, cx) {
            check_method_node(send, cx, opts.enforced_style);
        }
    }

    /// Send path: `obj.yield_self(&method(:foo))` — exactly one block-pass arg.
    #[on_node(kind = "send", methods = ["then", "yield_self"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ObjectThenOptions>();
        let args = cx.call_arguments(node);
        if args.len() == 1 && matches!(cx.kind(args[0]), NodeKind::BlockPass(_)) {
            check_method_node(node, cx, opts.enforced_style);
        }
    }

    /// Safe-navigation send path: `obj&.yield_self(&block)`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        // `methods = [...]` is not supported on csend — filter manually.
        let name = match cx.method_name(node) {
            Some(n) => n,
            None => return,
        };
        if name != "then" && name != "yield_self" {
            return;
        }
        let opts = cx.options_or_default::<ObjectThenOptions>();
        let args = cx.call_arguments(node);
        if args.len() == 1 && matches!(cx.kind(args[0]), NodeKind::BlockPass(_)) {
            check_method_node(node, cx, opts.enforced_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectThen, ObjectThenOptions, ObjectThenStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- EnforcedStyle: then (default) -----

    #[test]
    fn flags_yield_self_block_default_style() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                obj.yield_self { |x| x.do_something }
                    ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "obj.then { |x| x.do_something }\n",
        );
    }

    #[test]
    fn accepts_then_block_default_style() {
        test::<ObjectThen>().expect_no_offenses("obj.then { |x| x.do_something }\n");
    }

    #[test]
    fn flags_yield_self_numblock() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                obj.yield_self { _1.do_something }
                    ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "obj.then { _1.do_something }\n",
        );
    }

    #[test]
    fn flags_yield_self_block_pass() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                obj.yield_self(&method(:foo))
                    ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "obj.then(&method(:foo))\n",
        );
    }

    #[test]
    fn accepts_then_block_pass() {
        test::<ObjectThen>().expect_no_offenses("obj.then(&method(:foo))\n");
    }

    #[test]
    fn does_not_flag_send_without_block_pass() {
        // obj.yield_self with a regular argument is not a block-pass form
        test::<ObjectThen>().expect_no_offenses("obj.yield_self(:foo)\n");
    }

    #[test]
    fn does_not_flag_send_with_no_args() {
        // bare obj.yield_self (no block, no args) — send path requires block-pass
        test::<ObjectThen>().expect_no_offenses("obj.yield_self\n");
    }

    // ----- nil receiver: bare `yield_self { }` → `self.then { }` -----

    #[test]
    fn flags_bare_yield_self_block_to_self_then() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                yield_self { |x| x }
                ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "self.then { |x| x }\n",
        );
    }

    // ----- EnforcedStyle: yield_self -----

    #[test]
    fn flags_then_block_yield_self_style() {
        test::<ObjectThen>()
            .with_options(&ObjectThenOptions {
                enforced_style: ObjectThenStyle::YieldSelf,
            })
            .expect_correction(
                indoc! {"
                    obj.then { |x| x.do_something }
                        ^^^^ Prefer `yield_self` over `then`.
                "},
                "obj.yield_self { |x| x.do_something }\n",
            );
    }

    #[test]
    fn accepts_yield_self_block_yield_self_style() {
        test::<ObjectThen>()
            .with_options(&ObjectThenOptions {
                enforced_style: ObjectThenStyle::YieldSelf,
            })
            .expect_no_offenses("obj.yield_self { |x| x.do_something }\n");
    }

    #[test]
    fn flags_then_block_pass_yield_self_style() {
        test::<ObjectThen>()
            .with_options(&ObjectThenOptions {
                enforced_style: ObjectThenStyle::YieldSelf,
            })
            .expect_correction(
                indoc! {"
                    obj.then(&method(:foo))
                        ^^^^ Prefer `yield_self` over `then`.
                "},
                "obj.yield_self(&method(:foo))\n",
            );
    }

    // ----- csend / safe navigation -----

    #[test]
    fn flags_csend_yield_self_block_pass() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                obj&.yield_self(&method(:foo))
                     ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "obj&.then(&method(:foo))\n",
        );
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <ObjectThen as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(2, 6)),
        );
    }

    // ----- csend block form (block node with csend inner call) -----

    #[test]
    fn flags_csend_yield_self_block() {
        test::<ObjectThen>().expect_correction(
            indoc! {"
                obj&.yield_self { |x| x }
                     ^^^^^^^^^^ Prefer `then` over `yield_self`.
            "},
            "obj&.then { |x| x }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(ObjectThen);
