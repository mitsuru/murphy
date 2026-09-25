//! `RSpec/ExcessiveDocstringSpacing` — avoid excessive whitespace in descriptions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExcessiveDocstringSpacing
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `example_description`
//!   (`(send #rspec? {#Examples.all #ExampleGroups.all} ${$str
//!   $(dstr ...)} ...)`): RSpec-or-bare receiver, example / group selector,
//!   first arg is `Str` / `Dstr` / `Sym`. `Dstr` text concatenates `Str` /
//!   `Sym` parts (interpolations skipped); `Sym` uses the symbol name.
//!   Excessive whitespace mirrors `excessive_whitespace?` (leading /
//!   trailing blank, or 2+ consecutive blanks surrounded by non-blanks).
//!   Offense range is the description node (upstream trims quotes to the
//!   inner docstring; range differs by quotes). Detection is at parity;
//!   autocorrect (collapse whitespace) is not ported in this batch — same
//!   convention as `RSpec/Eq` (status: partial, autocorrect as gap;
//!   heredoc skipping not ported).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with example / group selectors:
//!
//! - `it '  has  excessive   spacing  ' do; end` — flagged.
//! - `context '  when x   is met  ' do; end` — flagged.
//! - `it 'has excessive spacing' do; end` — clean.
//! - `it 'has  double' do; end` — flagged (double space).
//! - `it ' ok' do; end` — flagged (leading).
//! - `it 'ok ' do; end` — flagged (trailing).
//!
//! ## No autocorrect
//!
//! Upstream collapses whitespace. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, regex::Regex};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExcessiveDocstringSpacing;

#[cop(
    name = "RSpec/ExcessiveDocstringSpacing",
    description = "Checks for excessive whitespace in example descriptions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ExcessiveDocstringSpacing {
    #[on_node(
        kind = "send",
        methods = [
            "describe",
            "context",
            "feature",
            "example_group",
            "xdescribe",
            "xcontext",
            "xfeature",
            "fdescribe",
            "fcontext",
            "ffeature",
            "it",
            "specify",
            "example",
            "scenario",
            "its",
            "fit",
            "fspecify",
            "fexample",
            "fscenario",
            "focus",
            "xit",
            "xspecify",
            "xexample",
            "xscenario",
            "skip",
            "pending"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, args,
        } = *cx.kind(node)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if !is_example_or_group(cx.symbol_str(method)) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        let text = match *cx.kind(first) {
            NodeKind::Str(id) => cx.string_str(id).to_owned(),
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
            NodeKind::Dstr(parts) => {
                let mut out = String::new();
                for &part in cx.list(parts) {
                    match *cx.kind(part) {
                        NodeKind::Str(id) => out.push_str(cx.string_str(id)),
                        NodeKind::Sym(sym) => out.push_str(cx.symbol_str(sym)),
                        _ => {}
                    }
                }
                out
            }
            _ => return,
        };
        if !has_excessive_whitespace(&text) {
            return;
        }
        cx.emit_offense(cx.range(first), "Excessive whitespace.", None);
    }
}

fn is_example_or_group(name: &str) -> bool {
    matches!(
        name,
        "describe"
            | "context"
            | "feature"
            | "example_group"
            | "xdescribe"
            | "xcontext"
            | "xfeature"
            | "fdescribe"
            | "fcontext"
            | "ffeature"
            | "it"
            | "specify"
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

fn has_excessive_whitespace(text: &str) -> bool {
    let re = Regex::new(r"\A[[:blank:]]|[[:blank:]]\z|[^[[:space:]]][[:blank:]]{2,}[^[[:blank:]]]")
        .expect("valid whitespace regex");
    re.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::ExcessiveDocstringSpacing;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_leading_trailing_double() {
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                it '  has  excessive   spacing  ' do; end
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Excessive whitespace.
            "#});
    }

    #[test]
    fn flags_context_spacing() {
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                context '  when a condition   is met  ' do; end
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Excessive whitespace.
            "#});
    }

    #[test]
    fn flags_double_space() {
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                it 'has  double' do; end
                   ^^^^^^^^^^^^^ Excessive whitespace.
            "#});
    }

    #[test]
    fn flags_leading() {
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                it ' ok' do; end
                   ^^^^^ Excessive whitespace.
            "#});
    }

    #[test]
    fn flags_trailing() {
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                it 'ok ' do; end
                   ^^^^^ Excessive whitespace.
            "#});
    }

    #[test]
    fn does_not_flag_clean() {
        test::<ExcessiveDocstringSpacing>().expect_no_offenses(indoc! {r#"
                it 'has excessive spacing' do; end
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<ExcessiveDocstringSpacing>().expect_no_offenses(indoc! {r#"
                Other.it '  bad  ' do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExcessiveDocstringSpacing);
