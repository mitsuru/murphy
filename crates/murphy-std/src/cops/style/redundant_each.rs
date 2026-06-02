//! `Style/RedundantEach` — checks for redundant `.each` in method chains.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantEach
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   This cop is marked unsafe in RuboCop (false positives if receiver is
//!   not an Enumerator). Murphy ships it default_enabled=true matching
//!   RuboCop's default.
//!
//!   Detection has two directions:
//!     Direction A (inner each): node is :each, parent is a call with
//!       receiver==node, parent method in {each, each_with_index,
//!       each_with_object, reverse_each}. Offense covers inner selector +
//!       parent dot ("each.").
//!     Direction B (outer call): node is :each/:each_with_index/:each_with_object,
//!       its receiver is a call (not inside a block) whose method starts with
//!       "each_" or is "reverse_each", with no block-pass last arg.
//!       Offense covers the outer method's selector.
//!
//!   Both send and csend are handled (matching RuboCop's alias on_csend on_send).
//!   Block-pass guard: skip if last argument of receiver is a BlockPass.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (direction A — inner each is redundant)
//! array.each.each { |v| v }            # => array.each { |v| v }
//! array.each.each_with_index { |v,i| } # => array.each.with_index { |v,i| }
//! array.each.each_with_object({}) { }  # => array.each.with_object({}) { }
//! array.each.reverse_each { |v| v }    # => array.reverse_each { |v| v }
//!
//! # bad (direction B — outer each is redundant)
//! array.reverse_each.each { |v| v }          # => array.reverse_each { |v| v }
//! array.each_with_index.each { |v| v }       # => array.each_with_index { |v| v }
//! array.each_with_object(x).each { |v| v }   # => array.each_with_object(x) { |v| v }
//!
//! # good
//! array.each { |v| v }
//! array.each { |v| v }.each { |v| v }   # each on block result, not chained bare each
//! array.each(&blk).each { }             # block-pass guard
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantEach;

const MSG: &str = "Remove redundant `each`.";
const MSG_WITH_INDEX: &str = "Use `with_index` to remove redundant `each`.";
const MSG_WITH_OBJECT: &str = "Use `with_object` to remove redundant `each`.";

#[cop(
    name = "Style/RedundantEach",
    description = "Checks for redundant `.each` in method chains.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantEach {
    #[on_node(kind = "send", methods = ["each", "each_with_index", "each_with_object"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if matches!(
            cx.symbol_str(method),
            "each" | "each_with_index" | "each_with_object"
        ) {
            check(node, cx);
        }
    }
}

/// Returns the method name if node is a send/csend, else None.
fn get_method_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    cx.method_name(node)
}

/// Returns true if the last argument of a call is a BlockPass.
fn has_block_pass_last_arg(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    if let Some(&last) = args.last() {
        return matches!(cx.kind(last), NodeKind::BlockPass(_));
    }
    false
}

