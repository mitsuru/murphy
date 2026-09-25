//! `RSpec/ImplicitSubject` — explicit `subject` vs implicit `is_expected` / `should` style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ImplicitSubject
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND`: `expect` /
//!   `is_expected` / `should` / `should_not`) with `EnforcedStyle`
//!   (`single_line_only` default, `single_statement_only`, `disallow`,
//!   `require_implicit`). `require_implicit` flags bare
//!   `expect(subject)` per `explicit_unnamed_subject?` with
//!   `Don't use explicit subject.` The other styles flag bare implicit
//!   subjects (`implicit_subject?`) outside `its` examples with
//!   `Don't use implicit subject.`: always under `disallow`, only in
//!   multiline examples under `single_line_only` (the example `Block`
//!   spans lines), only in multi-statement examples under
//!   `single_statement_only` (the example body is a `Begin`). The
//!   offense range is the whole node per `add_offense(node)`. Upstream
//!   `example?` is `Block`-only, so numbered-param (`Numblock`)
//!   examples never count here either. Detection is at parity;
//!   autocorrect (rewrite between spellings) is not ported in this
//!   batch — same convention as `RSpec/ImplicitExpect` (status:
//!   partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on bare `Send` (`methods = ["expect", "is_expected",
//! `"should", "should_not"]`). Default style `single_line_only`:
//!
//! - `it do is_expected.to be_truthy end` (multiline) — flagged.
//! - `it { is_expected.to be_truthy }` (single line) — clean.
//! - `it do should be_truthy end` (multiline) — flagged.
//! - `its(:size) do is_expected.to eq(1) end` — `its`, always clean.
//! - `is_expected.to be_truthy` outside an example — clean.
//!
//! Style `single_statement_only` flags only multi-statement examples;
//! `disallow` flags every implicit subject outside `its`;
//! `require_implicit` flags `expect(subject)` instead.
//!
//! ## No autocorrect
//!
//! Upstream rewrites between `is_expected` and `expect(subject)` (and
//! the `should` spellings). This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ImplicitSubject;

#[derive(CopOptions)]
pub struct ImplicitSubjectOptions {
    #[option(
        name = "EnforcedStyle",
        default = "single_line_only",
        description = "Whether implicit subject is allowed single-line, single-statement, never, or required."
    )]
    pub enforced_style: ImplicitSubjectStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ImplicitSubjectStyle {
    #[option(value = "single_line_only")]
    SingleLineOnly,
    #[option(value = "single_statement_only")]
    SingleStatementOnly,
    #[option(value = "disallow")]
    Disallow,
    #[option(value = "require_implicit")]
    RequireImplicit,
}

#[cop(
    name = "RSpec/ImplicitSubject",
    description = "Checks for usage of implicit subject (`is_expected` / `should`).",
    default_severity = "warning",
    default_enabled = true,
    options = ImplicitSubjectOptions,
)]
impl ImplicitSubject {
    #[on_node(kind = "send", methods = ["expect", "is_expected", "should", "should_not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let name = cx.symbol_str(method);
        let opts = cx.options_or_default::<ImplicitSubjectOptions>();
        if opts.enforced_style == ImplicitSubjectStyle::RequireImplicit {
            // Upstream `explicit_unnamed_subject?`: bare `expect` with a
            // single bare `subject` argument.
            if name != "expect" || !is_bare_subject_arg(cx, cx.list(args)) {
                return;
            }
            cx.emit_offense(cx.range(node), "Don't use explicit subject.", None);
            return;
        }
        if !matches!(name, "should" | "should_not" | "is_expected") {
            return;
        }
        let Some(example) = example_of(cx, node) else {
            return;
        };
        if is_its_block(cx, example) {
            return;
        }
        let invalid = match opts.enforced_style {
            ImplicitSubjectStyle::Disallow => true,
            ImplicitSubjectStyle::SingleLineOnly => !cx.is_single_line(example),
            ImplicitSubjectStyle::SingleStatementOnly => is_multi_statement(cx, example),
            ImplicitSubjectStyle::RequireImplicit => unreachable!(),
        };
        if !invalid {
            return;
        }
        cx.emit_offense(cx.range(node), "Don't use implicit subject.", None);
    }
}

/// `true` when `arg_ids` is a single bare `subject` send
/// (`explicit_unnamed_subject?`'s `(send nil? :subject)` argument).
fn is_bare_subject_arg(cx: &Cx<'_>, arg_ids: &[NodeId]) -> bool {
    if arg_ids.len() != 1 {
        return false;
    }
    let NodeKind::Send { receiver, method, .. } = *cx.kind(arg_ids[0]) else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "subject"
}

