//! `Layout/SpaceBeforeBrackets` — flags a space between a receiver and the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeBrackets
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-j1xd
//! notes: >
//!   Ports RuboCop's `on_send` for `[]` / `[]=` index sends: skip dotted
//!   calls (`obj.foo[bar]`, `collection.[](x)`, safe-nav), then flag any
//!   gap between the receiver's end and the `[` selector. Autocorrect
//!   removes the gap.
//!
//!   PARSER DIVERGENCE (documented gap): Murphy's Prism-based parser
//!   collapses `collection [index]` where `collection` is a *bare local
//!   variable / bare method name* into a regular method call with an
//!   array argument (`collection([index])`), not a `[]` index send. So
//!   the bare-receiver offense case RuboCop reports
//!   (`collection [index_or_key]`) is invisible to Murphy — there is no
//!   `[]` send to dispatch on. Unambiguous receivers (`@ivar [i]`,
//!   `@@cvar [i]`, `$gvar [i]`, `foo.bar(x) [i]`) DO parse as `[]` sends
//!   with the gap intact and are handled. Spaced index *assignment* with
//!   a bare-local receiver (`x [0] = 1`) is a Prism syntax error and
//!   cannot be linted at all.
//!
//!   This is an accepted parser-level divergence, not a fixable cop gap:
//!   detecting the collapsed `collection([index])` method-call shape and
//!   re-flagging it would false-positive on legitimate command calls with an
//!   array argument (`do_something [item]`, `expect(x).to eq []`), which are
//!   no-offense in both RuboCop and Murphy (and are pinned by tests below).
//!   No cop-level bypass exists, so this stays a documented divergence.
//! ```
//!
//! opening `[` of an index access (`@x [i]` → `@x[i]`). Mirrors RuboCop's
//! same-named cop for receivers Prism preserves as `[]` / `[]=` sends.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeBrackets;

#[cop(
    name = "Layout/SpaceBeforeBrackets",
    description = "Checks for receiver with a space before the opening brackets.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpaceBeforeBrackets {
    // RuboCop's `SpaceBeforeBrackets` only registers `on_send` (no `on_csend`).
    // A safe-navigation index call (`collection&.[](x)`) carries a `&.` dot
    // and would be excluded by the `loc.dot` guard regardless.
    #[on_node(kind = "send", methods = ["[]", "[]="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_index_send(node, cx);
    }
}

fn check_index_send(node: NodeId, cx: &Cx<'_>) {
    // RuboCop: `return if node.loc.dot`. A dotted index send
    // (`collection.[](x)`, `collection. [](x)`, `collection&.[](x)`) has
    // a call operator and is not this cop's concern.
    if cx.loc(node).dot() != Range::ZERO {
        return;
    }

    // RuboCop: `node.receiver.source_range.end_pos`.
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    let receiver_end = cx.range(receiver).end;

    // RuboCop: `node.loc.selector.begin_pos` — the `[` token start. Prism's
    // `loc.name` for a `[]` / `[]=` send covers the bracket selector; fall
    // back to scanning for the first `[` after the receiver if it is unset.
    let selector_begin = selector_begin(node, cx, receiver_end);
    let Some(selector_begin) = selector_begin else {
        return;
    };

    // RuboCop: `return if receiver_end_pos >= selector_begin_pos`.
    if receiver_end >= selector_begin {
        return;
    }

    let range = Range {
        start: receiver_end,
        end: selector_begin,
    };
    cx.emit_offense(range, "Remove the space before the opening brackets.", None);
    cx.emit_edit(range, "");
}

/// The byte offset of the `[` selector for an index send, preferring Prism's
/// `loc.name` and falling back to a token scan from `receiver_end`.
fn selector_begin(node: NodeId, cx: &Cx<'_>, receiver_end: u32) -> Option<u32> {
    let name = cx.loc(node).name;
    if name != Range::ZERO {
        let bytes = cx.source().as_bytes();
        // Only trust `loc.name` when it actually points at a `[`.
        if (name.start as usize) < bytes.len() && bytes[name.start as usize] == b'[' {
            return Some(name.start);
        }
    }

    // Fallback: first `[` after the receiver, within this node's range.
    // `[` is `SourceTokenKind::Other`, so match the source byte.
    let node_end = cx.range(node).end;
    let bytes = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < receiver_end);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            let s = t.range.start as usize;
            s < bytes.len() && bytes[s] == b'['
        })
        .map(|t| t.range.start)
}

