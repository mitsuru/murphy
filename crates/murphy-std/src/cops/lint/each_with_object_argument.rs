//! `Lint/EachWithObjectArgument` — flag an immutable argument to
//! `each_with_object`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EachWithObjectArgument
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port. We match `each_with_object` calls with exactly one
//!   argument; the offense fires when that argument is an immutable literal
//!   (`cx.is_immutable_literal`, mirroring RuboCop's `immutable_literal?`).
//!   Both receiverless (`each_with_object(0)`) and receiver calls are flagged,
//!   matching RuboCop's `(call _ :each_with_object ...)` where `_` binds the
//!   nil-filled receiver slot. Safe-navigation calls
//!   (`x&.each_with_object(0) { … }`) are handled via the `csend` arm,
//!   matching RuboCop's `alias_method :on_csend, :on_send`.
//! ```

use murphy_plugin_api::{cop, Cx, NodeId, NoOptions, Range, SourceTokenKind};

#[derive(Default)]
pub struct EachWithObjectArgument;

#[cop(
    name = "Lint/EachWithObjectArgument",
    description = "Checks for immutable argument given to each_with_object.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EachWithObjectArgument {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("each_with_object") {
        return;
    }
    let [arg] = cx.call_arguments(node) else {
        return;
    };
    if cx.is_immutable_literal(*arg) {
        cx.emit_offense(
            call_range(node, cx),
            "The argument to each_with_object cannot be immutable.",
            None,
        );
    }
}

/// The source range of the call itself, excluding any attached block.
///
/// Murphy's `send` node range extends through an attached `{ … }` / `do … end`
/// block, but RuboCop's `on_send` offense highlights only the call portion
/// (`collection.each_with_object(0)`). When this send is the `call` of a parent
/// `block`, trim the range to end at the block opener (`{` or `do`), then walk
/// back over any whitespace so the highlight ends at the last call byte.
fn call_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let full = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    // Search after the last argument's end so a string/symbol arg containing
    // `{` or `do` can't be mistaken for the block opener.
    let search_from = cx
        .call_arguments(node)
        .last()
        .map_or(full.start, |&a| cx.range(a).end);
    let idx = toks.partition_point(|t| t.range.start < search_from);
    let opener = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < full.end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        });
    let Some(opener) = opener else {
        return full;
    };
    // Trim trailing whitespace between the call and the block opener so the
    // highlight ends at the last non-space byte of the call.
    let mut end = opener.range.start as usize;
    while end > full.start as usize && source[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Range {
        start: full.start,
        end: end as u32,
    }
}

murphy_plugin_api::submit_cop!(EachWithObjectArgument);

#[cfg(test)]
mod tests {
    use super::EachWithObjectArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_integer_argument() {
        test::<EachWithObjectArgument>().expect_offense(indoc! {r#"
            collection.each_with_object(0) { |e, a| a + e }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The argument to each_with_object cannot be immutable.
        "#});
    }

    #[test]
    fn flags_symbol_argument() {
        test::<EachWithObjectArgument>().expect_offense(indoc! {r#"
            collection.each_with_object(:foo) { |e, a| a }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The argument to each_with_object cannot be immutable.
        "#});
    }

    #[test]
    fn flags_do_end_block_trims_to_call() {
        // The `do … end` block opener must be excluded from the offense range
        // just like the `{ … }` form; only the call portion is highlighted.
        test::<EachWithObjectArgument>().expect_offense(indoc! {r#"
            collection.each_with_object(0) do |e, a|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The argument to each_with_object cannot be immutable.
              a
            end
        "#});
    }

    #[test]
    fn flags_safe_navigation_call() {
        test::<EachWithObjectArgument>().expect_offense(indoc! {r#"
            collection&.each_with_object(0) { |e, a| a }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The argument to each_with_object cannot be immutable.
        "#});
    }

    #[test]
    fn accepts_mutable_array_argument() {
        test::<EachWithObjectArgument>()
            .expect_no_offenses("collection.each_with_object([]) { |e, a| a << e }\n");
    }

    #[test]
    fn accepts_mutable_hash_argument() {
        test::<EachWithObjectArgument>()
            .expect_no_offenses("collection.each_with_object({}) { |e, a| a }\n");
    }

    #[test]
    fn accepts_mutable_string_argument() {
        test::<EachWithObjectArgument>()
            .expect_no_offenses("collection.each_with_object('') { |e, a| a }\n");
    }

    #[test]
    fn accepts_no_argument() {
        test::<EachWithObjectArgument>()
            .expect_no_offenses("collection.each_with_object { |e, a| a }\n");
    }

    #[test]
    fn ignores_other_methods() {
        test::<EachWithObjectArgument>().expect_no_offenses("collection.reduce(0) { |a, e| a + e }\n");
    }

    #[test]
    fn flags_receiverless_call() {
        // murphy-if9y: `(call _ :each_with_object $_)` matches a receiverless
        // call too — the `_` receiver binds the absent (nil-filled) slot, just
        // as RuboCop's `(call _ ...)` matches `each_with_object(0)`.
        test::<EachWithObjectArgument>().expect_offense(indoc! {r#"
            each_with_object(0) { |e, a| a }
            ^^^^^^^^^^^^^^^^^^^ The argument to each_with_object cannot be immutable.
        "#});
    }
}