/// Example selectors (`Examples` in rubocop-rspec's default config):
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

/// The nearest enclosing example `Block` (`example?`: bare receiver,
/// `Examples.all` selector), if any.
fn example_of(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    cx.ancestors(node).find(|ancestor| {
        let NodeKind::Block { call, .. } = *cx.kind(*ancestor) else {
            return false;
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return false;
        };
        receiver == OptNodeId::NONE && is_example_name(cx.symbol_str(method))
    })
}

/// `true` when the example is an `its` block (implicit subjects there
/// describe the attribute, so every style leaves them alone).
fn is_its_block(cx: &Cx<'_>, example: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(example) else {
        return false;
    };
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return false;
    };
    cx.symbol_str(method) == "its"
}

/// `true` when the example body holds more than one statement (a
/// `Begin`), mirroring `!single_statement?`.
fn is_multi_statement(cx: &Cx<'_>, example: NodeId) -> bool {
    let NodeKind::Block { body, .. } = *cx.kind(example) else {
        return false;
    };
    let Some(body_id) = body.get() else {
        return false;
    };
    matches!(*cx.kind(body_id), NodeKind::Begin(_))
}

#[cfg(test)]
mod tests {
    use super::{ImplicitSubject, ImplicitSubjectOptions, ImplicitSubjectStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn single_statement_only() -> ImplicitSubjectOptions {
        ImplicitSubjectOptions {
            enforced_style: ImplicitSubjectStyle::SingleStatementOnly,
        }
    }

    fn disallow() -> ImplicitSubjectOptions {
        ImplicitSubjectOptions {
            enforced_style: ImplicitSubjectStyle::Disallow,
        }
    }

    fn require_implicit() -> ImplicitSubjectOptions {
        ImplicitSubjectOptions {
            enforced_style: ImplicitSubjectStyle::RequireImplicit,
        }
    }

    #[test]
    fn flags_multiline_is_expected_in_default_style() {
        test::<ImplicitSubject>().expect_offense(indoc! {r#"
                it do
                  is_expected.to be_truthy
                  ^^^^^^^^^^^ Don't use implicit subject.
                end
            "#});
    }

    #[test]
    fn flags_multiline_should_in_default_style() {
        test::<ImplicitSubject>().expect_offense(indoc! {r#"
                it do
                  should be_truthy
                  ^^^^^^^^^^^^^^^^ Don't use implicit subject.
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_line_in_default_style() {
        test::<ImplicitSubject>().expect_no_offenses(indoc! {r#"
                it { is_expected.to be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_its_in_default_style() {
        test::<ImplicitSubject>().expect_no_offenses(indoc! {r#"
                its(:size) do
                  is_expected.to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_implicit_subject_outside_example() {
        test::<ImplicitSubject>().expect_no_offenses(indoc! {r#"
                is_expected.to be_truthy
            "#});
    }

    #[test]
    fn flags_multi_statement_in_single_statement_only_style() {
        test::<ImplicitSubject>()
            .with_options(&single_statement_only())
            .expect_offense(indoc! {r#"
                it do
                  foo = 1
                  is_expected.to be_truthy
                  ^^^^^^^^^^^ Don't use implicit subject.
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_statement_in_single_statement_only_style() {
        test::<ImplicitSubject>()
            .with_options(&single_statement_only())
            .expect_no_offenses(indoc! {r#"
                it do
                  is_expected.to be_truthy
                end
            "#});
    }

    #[test]
    fn flags_single_line_in_disallow_style() {
        test::<ImplicitSubject>()
            .with_options(&disallow())
            .expect_offense(indoc! {r#"
                it { is_expected.to be_truthy }
                     ^^^^^^^^^^^ Don't use implicit subject.
            "#});
    }

    #[test]
    fn flags_expect_subject_in_require_implicit_style() {
        test::<ImplicitSubject>()
            .with_options(&require_implicit())
            .expect_offense(indoc! {r#"
                it { expect(subject).to be_truthy }
                     ^^^^^^^^^^^^^^^ Don't use explicit subject.
            "#});
    }

    #[test]
    fn does_not_flag_expect_other_in_require_implicit_style() {
        test::<ImplicitSubject>()
            .with_options(&require_implicit())
            .expect_no_offenses(indoc! {r#"
                it { expect(foo).to be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_is_expected_in_require_implicit_style() {
        test::<ImplicitSubject>()
            .with_options(&require_implicit())
            .expect_no_offenses(indoc! {r#"
                it { is_expected.to be_truthy }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ImplicitSubject);
