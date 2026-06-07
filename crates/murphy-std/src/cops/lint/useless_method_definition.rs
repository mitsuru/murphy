//! `Lint/UselessMethodDefinition` — Checks for method definitions whose body
//! is nothing but a delegating `super` call (or `zsuper`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessMethodDefinition
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: def/defs dispatch, zsuper/super
//!   delegation detection, rest/optional/kwopt arg skipping, access-modifier
//!   parent handling (public/private/protected/module_function), non-modifier
//!   parent suppression, full autocorrect removal.
//! ```
//!
//! ## Matched shapes
//!
//! - `def method; super; end` / `def method\n  super\nend` (zsuper or super)
//! - `def self.method; super; end`
//! - `def method(arg); super(arg); end` (args must match exactly)
//! - `private def method; super; end` / `def method; super; end`
//!
//! ## Autocorrect
//!
//! Removes the entire method definition (or the modifier + definition pair).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct UselessMethodDefinition;

#[cop(
    name = "Lint/UselessMethodDefinition",
    description = "Checks for method definitions that only delegate to `super`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG: &str = "Useless method definition detected.";

fn check(node: NodeId, cx: &Cx<'_>) {
    // 1. Skip methods with rest / optional / optional-keyword args.
    let Some(args_id) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(args_list) = *cx.kind(args_id) else {
        return;
    };
    let args = cx.list(args_list);
    if args
        .iter()
        .any(|&a| matches!(*cx.kind(a), NodeKind::Restarg(_) | NodeKind::Optarg { .. } | NodeKind::Kwoptarg { .. }))
    {
        return;
    }

    // 2. If the parent is a Send whose method is NOT an access modifier
    //    (public / protected / private / module_function), skip — the def is
    //    being passed as an argument to something that may wrap it.
    if let Some(parent) = cx.parent(node).get() {
        if let NodeKind::Send { method, .. } = *cx.kind(parent) {
            let name = cx.symbol_str(method);
            if !matches!(name, "public" | "protected" | "private" | "module_function") {
                return;
            }
        }
    }

    // 3. Body must be present and be a delegation (zsuper or super with
    //    source-matching args).
    let Some(body_id) = cx.def_body(node).get() else {
        return;
    };

    let is_delegating = match *cx.kind(body_id) {
        NodeKind::Zsuper => true,
        NodeKind::Super(ref super_args) => {
            let super_args_list = cx.list(*super_args);
            if super_args_list.len() != args.len() {
                false
            } else {
                super_args_list
                    .iter()
                    .zip(args.iter())
                    .all(|(&sa, &a)| cx.raw_source(cx.range(sa)) == cx.raw_source(cx.range(a)))
            }
        }
        _ => false,
    };

    if !is_delegating {
        return;
    }

    // Emit offense on the first line of the method definition.
    cx.emit_offense(first_line_range(node, cx), MSG, None);

    // Autocorrect: remove the entire definition.  If the parent is a
    // modifier send (public/private/protected), remove the whole pair.
    let remove_id = cx.parent(node).get()
        .filter(|&p| matches!(*cx.kind(p), NodeKind::Send { .. }))
        .unwrap_or(node);
    let line_range = whole_line_range(remove_id, cx);
    cx.emit_edit(line_range, "");
}

/// Range covering just the first source line of `node`.
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}

/// Range covering the full line(s) of `node` (for removal).
fn whole_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let source = cx.source();

    let line_start = source[..range.start as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    let line_end = source[range.end as usize..]
        .find('\n')
        .map(|i| range.end as usize + i + 1)
        .unwrap_or(source.len());

    Range {
        start: line_start as u32,
        end: line_end as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::UselessMethodDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense tests ---

    #[test]
    fn flags_def_with_only_super() {
        test::<UselessMethodDefinition>().expect_correction(
            indoc! {r#"
                def method
                ^^^^^^^^^^ Useless method definition detected.
                  super
                end
            "#},
            "",
        );
    }

    #[test]
    fn flags_def_with_zsuper() {
        test::<UselessMethodDefinition>().expect_offense(indoc! {r#"
            def method; super; end
            ^^^^^^^^^^^^^^^^^^^^^^^ Useless method definition detected.
        "#});
    }

    #[test]
    fn flags_defs_with_super() {
        test::<UselessMethodDefinition>().expect_correction(
            indoc! {r#"
                def self.method
                ^^^^^^^^^^^^^^^ Useless method definition detected.
                  super
                end
            "#},
            "",
        );
    }

    #[test]
    fn flags_def_with_super_args() {
        test::<UselessMethodDefinition>().expect_correction(
            indoc! {r#"
                def method(arg)
                ^^^^^^^^^^^^^^^ Useless method definition detected.
                  super(arg)
                end
            "#},
            "",
        );
    }

    #[test]
    fn flags_modifier_def() {
        test::<UselessMethodDefinition>().expect_correction(
            indoc! {r#"
                private def method
                        ^^^^^^^^^^ Useless method definition detected.
                  super
                end
            "#},
            "",
        );
    }

    // --- acceptance tests ---

    #[test]
    fn accepts_def_with_additional_code() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def method
              super
              do_something
            end
        "#});
    }

    #[test]
    fn accepts_different_super_args() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def method1(foo)
              super(bar)
            end
        "#});
    }

    #[test]
    fn accepts_rest_args() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def method(*args)
              super
            end
        "#});
    }

    #[test]
    fn accepts_optional_arg() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def method(x = 1)
              super
            end
        "#});
    }

    #[test]
    fn accepts_optional_kwarg() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def method(x: 1)
              super
            end
        "#});
    }

    #[test]
    fn accepts_empty_constructor_with_args() {
        test::<UselessMethodDefinition>().expect_no_offenses(
            "def initialize(arg1, arg2); end\n",
        );
    }

    #[test]
    fn accepts_constructor_with_comments() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def initialize(arg)
              # Comment
            end
        "#});
    }

    #[test]
    fn accepts_constructor_with_additional_code() {
        test::<UselessMethodDefinition>().expect_no_offenses(indoc! {r#"
            def initialize(arg)
              super
              initialize_internals
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UselessMethodDefinition);
