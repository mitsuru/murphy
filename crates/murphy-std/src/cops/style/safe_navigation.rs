//! `Style/SafeNavigation` — use `&.` safe navigation instead of nil guards.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SafeNavigation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects four patterns that can be replaced with safe navigation:
//!     1. `foo.bar if foo`              → `foo&.bar`      (modifier if)
//!     2. `foo.bar unless foo.nil?`     → `foo&.bar`      (unless nil?)
//!     3. `foo.bar unless !foo`         → `foo&.bar`      (unless negated)
//!     4. `foo.bar if !foo.nil?`        → `foo&.bar`      (if negated nil?)
//!     5. `foo ? foo.bar : nil`         → `foo&.bar`      (ternary)
//!     6. `foo && foo.bar`              → `foo&.bar`      (and-chain)
//!   Multi-dot chains are converted (`foo&.bar&.baz`).
//!
//!   Guards (no offense):
//!   - `unless foo` plain check (no negation, no nil?) — not converted
//!   - `if !foo` plain negated check — not converted
//!   - Assignment methods (`foo.baz = bar if foo`)
//!   - Operator methods (`foo.bar > 2 if foo`, `foo.bar + baz`)
//!   - `empty?` / `nil?` methods (nil-safe methods)
//!   - Bracket access `[]` / `[]=`
//!   - Chain length > MaxChainLength (default: 2)
//!   - RHS contains `||` (&&-or pattern)
//!
//!   Safety note: RuboCop marks this cop SafeAutoCorrect: false because
//!   `foo && foo.bar` returns `false` when `foo == false`, but `foo&.bar`
//!   would raise NoMethodError on `false`. This cop mirrors RuboCop's default
//!   behavior of flagging truthiness guards (not just nil?). Patterns using
//!   bool literals like `false && false.method` are not converted since
//!   NodeKind::False_ is not a simple variable. Variables that can return
//!   `false` will be flagged — the same as upstream.
//!
//!   Gaps vs RuboCop:
//!   - `ConvertCodeThatCanStartToReturnNil` option not implemented
//!     (`!foo.nil? && foo.bar` is not converted).
//!   - AllowedMethods from NilMethods mixin: only `empty?` and `nil?`
//!     are explicitly guarded; the full `nil.methods` list is not consulted.
//!   - Comment movement inside if blocks is not implemented.
//!   - Complex `&&`-chain with multiple matching receivers not de-duped.
//! ```
//!
//! ## Matched shapes
//!
//! See parity notes above for the six detected patterns.
//!
//! ## Autocorrect
//!
//! Removes the guard condition and inserts `&` before each `.` in the call
//! chain that routes through the checked variable.
//! - `foo.bar if foo` → remove `if foo`, insert `&` before `.bar` → `foo&.bar`
//! - `foo.bar.baz if foo` → `foo&.bar&.baz`

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Use safe navigation (`&.`) instead of checking if an object exists before calling the method.";

/// Methods that nil itself responds to — converting calls that end in these
/// would change semantics or return wrong results.
const NIL_SAFE_METHODS: &[&str] = &[
    "nil?",
    "blank?",
    "present?",
    "empty?",
    "to_a",
    "to_i",
    "to_f",
    "to_s",
    "to_h",
    "to_r",
    "to_c",
    "to_sym",
    "inspect",
    "frozen?",
    "object_id",
    "class",
    "is_a?",
    "kind_of?",
    "respond_to?",
    "instance_of?",
    "freeze",
    "dup",
    "clone",
    "hash",
    "equal?",
    "itself",
];

/// Stateless unit struct.
#[derive(Default)]
pub struct SafeNavigation;

/// Options for `Style/SafeNavigation`.
#[derive(CopOptions)]
pub struct SafeNavigationOptions {
    #[option(
        name = "MaxChainLength",
        default = 2,
        description = "Maximum method chain length that can be converted to safe navigation."
    )]
    pub max_chain_length: i64,
}

#[cop(
    name = "Style/SafeNavigation",
    description = "Use safe navigation (`&.`) instead of nil guards.",
    default_severity = "warning",
    default_enabled = true,
    options = SafeNavigationOptions,
)]
impl SafeNavigation {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check_if_node(node, cx);
    }

    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        check_and_node(node, cx);
    }
}