#[cfg(test)]
mod tests {
    use super::SpaceBeforeBrackets;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_ivar_index_with_space() {
        test::<SpaceBeforeBrackets>().expect_correction(
            indoc! {r#"
                @collection [index_or_key]
                           ^ Remove the space before the opening brackets.
            "#},
            "@collection[index_or_key]\n",
        );
    }

    #[test]
    fn flags_cvar_index_with_space() {
        test::<SpaceBeforeBrackets>().expect_offense(indoc! {r#"
            @@collection [index_or_key]
                        ^ Remove the space before the opening brackets.
        "#});
    }

    #[test]
    fn flags_gvar_index_with_space() {
        test::<SpaceBeforeBrackets>().expect_offense(indoc! {r#"
            $collection [index_or_key]
                       ^ Remove the space before the opening brackets.
        "#});
    }

    #[test]
    fn flags_method_result_index_with_space() {
        test::<SpaceBeforeBrackets>().expect_offense(indoc! {r#"
            collection.call(arg) [index_or_key]
                                ^ Remove the space before the opening brackets.
        "#});
    }

    #[test]
    fn flags_and_corrects_index_assignment_with_space() {
        test::<SpaceBeforeBrackets>().expect_correction(
            indoc! {r#"
                @correction [index_or_key] = :value
                           ^ Remove the space before the opening brackets.
            "#},
            "@correction[index_or_key] = :value\n",
        );
    }

    #[test]
    fn flags_index_assignment_with_inner_space_too() {
        // RuboCop reports only the gap before `[`; inner-bracket spacing is
        // SpaceInsideReferenceBrackets' concern.
        test::<SpaceBeforeBrackets>().expect_offense(indoc! {r#"
            @correction [ index_or_key] = :value
                       ^ Remove the space before the opening brackets.
        "#});
    }

    #[test]
    fn accepts_ivar_index_without_space() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("@collection[index_or_key]\n");
    }

    #[test]
    fn accepts_index_assignment_without_space() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("@correction[index_or_key] = :value\n");
    }

    #[test]
    fn accepts_inner_bracket_spacing_only() {
        test::<SpaceBeforeBrackets>()
            .expect_no_offenses("@collections[ index_or_key ] = :value\n")
            .expect_no_offenses("@collections[  index_or_key] = value\n");
    }

    #[test]
    fn accepts_standalone_array_literal() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("[index_or_key]\n");
    }

    #[test]
    fn accepts_array_in_parentheses() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("do_something([item_of_array_literal])\n");
    }

    #[test]
    fn accepts_desugared_index_call() {
        test::<SpaceBeforeBrackets>()
            .expect_no_offenses("collection.[](index_or_key)\n")
            .expect_no_offenses("collection.[]=(index_or_key, value)\n");
    }

    #[test]
    fn accepts_dotted_index_call_with_space_after_dot() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("collection. [](index_or_key)\n");
    }

    #[test]
    fn accepts_dotted_index_call_with_space_before_dot() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("collection .[](index_or_key)\n");
    }

    #[test]
    fn accepts_safe_nav_index_call() {
        test::<SpaceBeforeBrackets>().expect_no_offenses("collection&. [](index_or_key)\n");
    }

    #[test]
    fn accepts_method_call_with_array_arg() {
        // `do_something [item]` is a command call with an array argument, not
        // an index send — no offense, in both RuboCop and Murphy.
        test::<SpaceBeforeBrackets>()
            .expect_no_offenses("do_something [item_of_array_literal]\n")
            .expect_no_offenses("do_something [foo], bar\n")
            .expect_no_offenses("expect(offenses).to eq []\n");
    }
}
murphy_plugin_api::submit_cop!(SpaceBeforeBrackets);
