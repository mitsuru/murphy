//! `RSpec/ExampleWording` — avoid `should` / `will` / `it` prefixes and insufficient descriptions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExampleWording
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `it_description`
//!   (`(block (send _ :it ${(str $_) (dstr (str $_) ...)} ...) ...)`) for
//!   `it` blocks (any receiver per upstream `_`): `Str` descriptions use
//!   the string value, `Dstr` uses the concatenated `Str` parts
//!   (interpolations skipped). Prefix checks mirror `SHOULD_PREFIX`
//!   (`should` / `shouldn't` / `shouldn’t`, word-boundary, case-insensitive),
//!   `WILL_PREFIX` (`will` / `won't` / `won’t`), and `IT_PREFIX` (`it `).
//!   Insufficient descriptions mirror `insufficient_docstring?`
//!   (`strip.squeeze(' ').downcase` in `DisallowedExamples`, default
//!   `works`). Offense range is the description node per
//!   `add_offense(docstring)` simplified to the full string range
//!   (upstream trims quotes; range differs by quotes). Detection is at
//!   parity; autocorrect (rewriting via `Wording`, `CustomTransform` /
//!   `IgnoredWords`) is not ported in this batch — same convention as
//!   `RSpec/Eq` (status: partial, autocorrect as gap; `CustomTransform` /
//!   `IgnoredWords` accepted as config but unused).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is `it` (any receiver):
//!
//! - `it 'should find nothing' do; end` — flagged (`should`).
//! - `it 'will find nothing' do; end` — flagged (`will`).
//! - `it 'it does things' do; end` — flagged (`it`).
//! - `it 'works' do; end` — flagged (insufficient).
//! - `it 'finds nothing' do; end` — clean.
//! - `it 'Should do x' do; end` — case-insensitive, flagged.
//! - `specify 'should x' do; end` — not `it`, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream rewrites the docstring (`Wording#rewrite`, `CustomTransform`).
//! This batch reports only.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, regex::Regex};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExampleWording;

#[derive(CopOptions)]
pub struct ExampleWordingOptions {
    #[option(
        name = "DisallowedExamples",
        default = ["works"],
        description = "Example descriptions considered insufficient (case-insensitive, whitespace-squeezed)."
    )]
    pub disallowed_examples: Vec<String>,
}

#[cop(
    name = "RSpec/ExampleWording",
    description = "Checks for common mistakes in example descriptions.",
    default_severity = "warning",
    default_enabled = true,
    options = ExampleWordingOptions,
)]
impl ExampleWording {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send { method, args, .. } = *cx.kind(call) else {
            return;
        };
        if cx.symbol_str(method) != "it" {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        let text = match *cx.kind(first) {
            NodeKind::Str(id) => cx.string_str(id).to_owned(),
            NodeKind::Dstr(parts) => {
                let mut out = String::new();
                for &part in cx.list(parts) {
                    if let NodeKind::Str(id) = *cx.kind(part) {
                        out.push_str(cx.string_str(id));
                    }
                }
                out
            }
            _ => return,
        };
        // Upstream order: should, will, it, insufficient.
        if matches_should(&text) {
            cx.emit_offense(
                cx.range(first),
                "Do not use should when describing your tests.",
                None,
            );
            return;
        }
        if matches_will(&text) {
            cx.emit_offense(
                cx.range(first),
                "Do not use the future tense when describing your tests.",
                None,
            );
            return;
        }
        if matches_it(&text) {
            cx.emit_offense(
                cx.range(first),
                "Do not repeat 'it' when describing your tests.",
                None,
            );
            return;
        }
        let opts = cx.options_or_default::<ExampleWordingOptions>();
        if is_insufficient(&text, &opts) {
            cx.emit_offense(
                cx.range(first),
                "Your example description is insufficient.",
                None,
            );
        }
    }
}

fn matches_should(text: &str) -> bool {
    // /\Ashould(?:n't|n’t)?\b/i — ASCII + curly apostrophe.
    let re = Regex::new(r"(?i)\Ashould(?:n't|n’t)?\b").expect("valid should regex");
    re.is_match(text)
}

fn matches_will(text: &str) -> bool {
    // /\A(?:will|won't|won’t)\b/i
    let re = Regex::new(r"(?i)\A(?:will|won't|won’t)\b").expect("valid will regex");
    re.is_match(text)
}

fn matches_it(text: &str) -> bool {
    // /\Ait /i
    let re = Regex::new(r"(?i)\Ait ").expect("valid it regex");
    re.is_match(text)
}

fn preprocess(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_insufficient(text: &str, opts: &ExampleWordingOptions) -> bool {
    let needle = preprocess(text);
    opts.disallowed_examples
        .iter()
        .any(|ex| preprocess(ex) == needle)
}

#[cfg(test)]
mod tests {
    use super::ExampleWording;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_should() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it 'should find nothing' do; end
                   ^^^^^^^^^^^^^^^^^^^^^ Do not use should when describing your tests.
            "#});
    }

    #[test]
    fn flags_should_case_insensitive() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it 'Should do x' do; end
                   ^^^^^^^^^^^^^ Do not use should when describing your tests.
            "#});
    }

    #[test]
    fn flags_shouldnt() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it "shouldn't do x" do; end
                   ^^^^^^^^^^^^^^^^ Do not use should when describing your tests.
            "#});
    }

    #[test]
    fn flags_will() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it 'will find nothing' do; end
                   ^^^^^^^^^^^^^^^^^^^ Do not use the future tense when describing your tests.
            "#});
    }

    #[test]
    fn flags_it_prefix() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it 'it does things' do; end
                   ^^^^^^^^^^^^^^^^ Do not repeat 'it' when describing your tests.
            "#});
    }

    #[test]
    fn flags_insufficient_works() {
        test::<ExampleWording>().expect_offense(indoc! {r#"
                it 'works' do; end
                   ^^^^^^^ Your example description is insufficient.
            "#});
    }

    #[test]
    fn does_not_flag_good() {
        test::<ExampleWording>().expect_no_offenses(indoc! {r#"
                it 'finds nothing' do; end
            "#});
    }

    #[test]
    fn does_not_flag_specify() {
        test::<ExampleWording>().expect_no_offenses(indoc! {r#"
                specify 'should x' do; end
            "#});
    }

    #[test]
    fn does_not_flag_no_arg() {
        test::<ExampleWording>().expect_no_offenses(indoc! {r#"
                it { do_something }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExampleWording);
