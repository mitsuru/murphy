//! `Lint/ConstantOverwrittenInRescue` — flag `rescue => SomeConstant`, where the
//! `=>` accidentally overwrites the constant with the caught exception instead
//! of catching that exception class.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ConstantOverwrittenInRescue
//! upstream_version_checked: 1.86.2
//! version_added: "1.31"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's matcher `(resbody nil? $(casgn _ _) nil?)` — a rescue
//!   clause with no exception class list, whose `=> Target` binding is a
//!   constant (translated to a value-less `Casgn`), and an empty body. The
//!   offense range is the `=>` token (RuboCop's `node.loc.assoc`) and the
//!   autocorrect deletes ` =>` between the `rescue` keyword and the constant,
//!   turning `rescue => StandardError` into `rescue StandardError`. The
//!   value-less `Casgn` shape for `rescue => Const` depends on the
//!   `translate_target` constant-target mapping (added alongside this cop);
//!   before that fix the binding parsed as `Unknown` and the pattern never
//!   matched.
//! ```
//!
//! ## Matched shapes
//!
//! A `Resbody` node whose `exceptions` list is empty, whose `var` binding is a
//! `Casgn` (the `=> Const` target), and whose body is empty. RuboCop's pattern
//! `(resbody nil? $(casgn _ _) nil?)` requires all three: `nil?` exceptions,
//! the captured `casgn`, and `nil?` body.
//!
//! ## Why this shape
//!
//! `rescue => Foo` binds the caught exception to `Foo`, overwriting the
//! constant. The fix is `rescue Foo` (catch the `Foo` exception). RuboCop only
//! fires when the body is empty because a non-empty body means the author also
//! used the bound value, which is a different (and unlikely-accidental) intent.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct ConstantOverwrittenInRescue;

#[cop(
    name = "Lint/ConstantOverwrittenInRescue",
    description = "Checks for overwriting an exception with an exception result by use `rescue =>`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ConstantOverwrittenInRescue {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody {
            exceptions,
            var,
            body,
        } = *cx.kind(node)
        else {
            return;
        };

        // `(resbody nil? $(casgn _ _) nil?)`: no exception class list and an
        // empty body.
        if !cx.list(exceptions).is_empty() || body.get().is_some() {
            return;
        }

        // The `=> Target` binding must be a constant (value-less `Casgn`).
        let Some(var_id) = var.get() else {
            return;
        };
        if !matches!(cx.kind(var_id), NodeKind::Casgn { .. }) {
            return;
        }

        // Locate the `=>` token between the `rescue` keyword and the constant.
        let Some(assoc) = assoc_range(node, var_id, cx) else {
            return;
        };

        let constant = cx.raw_source(cx.range(var_id));
        let message = format!("`{constant}` is overwritten by `rescue =>`.");
        cx.emit_offense(assoc, &message, None);

        // Delete from the end of `rescue` to the end of `=>`, collapsing
        // `rescue => Const` into `rescue Const`.
        let keyword_end = cx.range(node).start + "rescue".len() as u32;
        cx.emit_edit(
            Range {
                start: keyword_end,
                end: assoc.end,
            },
            "",
        );
    }
}

/// Source range of the `=>` token inside a `rescue => Const` resbody. Scans
/// tokens between the `rescue` keyword and the constant binding for the `=>`
/// operator (`SourceTokenKind::Other` with text `"=>"`).
fn assoc_range(resbody: NodeId, var: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let var_start = cx.range(var).start;
    let resbody_start = cx.range(resbody).start;
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < resbody_start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= var_start)
        .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "=>")
        .map(|t| t.range)
}

#[cfg(test)]
mod tests {
    use super::ConstantOverwrittenInRescue;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_constant_overwrite() {
        test::<ConstantOverwrittenInRescue>().expect_offense(indoc! {r#"
            begin
            rescue => StandardError
                   ^^ `StandardError` is overwritten by `rescue =>`.
            end
        "#});
    }

    #[test]
    fn flags_namespaced_constant_overwrite() {
        test::<ConstantOverwrittenInRescue>().expect_offense(indoc! {r#"
            begin
            rescue => Foo::Bar
                   ^^ `Foo::Bar` is overwritten by `rescue =>`.
            end
        "#});
    }

    #[test]
    fn corrects_to_rescue_without_assoc() {
        test::<ConstantOverwrittenInRescue>().expect_correction(
            indoc! {r#"
                begin
                rescue => StandardError
                       ^^ `StandardError` is overwritten by `rescue =>`.
                end
            "#},
            "begin\nrescue StandardError\nend\n",
        );
    }

    /// RuboCop parity (verified against 1.86.2): the autocorrect is
    /// `corrector.remove(range_between(keyword.end_pos, assoc.end_pos))` — a
    /// plain removal, not a normalize-to-one-space. For the space-less
    /// `rescue=>Const` form this collapses to `rescueConst`, byte-for-byte
    /// identical to RuboCop's own output. Replacing the span with `" "` would
    /// (a) diverge from RuboCop and (b) double the space in the common
    /// `rescue => Const` form, so the empty removal is the faithful port.
    #[test]
    fn corrects_spaceless_rescue_assoc_like_rubocop() {
        test::<ConstantOverwrittenInRescue>().expect_correction(
            indoc! {r#"
                begin
                rescue=>StandardError
                      ^^ `StandardError` is overwritten by `rescue =>`.
                end
            "#},
            "begin\nrescueStandardError\nend\n",
        );
    }

    #[test]
    fn does_not_flag_proper_rescue() {
        test::<ConstantOverwrittenInRescue>().expect_no_offenses(indoc! {r#"
            begin
            rescue StandardError
            end
        "#});
    }

    #[test]
    fn does_not_flag_local_variable_binding() {
        test::<ConstantOverwrittenInRescue>().expect_no_offenses(indoc! {r#"
            begin
            rescue => e
            end
        "#});
    }

    #[test]
    fn does_not_flag_class_with_variable_binding() {
        test::<ConstantOverwrittenInRescue>().expect_no_offenses(indoc! {r#"
            begin
            rescue StandardError => e
            end
        "#});
    }

    #[test]
    fn does_not_flag_when_body_present() {
        // `(resbody nil? $(casgn _ _) nil?)` requires an empty body.
        test::<ConstantOverwrittenInRescue>().expect_no_offenses(indoc! {r#"
            begin
            rescue => StandardError
              puts 1
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ConstantOverwrittenInRescue);