// ---------------------------------------------------------------------------
// If/unless/ternary handling
// ---------------------------------------------------------------------------

fn check_if_node(node: NodeId, cx: &Cx<'_>) {
    if cx.is_elsif(node) {
        return;
    }
    if cx.is_ternary(node) {
        check_ternary(node, cx);
    } else if cx.is_modifier_form(node) {
        check_modifier_if(node, cx);
    }
}

/// Extract (checked_var_src, body_node_id) from a modifier if/unless form.
fn extract_modifier_if_parts<'a>(
    node: NodeId,
    cx: &Cx<'a>,
) -> Option<(&'a str, NodeId)> {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return None;
    };

    if cx.is_unless(node) {
        // unless form: body is in else_ branch.
        // Valid:
        //   `unless var.nil?`  → cond = (send var :nil?)
        //   `unless !var`      → cond = (send var :!)
        let body_id = else_.get()?;
        let checked_var = extract_unless_condition_variable(cond, cx)?;
        Some((checked_var, body_id))
    } else {
        // if form: body is in then_ branch.
        // Valid:
        //   `if var`       → cond is bare send/lvar/etc.
        //   `if !var.nil?` → cond = (send (send var :nil?) :!)
        let body_id = then_.get()?;
        let checked_var = extract_if_condition_variable(cond, cx)?;
        Some((checked_var, body_id))
    }
}

/// For `unless` forms: extract checked variable source from condition.
/// Returns Some only for `unless var.nil?` or `unless !var` patterns.
fn extract_unless_condition_variable<'a>(cond: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if let NodeKind::Send { receiver, method, args } = cx.kind(cond) {
        let method_name = cx.symbol_str(*method);
        let args_list = cx.list(*args);
        if args_list.is_empty() {
            if method_name == "nil?" {
                // `unless foo.nil?` → checked var = receiver
                let recv = receiver.get()?;
                return Some(cx.raw_source(cx.range(recv)));
            }
            if method_name == "!" {
                // `unless !foo` → checked var = receiver of `!`
                let recv = receiver.get()?;
                return Some(cx.raw_source(cx.range(recv)));
            }
        }
    }
    None
}

/// For `if` forms: extract checked variable source from condition.
/// Returns Some only for `if var` or `if !var.nil?` patterns.
fn extract_if_condition_variable<'a>(cond: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match cx.kind(cond) {
        NodeKind::Send { receiver, method, args } => {
            let method_name = cx.symbol_str(*method);
            let args_list = cx.list(*args);
            if args_list.is_empty() {
                if method_name == "!" {
                    // `if !var.nil?` → check if receiver is `var.nil?`
                    let inner = receiver.get()?;
                    if let NodeKind::Send {
                        receiver: inner_recv,
                        method: inner_method,
                        args: inner_args,
                    } = cx.kind(inner)
                    {
                        let inner_method_name = cx.symbol_str(*inner_method);
                        let inner_args_list = cx.list(*inner_args);
                        if inner_method_name == "nil?" && inner_args_list.is_empty() {
                            let var = inner_recv.get()?;
                            return Some(cx.raw_source(cx.range(var)));
                        }
                    }
                    // `if !foo` (without .nil?) is NOT converted.
                    return None;
                }
                // `if foo` where foo is a bare method call (no receiver).
                if receiver.get().is_none() {
                    return Some(cx.raw_source(cx.range(cond)));
                }
                // `if foo.bar` has a receiver → NOT converted.
                None
            } else {
                None
            }
        }
        NodeKind::Lvar(_) | NodeKind::Ivar(_) | NodeKind::Gvar(_) | NodeKind::Cvar(_) => {
            // `if @foo`, `if @@foo`, `if $foo`, `if foo` (lvar) — plain variable.
            Some(cx.raw_source(cx.range(cond)))
        }
        _ => None,
    }
}

