//! `RSpec/ExampleWithoutDescription` — checks for examples without a description.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExampleWithoutDescription
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example?`: `Block` only, bare receiver,
//!   `Examples.all` selectors) with `EnforcedStyle` (`always_allow`
//!   default, `single_line_only`, `disallow`). A single empty-string arg
//!   flags the `Str` node per `example_description` + `MSG_DEFAULT_ARGUMENT`
//!   in every style; an argless example flags the `Send` per
//!   `check_example_without_description` + `MSG_ADD_DESCRIPTION` only when
//!   the style disallows it (`disallow`, or `single_line_only` with a
//!   multiline block), except multiline `specify` which is always allowed.
//!   Multiline is measured by a newline inside the `Block` range, matching
//!   upstream's `node.parent.multiline?`. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare example (`it`, `specify`,
//! `example`, `scenario`, `its`, focused / skipped / pending variants):
//!
//! - `it('') { }` — empty arg, flagged (the `''`) in every style.
//! - `specify '' do; end` — empty arg, flagged.
//! - `it { }` — argless single-line, flagged only under `disallow`.
//! - `it do; end` multiline — flagged under `single_line_only` and
//!   `disallow`, clean under `always_allow`.
//! - `specify do; end` multiline — always clean (`specify` exception).
//! - `it 'desc' { }` — description present, not flagged.
//! - `it('desc', :focus) { }` — metadata form, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; writing the description needs human
//! judgement.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::send_without_block_range;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExampleWithoutDescription;

#[derive(CopOptions)]
pub struct ExampleWithoutDescriptionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "always_allow",
        description = "Whether auto-generated (undescribed) examples are always allowed, allowed only single-line, or disallowed."
    )]
    pub enforced_style: ExampleWithoutDescriptionStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ExampleWithoutDescriptionStyle {
    #[option(value = "always_allow")]
    AlwaysAllow,
    #[option(value = "single_line_only")]
    SingleLineOnly,
    #[option(value = "disallow")]
    Disallow,
}

#[cop(
    name = "RSpec/ExampleWithoutDescription",
    description = "Checks for examples without a description.",
    default_severity = "warning",
    default_enabled = true,
    options = ExampleWithoutDescriptionOptions,
)]
impl ExampleWithoutDescription {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_example_name(cx.symbol_str(method)) {
            return;
        }
        let opts = cx.options_or_default::<ExampleWithoutDescriptionOptions>();
        let arg_ids = cx.list(args);
        if arg_ids.is_empty() {
            if !disallows_argless(opts.enforced_style, cx, node, cx.symbol_str(method)) {
                return;
            }
            // Trim the wrapping block (`it { ... }` → `it`), since Murphy's
            // `Send` range covers the block while RuboCop's does not.
            cx.emit_offense(send_without_block_range(cx, call), "Add a description.", None);
            return;
        }
        // Upstream `example_description` matches a single `str` argument.
        if arg_ids.len() != 1 {
            return;
        }
        let NodeKind::Str(id) = *cx.kind(arg_ids[0]) else {
            return;
        };
        if !cx.string_str(id).is_empty() {
            return;
        }
        cx.emit_offense(
            cx.range(arg_ids[0]),
            "Omit the argument when you want to have auto-generated description.",
            None,
        );
    }
}

/// Example selectors (`Examples.all` in rubocop-rspec's default config):
/// regular (`it`, `specify`, `example`, `scenario`, `its`), focused
/// (`fit`, `fspecify`, `fexample`, `fscenario`, `focus`), skipped
/// (`xit`, `xspecify`, `xexample`, `xscenario`, `skip`) and pending
/// (`pending`).
fn is_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

/// `true` when an argless example needs a description under `style`.
///
/// Mirrors upstream `disallow_empty_description?` (`disallow`, or
/// `single_line_only` with a multiline block) plus the `specify`
/// exception (`specify` with a multiline block is always allowed).
fn disallows_argless(
    style: ExampleWithoutDescriptionStyle,
    cx: &Cx<'_>,
    block: NodeId,
    method: &str,
) -> bool {
    let disallowed = match style {
        ExampleWithoutDescriptionStyle::Disallow => true,
        ExampleWithoutDescriptionStyle::SingleLineOnly => is_multiline(cx, block),
        ExampleWithoutDescriptionStyle::AlwaysAllow => false,
    };
    if !disallowed {
        return false;
    }
    if method == "specify" && is_multiline(cx, block) {
        return false;
    }
    true
}

/// `true` when the `Block` range spans more than one line.
///
/// Mirrors upstream `node.parent.multiline?` (first line != last line).
fn is_multiline(cx: &Cx<'_>, block: NodeId) -> bool {
    let range = cx.range(block);
    let src = cx.source().as_bytes();
    let (Ok(lo), Ok(hi)) = (usize::try_from(range.start), usize::try_from(range.end)) else {
        return false;
    };
    if hi > src.len() || lo > hi {
        return false;
    }
    src[lo..hi].contains(&b'\n')
}

#[cfg(test)]
mod tests {
    use super::{
        ExampleWithoutDescription, ExampleWithoutDescriptionOptions,
        ExampleWithoutDescriptionStyle,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn single_line_only() -> ExampleWithoutDescriptionOptions {
        ExampleWithoutDescriptionOptions {
            enforced_style: ExampleWithoutDescriptionStyle::SingleLineOnly,
        }
    }

    fn disallow() -> ExampleWithoutDescriptionOptions {
        ExampleWithoutDescriptionOptions {
            enforced_style: ExampleWithoutDescriptionStyle::Disallow,
        }
    }

    #[test]
    fn flags_empty_string_arg_in_default_style() {
        test::<ExampleWithoutDescription>().expect_offense(indoc! {r#"
                it('') { is_expected.to be_good }
                   ^^ Omit the argument when you want to have auto-generated description.
            "#});
    }

    #[test]
    fn flags_empty_specify_arg_multiline() {
        test::<ExampleWithoutDescription>().expect_offense(indoc! {r#"
                specify '' do
                        ^^ Omit the argument when you want to have auto-generated description.
                end
            "#});
    }

    #[test]
    fn flags_empty_string_arg_in_disallow_style() {
        test::<ExampleWithoutDescription>()
            .with_options(&disallow())
            .expect_offense(indoc! {r#"
                it('') { is_expected.to be_good }
                   ^^ Omit the argument when you want to have auto-generated description.
            "#});
    }

    #[test]
    fn does_not_flag_description() {
        test::<ExampleWithoutDescription>().expect_no_offenses(indoc! {r#"
                it 'does something' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_argless_single_line_in_default_style() {
        test::<ExampleWithoutDescription>().expect_no_offenses(indoc! {r#"
                it { is_expected.to be_good }
            "#});
    }

    #[test]
    fn flags_argless_single_line_in_disallow_style() {
        test::<ExampleWithoutDescription>()
            .with_options(&disallow())
            .expect_offense(indoc! {r#"
                it { is_expected.to be_good }
                ^^ Add a description.
            "#});
    }

    #[test]
    fn flags_argless_multiline_in_single_line_only_style() {
        test::<ExampleWithoutDescription>()
            .with_options(&single_line_only())
            .expect_offense(indoc! {r#"
                it do
                ^^ Add a description.
                  foo
                end
            "#});
    }

    #[test]
    fn does_not_flag_argless_single_line_in_single_line_only_style() {
        test::<ExampleWithoutDescription>()
            .with_options(&single_line_only())
            .expect_no_offenses(indoc! {r#"
                it { is_expected.to be_good }
            "#});
    }

    #[test]
    fn does_not_flag_multiline_specify_in_disallow_style() {
        // The `specify` exception: multiline `specify` is always allowed.
        test::<ExampleWithoutDescription>()
            .with_options(&disallow())
            .expect_no_offenses(indoc! {r#"
                specify do
                  foo
                end
            "#});
    }

    #[test]
    fn flags_single_line_specify_in_disallow_style() {
        test::<ExampleWithoutDescription>()
            .with_options(&disallow())
            .expect_offense(indoc! {r#"
                specify { foo }
                ^^^^^^^ Add a description.
            "#});
    }

    #[test]
    fn does_not_flag_metadata_form() {
        test::<ExampleWithoutDescription>().expect_no_offenses(indoc! {r#"
                it('desc', :focus) { foo }
            "#});
    }

    #[test]
    fn does_not_flag_non_example_block() {
        test::<ExampleWithoutDescription>().expect_no_offenses(indoc! {r#"
                describe Foo do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExampleWithoutDescription);
