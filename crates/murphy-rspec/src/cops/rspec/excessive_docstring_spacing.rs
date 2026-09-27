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
//!   `Sym` parts plus interpolation source (`#{x}`, mirroring upstream
//!   `text(node)` → `node.source` for non-str/sym); `Sym` uses the symbol name.
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
            NodeKind::Dstr(_) => dstr_text(cx, first),
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

/// Interpolated docstring text (murphy-bjrg.5): mirrors upstream `text(node)`
/// (`dstr → node_parts.map(text).join`, `str/sym → value`, else `node.source`).
/// Skipping interpolations would leave a trailing blank (`"of #{x}"` → `"of "`)
/// and false-flag Mastodon's 7 interpolated docstrings; using the interpolation
/// source (`#{x}`) avoids the false trailing/leading blank.
fn dstr_text(cx: &Cx<'_>, node: NodeId) -> String {
    match *cx.kind(node) {
        NodeKind::Str(id) => cx.string_str(id).to_owned(),
        NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
        NodeKind::Dstr(parts) => {
            let mut out = String::new();
            for &part in cx.list(parts) {
                out.push_str(&dstr_text(cx, part));
            }
            out
        }
        _ => {
            let range = cx.range(node);
            let src = cx.source();
            let start = range.start as usize;
            let end = range.end as usize;
            src.get(start..end).unwrap_or("").to_owned()
        }
    }
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
    fn does_not_flag_interpolated_docstring() {
        // murphy-bjrg.5: Mastodon 7 FPs, all interpolated (`"of #{error}"`).
        // Skipping the interpolation leaves `"of "` (trailing blank); upstream
        // uses `node.source` (`#{error}`) so no excessive whitespace.
        // Verified vs rubocop 1.91.0 full-config (TargetRubyVersion 3.3).
        test::<ExcessiveDocstringSpacing>().expect_no_offenses(indoc! {r#"
                it "Handles error class of #{error}" do; end
            "#});
    }

    #[test]
    fn flags_interpolated_with_double_space() {
        // True pin: double space outside interpolation still flags.
        test::<ExcessiveDocstringSpacing>().expect_offense(indoc! {r#"
                it "has  double #{x}" do; end
                   ^^^^^^^^^^^^^^^^^^ Excessive whitespace.
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