fn check_modifier_if(node: NodeId, cx: &Cx<'_>) {
    let Some((checked_var_src, body_id)) = extract_modifier_if_parts(node, cx) else {
        return;
    };

    let opts = cx.options_or_default::<SafeNavigationOptions>();
    let max_chain = opts.max_chain_length as usize;

    let Some((receiver_id, chain_len)) = find_matching_receiver(body_id, checked_var_src, cx) else {
        return;
    };

    if chain_len > max_chain {
        return;
    }

    let Some(direct_call) = parent_send_of(receiver_id, body_id, cx) else {
        return;
    };
    if is_unsafe_call(direct_call, body_id, cx) {
        return;
    }

    let node_range = cx.range(node);
    let body_range = cx.range(body_id);

    cx.emit_offense(node_range, MSG, None);

    // Autocorrect: remove prefix (empty for modifier form since body is first) and
    // the trailing ` if foo` / ` unless ...` suffix.
    let prefix_range = Range { start: node_range.start, end: body_range.start };
    let suffix_range = Range { start: body_range.end, end: node_range.end };
    cx.emit_edit(prefix_range, "");
    cx.emit_edit(suffix_range, "");

    add_safe_nav_to_chain(receiver_id, body_id, cx);
}

fn check_ternary(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    let opts = cx.options_or_default::<SafeNavigationOptions>();
    let max_chain = opts.max_chain_length as usize;

    // Determine which branch is body (non-nil) and extract checked variable.
    let result = match (then_.get(), else_.get()) {
        (Some(then_id), Some(else_id)) => {
            if matches!(cx.kind(else_id), NodeKind::Nil) {
                // `cond ? body : nil` — extract variable from condition (not negated/nil?).
                extract_ternary_condition_variable(cond, cx, false).map(|v| (v, then_id))
            } else if matches!(cx.kind(then_id), NodeKind::Nil) {
                // `cond ? nil : body` — condition is negated.
                extract_ternary_condition_variable(cond, cx, true).map(|v| (v, else_id))
            } else {
                None
            }
        }
        _ => None,
    };

    let Some((checked_var_src, body_id)) = result else {
        return;
    };

    let Some((receiver_id, chain_len)) = find_matching_receiver(body_id, checked_var_src, cx) else {
        return;
    };

    if chain_len > max_chain {
        return;
    }

    let Some(direct_call) = parent_send_of(receiver_id, body_id, cx) else {
        return;
    };
    if is_unsafe_call(direct_call, body_id, cx) {
        return;
    }

    let node_range = cx.range(node);
    let body_range = cx.range(body_id);

    cx.emit_offense(node_range, MSG, None);

    let prefix_range = Range { start: node_range.start, end: body_range.start };
    let suffix_range = Range { start: body_range.end, end: node_range.end };
    cx.emit_edit(prefix_range, "");
    cx.emit_edit(suffix_range, "");

    add_safe_nav_to_chain(receiver_id, body_id, cx);
}