/// Returns true if the node (send/csend) is a valid "previous method" for
/// direction B detection: must be a call type, its own parent must NOT be any
/// block type, and must not have a block-pass last argument.
fn is_valid_prev_method(node: NodeId, cx: &Cx<'_>) -> bool {
    // Must be send or csend
    if !matches!(cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    // Parent of this node must not be a block
    if let Some(parent) = cx.parent(node).get() {
        if matches!(
            cx.kind(parent),
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
        ) {
            return false;
        }
    }
    // Must not have block-pass last arg
    if has_block_pass_last_arg(node, cx) {
        return false;
    }
    true
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match get_method_name(node, cx) {
        Some(m) => m,
        None => return,
    };

    // Block-pass guard for node itself
    if has_block_pass_last_arg(node, cx) {
        return;
    }

    match method {
        "each" => check_each(node, cx),
        "each_with_index" => check_direction_b(node, "each_with_index", cx),
        "each_with_object" => check_direction_b(node, "each_with_object", cx),
        _ => {}
    }
}

/// Handle :each nodes.
/// Tries direction A first (inner each check), then direction B (outer each via reverse_each).
fn check_each(node: NodeId, cx: &Cx<'_>) {
    // Direction A: node is :each, parent is a call with receiver==node
    // and parent method in {each, each_with_index, each_with_object, reverse_each}.
    // The parent must NOT be inside a block itself (parent.parent is not a block).
    // Actually: check `!node.parent.any_block_type?` — i.e. the immediate parent
    // of this `each` node is not a block. But we need the parent to be a send/csend.
    let parent_opt = cx.parent(node).get();
    if let Some(parent) = parent_opt {
        // Confirm parent is a call (send/csend)
        if matches!(cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            // Confirm parent's receiver is this node
            if cx.call_receiver(parent).get() == Some(node) {
                let parent_method = get_method_name(parent, cx).unwrap_or("");
                if matches!(
                    parent_method,
                    "each" | "each_with_index" | "each_with_object" | "reverse_each"
                ) {
                    // Found direction A match
                    emit_direction_a(node, parent, cx);
                    return;
                }
            }
        }
    }

    // Direction B: node is :each, receiver is reverse_each
    let receiver = match cx.call_receiver(node).get() {
        Some(r) => r,
        None => return,
    };

    if !is_valid_prev_method(receiver, cx) {
        return;
    }

    let recv_method = match get_method_name(receiver, cx) {
        Some(m) => m,
        None => return,
    };

    if recv_method == "reverse_each" || recv_method.starts_with("each_") {
        // Direction B: outer each is redundant
        emit_direction_b_each(node, cx);
    }
}

/// Direction B for each_with_index / each_with_object:
/// receiver must start with "each_" or be "reverse_each".
fn check_direction_b(node: NodeId, method: &str, cx: &Cx<'_>) {
    let receiver = match cx.call_receiver(node).get() {
        Some(r) => r,
        None => return,
    };

    if !is_valid_prev_method(receiver, cx) {
        return;
    }

    let recv_method = match get_method_name(receiver, cx) {
        Some(m) => m,
        None => return,
    };

    // Only trigger if prev method starts with "each_" or is "reverse_each"
    if !recv_method.starts_with("each_") && recv_method != "reverse_each" {
        return;
    }

    // Offense on the outer method selector
    let selector = cx.selector(node);
    let msg = if method == "each_with_index" {
        MSG_WITH_INDEX
    } else {
        MSG_WITH_OBJECT
    };
    cx.emit_offense(selector, msg, None);

    // Autocorrect: rename each_with_index → with_index, each_with_object → with_object
    let new_name = if method == "each_with_index" {
        "with_index"
    } else {
        "with_object"
    };
    cx.emit_edit(selector, new_name);
}

/// Direction A: inner each is redundant.
/// Offense range: inner each selector + parent dot ("each.").
/// Correction depends on what the parent method is.
fn emit_direction_a(each_node: NodeId, parent: NodeId, cx: &Cx<'_>) {
    let parent_method = get_method_name(parent, cx).unwrap_or("");

    // Offense range: inner each selector joined with parent dot
    let inner_selector = cx.selector(each_node);
    let offense_range = if matches!(cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        let parent_dot = cx.loc(parent).dot();
        if parent_dot != Range::ZERO {
            Range {
                start: inner_selector.start,
                end: parent_dot.end,
            }
        } else {
            inner_selector
        }
    } else {
        inner_selector
    };

    let msg = match parent_method {
        "each_with_index" => MSG_WITH_INDEX,
        "each_with_object" => MSG_WITH_OBJECT,
        _ => MSG,
    };

    cx.emit_offense(offense_range, msg, None);

    // Autocorrect
    match parent_method {
        "each_with_index" => {
            // Remove "each." (the offense range covers inner selector + parent dot)
            cx.emit_edit(offense_range, "");
            // Rename parent selector from each_with_index to each.with_index
            let parent_selector = cx.selector(parent);
            cx.emit_edit(parent_selector, "each.with_index");
        }
        "each_with_object" => {
            // Remove "each."
            cx.emit_edit(offense_range, "");
            // Rename parent selector from each_with_object to each.with_object
            let parent_selector = cx.selector(parent);
            cx.emit_edit(parent_selector, "each.with_object");
        }
        _ => {
            // For :each and :reverse_each: remove the inner "each." portion
            cx.emit_edit(offense_range, "");
        }
    }
}

/// Direction B for outer :each (receiver is reverse_each).
/// Offense range: dot before each + each selector (".each").
/// Correction: remove ".each"
fn emit_direction_b_each(each_node: NodeId, cx: &Cx<'_>) {
    let selector = cx.selector(each_node);
    let dot = cx.loc(each_node).dot();

    let offense_range = if dot != Range::ZERO {
        Range { start: dot.start, end: selector.end }
    } else {
        selector
    };

    cx.emit_offense(offense_range, MSG, None);
    cx.emit_edit(offense_range, "");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantEach;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- Direction A: inner each is redundant ----

    #[test]
    fn flags_each_each() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each.each { |v| v }
                  ^^^^^ Remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_each() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each.each { |v| v }
                      ^^^^^ Remove redundant `each`.
            "},
            "array.each { |v| v }\n",
        );
    }

    #[test]
    fn flags_each_each_with_index() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each.each_with_index { |v, i| v }
                  ^^^^^ Use `with_index` to remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_each_with_index() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each.each_with_index { |v, i| v }
                      ^^^^^ Use `with_index` to remove redundant `each`.
            "},
            "array.each.with_index { |v, i| v }\n",
        );
    }

    #[test]
    fn flags_each_each_with_object() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each.each_with_object({}) { |v, o| v }
                  ^^^^^ Use `with_object` to remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_each_with_object() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each.each_with_object({}) { |v, o| v }
                      ^^^^^ Use `with_object` to remove redundant `each`.
            "},
            "array.each.with_object({}) { |v, o| v }\n",
        );
    }

    #[test]
    fn flags_each_reverse_each() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each.reverse_each { |v| v }
                  ^^^^^ Remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_reverse_each() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each.reverse_each { |v| v }
                      ^^^^^ Remove redundant `each`.
            "},
            "array.reverse_each { |v| v }\n",
        );
    }

    // ---- Direction B: outer each is redundant (receiver is reverse_each) ----

    #[test]
    fn flags_reverse_each_each() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.reverse_each.each { |v| v }
                              ^^^^^ Remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_reverse_each_each() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.reverse_each.each { |v| v }
                                  ^^^^^ Remove redundant `each`.
            "},
            "array.reverse_each { |v| v }\n",
        );
    }

    // ---- Direction B: each_with_index/object whose receiver starts with each_ ----

    #[test]
    fn flags_each_with_index_each_with_index() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each_with_index.each_with_index { |v, i| v }
                                  ^^^^^^^^^^^^^^^ Use `with_index` to remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_with_index_each_with_index() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each_with_index.each_with_index { |v, i| v }
                                      ^^^^^^^^^^^^^^^ Use `with_index` to remove redundant `each`.
            "},
            "array.each_with_index.with_index { |v, i| v }\n",
        );
    }

    // ---- Direction B: each_with_index.each / each_with_object(x).each ----

    #[test]
    fn flags_each_with_index_each() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each_with_index.each { |v| v }
                                 ^^^^^ Remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_with_index_each() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each_with_index.each { |v| v }
                                     ^^^^^ Remove redundant `each`.
            "},
            "array.each_with_index { |v| v }\n",
        );
    }

    #[test]
    fn flags_each_with_object_each() {
        test::<RedundantEach>().expect_offense(indoc! {"
            array.each_with_object(x).each { |v| v }
                                     ^^^^^ Remove redundant `each`.
        "});
    }

    #[test]
    fn corrects_each_with_object_each() {
        test::<RedundantEach>().expect_correction(
            indoc! {"
                array.each_with_object(x).each { |v| v }
                                         ^^^^^ Remove redundant `each`.
            "},
            "array.each_with_object(x) { |v| v }\n",
        );
    }

    // ---- Negative cases ----

    #[test]
    fn no_offense_plain_each() {
        test::<RedundantEach>().expect_no_offenses("array.each { |v| v }\n");
    }

    #[test]
    fn no_offense_each_on_block_result() {
        // array.each { }.each { } — inner each's parent is a block, outer each's
        // receiver is a block — not a redundant pattern
        test::<RedundantEach>().expect_no_offenses("array.each { |v| v }.each { |v| v }\n");
    }

    #[test]
    fn no_offense_each_with_block_pass() {
        // array.each(&blk).each — the receiver has a block-pass arg
        test::<RedundantEach>().expect_no_offenses("array.each(&blk).each { |v| v }\n");
    }

    #[test]
    fn no_offense_each_with_index_alone() {
        test::<RedundantEach>().expect_no_offenses("array.each_with_index { |v, i| v }\n");
    }

    #[test]
    fn no_offense_each_with_object_alone() {
        test::<RedundantEach>().expect_no_offenses("array.each_with_object({}) { |v, o| v }\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantEach);
