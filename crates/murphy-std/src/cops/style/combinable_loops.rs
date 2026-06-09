//! `Style/CombinableLoops` — combine consecutive loops over the same collection.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CombinableLoops
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags consecutive `each` blocks on the same receiver.
//!   Autocorrect is a v1 gap; only offense reporting is implemented.
//!   `for` loops not yet handled.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Combine this loop with the previous loop.";

#[derive(Default)]
pub struct CombinableLoops;

#[cop(
    name = "Style/CombinableLoops",
    description = "Combine consecutive loops over the same collection.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CombinableLoops {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let parent = cx.parent(node);
        let Some(parent_id) = parent.get() else {
            return;
        };
        if !matches!(cx.kind(parent_id), NodeKind::Begin { .. }) {
            return;
        }
        let siblings = cx.children(parent_id);
        let Some(pos) = siblings.iter().position(|&s| s == node) else {
            return;
        };
        if pos == 0 {
            return;
        }
        let prev = siblings[pos - 1];
        if !matches!(cx.kind(prev), NodeKind::Block { .. }) {
            return;
        }
        let NodeKind::Block { call: this_call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Block { call: prev_call, .. } = *cx.kind(prev) else {
            return;
        };
        let NodeKind::Send { receiver: this_recv, method: this_method, .. } = *cx.kind(this_call) else {
            return;
        };
        let NodeKind::Send { receiver: prev_recv, method: prev_method, .. } = *cx.kind(prev_call) else {
            return;
        };
        let this_method_str = cx.symbol_str(this_method);
        let prev_method_str = cx.symbol_str(prev_method);
        if this_method_str != prev_method_str {
            return;
        }
        if !this_method_str.starts_with("each") && !this_method_str.ends_with("_each") {
            return;
        }
        // Compare receiver expressions by source text (separate node instances
        // of the same expression have distinct NodeIds).
        let this_recv_src = match this_recv.get() {
            Some(r) => cx.raw_source(cx.range(r)),
            None => return,
        };
        let prev_recv_src = match prev_recv.get() {
            Some(r) => cx.raw_source(cx.range(r)),
            None => return,
        };
        if this_recv_src != prev_recv_src {
            return;
        }
        cx.emit_offense(first_line_range(cx.range(node), cx.source()), MSG, None);
    }
}

fn first_line_range(range: Range, source: &str) -> Range {
    let bytes = source.as_bytes();
    let mut end = range.start as usize;
    while end < range.end as usize && end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    Range { start: range.start, end: end as u32 }
}

#[cfg(test)]
mod tests {
    use super::CombinableLoops;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_combinable_each_blocks() {
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              items.each do |item|
                do_something(item)
              end

              items.each do |item|
              ^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                do_something_else(item)
              end
            end
        "});
    }

    #[test]
    fn accepts_single_each() {
        test::<CombinableLoops>().expect_no_offenses(
            "items.each { |item| do_something(item) }\n",
        );
    }

    #[test]
    fn accepts_different_collections() {
        test::<CombinableLoops>().expect_no_offenses(
            "items.each { |i| f(i) }\nother.each { |i| g(i) }\n",
        );
    }
}
murphy_plugin_api::submit_cop!(CombinableLoops);