/// Extract checked variable source for a ternary condition.
/// `body_is_else`: true when the body is the else branch (negated condition).
fn extract_ternary_condition_variable<'a>(
    cond: NodeId,
    cx: &Cx<'a>,
    body_is_else: bool,
) -> Option<&'a str> {
    match cx.kind(cond) {
        NodeKind::Send { receiver, method, args } => {
            let method_name = cx.symbol_str(*method);
            let args_list = cx.list(*args);
            if args_list.is_empty() {
                if method_name == "nil?" && body_is_else {
                    // `foo.nil? ? nil : body` → var = receiver of nil?
                    let recv = receiver.get()?;
                    return Some(cx.raw_source(cx.range(recv)));
                }
                if method_name == "!" && !body_is_else {
                    // `!foo.nil? ? body : nil` → var = receiver of nil?
                    let inner = receiver.get()?;
                    if let NodeKind::Send {
                        receiver: inner_recv,
                        method: inner_method,
                        args: inner_args,
                    } = cx.kind(inner)
                        && cx.symbol_str(*inner_method) == "nil?"
                            && cx.list(*inner_args).is_empty()
                        {
                            let var = inner_recv.get()?;
                            return Some(cx.raw_source(cx.range(var)));
                        }
                    return None;
                }
                // `foo ? body : nil` — cond is bare send with no receiver.
                if receiver.get().is_none() && !body_is_else {
                    return Some(cx.raw_source(cx.range(cond)));
                }
                None
            } else {
                None
            }
        }
        NodeKind::Lvar(_) | NodeKind::Ivar(_) | NodeKind::Gvar(_) | NodeKind::Cvar(_) => {
            if !body_is_else {
                Some(cx.raw_source(cx.range(cond)))
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// And-chain handling
// ---------------------------------------------------------------------------

fn check_and_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
        return;
    };

    let opts = cx.options_or_default::<SafeNavigationOptions>();
    let max_chain = opts.max_chain_length as usize;

    // lhs must be a simple variable reference.
    let Some(lhs_src) = simple_variable_source(lhs, cx) else {
        return;
    };

    let Some((receiver_id, chain_len)) = find_matching_receiver(rhs, lhs_src, cx) else {
        return;
    };

    if chain_len > max_chain {
        return;
    }

    // Skip if rhs contains `||`.
    if contains_or(rhs, cx) {
        return;
    }

    let Some(direct_call) = parent_send_of(receiver_id, rhs, cx) else {
        return;
    };
    if is_unsafe_call(direct_call, rhs, cx) {
        return;
    }

    let lhs_range = cx.range(lhs);
    let rhs_range = cx.range(rhs);
    let offense_range = Range { start: lhs_range.start, end: rhs_range.end };

    cx.emit_offense(offense_range, MSG, None);

    // Remove `foo && ` (from lhs start to rhs start).
    cx.emit_edit(Range { start: lhs_range.start, end: rhs_range.start }, "");

    add_safe_nav_to_chain(receiver_id, rhs, cx);
}

/// Return Some(src) if `node` is a simple variable (lvar/ivar/gvar/cvar or
/// a bare send with no receiver and no args).
fn simple_variable_source<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match cx.kind(node) {
        NodeKind::Lvar(_) | NodeKind::Ivar(_) | NodeKind::Gvar(_) | NodeKind::Cvar(_) => {
            Some(cx.raw_source(cx.range(node)))
        }
        NodeKind::Send { receiver, args, .. } => {
            if receiver.get().is_none() && cx.list(*args).is_empty() {
                Some(cx.raw_source(cx.range(node)))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if `node` contains any `Or` sub-node (shallow; for guarding `&&`-rhs-`||`).
fn contains_or(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Or { .. } => true,
        NodeKind::Send { receiver, args, .. } => {
            if let Some(recv) = receiver.get()
                && contains_or(recv, cx) {
                    return true;
                }
            cx.list(*args).iter().any(|&a| contains_or(a, cx))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

/// Find the receiver node in `call_chain` whose source matches `checked_var_src`.
/// Returns `(receiver_node_id, chain_length)` where chain_length counts the
/// number of method calls between the receiver and the top of the chain.
fn find_matching_receiver(
    call_chain: NodeId,
    checked_var_src: &str,
    cx: &Cx<'_>,
) -> Option<(NodeId, usize)> {
    find_receiver_inner(call_chain, checked_var_src, cx, 0)
}

fn find_receiver_inner(
    node: NodeId,
    target_src: &str,
    cx: &Cx<'_>,
    depth: usize,
) -> Option<(NodeId, usize)> {
    match cx.kind(node) {
        NodeKind::Send { receiver, .. } => {
            let recv_id = receiver.get()?;
            // Check if receiver matches the target.
            if cx.raw_source(cx.range(recv_id)) == target_src {
                return Some((recv_id, depth + 1));
            }
            // Recurse into the receiver.
            find_receiver_inner(recv_id, target_src, cx, depth + 1)
        }
        // Handle Block wrapping a Send (e.g., `foo.bar { |e| ... }`).
        NodeKind::Block { call, .. } => find_receiver_inner(*call, target_src, cx, depth),
        _ => None,
    }
}

/// Find the direct parent Send of `receiver_id` in the chain.
fn parent_send_of(receiver_id: NodeId, chain_top: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    parent_send_inner(receiver_id, chain_top, cx)
}

fn parent_send_inner(target: NodeId, node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(node) {
        NodeKind::Send { receiver, .. } => {
            if let Some(recv_id) = receiver.get() {
                if recv_id == target {
                    return Some(node);
                }
                parent_send_inner(target, recv_id, cx)
            } else {
                None
            }
        }
        NodeKind::Block { call, .. } => {
            // The block wraps a call; the call might be the parent.
            if *call == target {
                // target is the send that is wrapped in this block — not valid.
                return None;
            }
            parent_send_inner(target, *call, cx)
        }
        _ => None,
    }
}

/// Check if a call (and its chain parents up to chain_top) is unsafe to convert.
fn is_unsafe_call(call_id: NodeId, chain_top: NodeId, cx: &Cx<'_>) -> bool {
    if is_unsafe_method(call_id, cx) {
        return true;
    }
    // Walk chain from call_id up to chain_top checking each method.
    let mut current = call_id;
    while let Some(parent_id) = find_chain_parent(current, chain_top, cx) {
        if is_unsafe_method(parent_id, cx) {
            return true;
        }
        // Nil-safe method in chain → unsafe.
        if let Some(name) = cx.method_name(parent_id)
            && NIL_SAFE_METHODS.contains(&name) {
                return true;
            }
        current = parent_id;
    }
    false
}

/// Find the parent of `target` within `chain` (one level up in the Send chain).
fn find_chain_parent(target: NodeId, chain: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(chain) {
        NodeKind::Send { receiver, .. } => {
            if chain == target {
                return None;
            }
            if let Some(recv_id) = receiver.get() {
                if recv_id == target {
                    return Some(chain);
                }
                find_chain_parent(target, recv_id, cx)
            } else {
                None
            }
        }
        NodeKind::Block { call, .. } => {
            if chain == target {
                return None;
            }
            if *call == target {
                return Some(chain);
            }
            find_chain_parent(target, *call, cx)
        }
        _ => None,
    }
}

/// Check if a specific method call node is unsafe to convert to safe navigation.
fn is_unsafe_method(call_id: NodeId, cx: &Cx<'_>) -> bool {
    // call_operator_loc is None for operator methods, bracket access, implicit sends.
    if cx.call_operator_loc(call_id).is_none() {
        return true;
    }
    if cx.is_assignment_method(call_id) {
        return true;
    }
    if cx.is_operator_method(call_id) {
        return true;
    }
    if let Some(name) = cx.method_name(call_id)
        && NIL_SAFE_METHODS.contains(&name) {
            return true;
        }
    false
}

/// Insert `&` before each `.` in the call chain from `start_receiver` up to `chain_top`.
fn add_safe_nav_to_chain(start_receiver: NodeId, chain_top: NodeId, cx: &Cx<'_>) {
    add_safe_nav_inner(start_receiver, chain_top, cx);
}

fn add_safe_nav_inner(from_receiver: NodeId, current: NodeId, cx: &Cx<'_>) {
    match cx.kind(current) {
        NodeKind::Send { receiver, .. } => {
            if let Some(recv_id) = receiver.get() {
                // Recurse first (bottom-up).
                if recv_id != from_receiver {
                    add_safe_nav_inner(from_receiver, recv_id, cx);
                }
                // Insert `&` before the dot for this call.
                if let Some(dot_range) = cx.call_operator_loc(current) {
                    let dot_src = cx.raw_source(dot_range);
                    if dot_src == "." {
                        cx.emit_edit(
                            Range { start: dot_range.start, end: dot_range.start },
                            "&",
                        );
                    }
                }
            }
        }
        NodeKind::Block { call, .. } => {
            add_safe_nav_inner(from_receiver, *call, cx);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::SafeNavigation;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Modifier `if` -----

    #[test]
    fn flags_method_call_safeguarded_by_if_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar if foo
                ^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_method_call_with_ivar_safeguarded_by_if_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                @foo.bar if @foo
                ^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "@foo&.bar\n",
        );
    }

    #[test]
    fn flags_chained_method_call_safeguarded_by_if_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar.baz if foo
                ^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar&.baz\n",
        );
    }

    #[test]
    fn flags_method_with_params_safeguarded_by_if_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar(baz) if foo
                ^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar(baz)\n",
        );
    }

    // ----- Modifier `unless` -----

    #[test]
    fn flags_method_call_with_unless_negation() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar unless !foo
                ^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_method_call_with_unless_nil_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar unless foo.nil?
                ^^^^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_method_call_with_if_not_nil() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.bar if !foo.nil?
                ^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    // ----- Ternary -----

    #[test]
    fn flags_ternary_with_nil_else() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo ? foo.bar : nil
                ^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_ternary_nil_check_with_nil_then() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo.nil? ? nil : foo.bar
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_ternary_not_nil_check() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                !foo.nil? ? foo.bar : nil
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    // ----- And-chain -----

    #[test]
    fn flags_and_chain_with_method_call() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo && foo.bar
                ^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_and_chain_with_chained_method() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo && foo.bar.baz
                ^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar&.baz\n",
        );
    }

    #[test]
    fn flags_and_chain_with_method_and_params() {
        test::<SafeNavigation>().expect_correction(
            indoc! {"
                foo && foo.bar(baz)
                ^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of checking if an object exists before calling the method.
            "},
            "foo&.bar(baz)\n",
        );
    }

    // ----- No offense cases -----

    #[test]
    fn no_offense_plain_unless_check() {
        // `unless foo` without negation or nil? → not converted.
        test::<SafeNavigation>().expect_no_offenses("obj.do_something unless obj\n");
    }

    #[test]
    fn no_offense_negated_if_check() {
        // `if !foo` without nil? → not converted.
        test::<SafeNavigation>().expect_no_offenses("obj.do_something if !obj\n");
    }

    #[test]
    fn no_offense_assignment_method() {
        test::<SafeNavigation>().expect_no_offenses("foo.baz = bar if foo\n");
    }

    #[test]
    fn no_offense_comparison_in_chain() {
        // `foo.bar > 2 if foo` — `>` is an operator on the chain result.
        test::<SafeNavigation>().expect_no_offenses("foo.bar > 2 if foo\n");
    }

    #[test]
    fn no_offense_empty_predicate_in_and() {
        // `foo && foo.empty?` — empty? is nil-safe, skip.
        test::<SafeNavigation>().expect_no_offenses("foo && foo.empty?\n");
    }

    #[test]
    fn no_offense_nil_predicate_in_chain() {
        // `user && user.thing.nil?` — nil? is in NIL_SAFE_METHODS.
        test::<SafeNavigation>().expect_no_offenses("user && user.thing.nil?\n");
    }

    #[test]
    fn no_offense_safe_navigation_already() {
        test::<SafeNavigation>().expect_no_offenses("foo&.bar\n");
    }

    #[test]
    fn no_offense_method_chain_too_long() {
        // `user && user.one.two.three` — chain length 3 > default max 2.
        test::<SafeNavigation>().expect_no_offenses("user && user.one.two.three\n");
    }

    #[test]
    fn no_offense_different_receiver_in_if() {
        // Condition variable doesn't match method receiver.
        test::<SafeNavigation>().expect_no_offenses("foo.bar if baz\n");
    }

    #[test]
    fn no_offense_bracket_access_in_and() {
        // `foo && foo[:bar]` — bracket access (no dot operator).
        test::<SafeNavigation>().expect_no_offenses("foo && foo[:bar]\n");
    }

    #[test]
    fn no_offense_plain_method_call() {
        test::<SafeNavigation>().expect_no_offenses("foo.bar\n");
    }

    // ----- Semantic safety regression tests -----
    // These document that bool literals are not converted (NodeKind::False_ / True_
    // are not simple variables) — aligning with RuboCop SafeAutoCorrect: false note.

    #[test]
    fn no_offense_false_literal_and_chain() {
        // `false && false.bar` — lhs is a bool literal (NodeKind::False_), not a
        // variable, so simple_variable_source returns None. Not converted.
        test::<SafeNavigation>().expect_no_offenses("false && false.bar\n");
    }

    #[test]
    fn no_offense_true_literal_if_check() {
        // `true.bar if true` — lhs is a bool literal. Not converted.
        test::<SafeNavigation>().expect_no_offenses("true.bar if true\n");
    }
}
murphy_plugin_api::submit_cop!(SafeNavigation);
