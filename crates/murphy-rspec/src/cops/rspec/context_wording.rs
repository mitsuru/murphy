//! `RSpec/ContextWording` — checks that `context` docstrings start with an allowed prefix.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ContextWording
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `context_wording`
//!   (`(block (send #rspec? { :context :shared_context } $({str dstr xstr}
//!   ...) ...) ...)`): the first arg must be a string literal. The
//!   description matches when it starts with one of `Prefixes` (default
//!   `when` / `with` / `without`, word-boundary anchored like upstream's
//!   `/^prefix\b/`) or matches one of `AllowedPatterns` (regexes, default
//!   empty). With neither configured the cop always reports per
//!   `MSG_ALWAYS`. `Dstr` / `Xstr` descriptions use the concatenated
//!   leading literal text (interpolations skipped); `Xstr` (backticks) is
//!   matched the same way. The offense range is the string-arg node per
//!   `add_offense(context)`. Messages mirror `MSG_MATCH` / `MSG_ALWAYS`.
//!   No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare or `RSpec`-namespaced
//! `context` / `shared_context` with a string first arg:
//!
//! - `context 'the display' do; end` — no prefix, flagged.
//! - `context 'when the display' do; end` — prefix, not flagged.
//! - `context 'with foo' do; end` — prefix, not flagged.
//! - `shared_context 'without bar' do; end` — prefix, not flagged.
//! - `context 'whenever x' do; end` — `when` without a word boundary,
//!   flagged.
//! - `describe 'the display' do; end` — not a context, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; rewording the description needs human
//! judgement.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, regex::Regex};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ContextWording;

#[derive(CopOptions)]
pub struct ContextWordingOptions {
    #[option(
        name = "Prefixes",
        default = ["when", "with", "without"],
        description = "Prefixes that context descriptions must start with (word-boundary anchored)."
    )]
    pub prefixes: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns that context descriptions may match instead of a prefix."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "RSpec/ContextWording",
    description = "Checks that context docstring starts with an allowed prefix.",
    default_severity = "warning",
    default_enabled = true,
    options = ContextWordingOptions,
)]
impl ContextWording {
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
        if !matches!(cx.symbol_str(method), "context" | "shared_context") {
            return;
        }
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !matches!(
            *cx.kind(first),
            NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Xstr(_)
        ) {
            return;
        }
        let opts = cx.options_or_default::<ContextWordingOptions>();
        let description = literal_text(cx, first);
        if description_matches(&description, &opts) {
            return;
        }
        cx.emit_offense(cx.range(first), &match_message(&opts), None);
    }
}

/// Leading literal text of a string node: `Str` content, or the
/// concatenated `Str` parts of a `Dstr` / `Xstr` (interpolations skipped).
fn literal_text(cx: &Cx<'_>, node: NodeId) -> String {
    match *cx.kind(node) {
        NodeKind::Str(id) => cx.string_str(id).to_owned(),
        NodeKind::Dstr(parts) | NodeKind::Xstr(parts) => {
            let mut out = String::new();
            for &part in cx.list(parts) {
                if let NodeKind::Str(id) = *cx.kind(part) {
                    out.push_str(cx.string_str(id));
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// `true` when `description` starts with a configured prefix (word-boundary
/// anchored, mirroring `/^prefix\b/`) or matches a configured regex.
fn description_matches(description: &str, opts: &ContextWordingOptions) -> bool {
    if opts.prefixes.iter().any(|pre| has_word_prefix(description, pre)) {
        return true;
    }
    opts.allowed_patterns.iter().any(|pat| {
        Regex::new(pat).is_ok_and(|re| re.is_match(description))
    })
}

/// `true` when `description` starts with `prefix` followed by a word
/// boundary (end of string or a non-word byte), mirroring Ruby's
/// `/^escaped\b/` where `\w` is ASCII `[A-Za-z0-9_]`.
fn has_word_prefix(description: &str, prefix: &str) -> bool {
    if !description.starts_with(prefix) {
        return false;
    }
    match description.as_bytes().get(prefix.len()) {
        None => true,
        Some(b) => !b.is_ascii_alphanumeric() && *b != b'_',
    }
}

/// Upstream `message`: `MSG_ALWAYS` when nothing is configured, else
/// `MSG_MATCH` with the `/pattern/` list (`or` before the last item).
fn match_message(opts: &ContextWordingOptions) -> String {
    let mut patterns: Vec<String> = opts
        .prefixes
        .iter()
        .map(|pre| format!("/^{pre}\\b/"))
        .collect();
    for pat in &opts.allowed_patterns {
        // Invalid patterns never match; keep the message truthful by
        // listing only what is enforced.
        if Regex::new(pat).is_ok() {
            patterns.push(format!("/{pat}/"));
        }
    }
    if patterns.is_empty() {
        return "Current settings will always report an offense. Please add allowed words to `Prefixes` or `AllowedPatterns`.".to_owned();
    }
    if patterns.len() == 1 {
        return format!("Context description should match {}.", patterns[0]);
    }
    let last = patterns.pop().expect("len > 1");
    format!("Context description should match {}, or {}.", patterns.join(", "), last)
}

#[cfg(test)]
mod tests {
    use super::{ContextWording, ContextWordingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn no_prefixes() -> ContextWordingOptions {
        ContextWordingOptions {
            prefixes: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    fn with_patterns() -> ContextWordingOptions {
        ContextWordingOptions {
            prefixes: vec!["when".to_owned()],
            allowed_patterns: vec!["とき$".to_owned()],
        }
    }

    #[test]
    fn flags_context_without_prefix() {
        test::<ContextWording>().expect_offense(indoc! {r#"
                context 'the display name not present' do; end
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Context description should match /^when\b/, /^with\b/, or /^without\b/.
            "#});
    }

    #[test]
    fn does_not_flag_when_prefix() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                context 'when the display name is not present' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_prefix() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                context 'with foo' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_shared_context_prefix() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                shared_context 'without bar' do
                end
            "#});
    }

    #[test]
    fn flags_prefix_without_word_boundary() {
        // `whenever` starts with `when` but has no word boundary.
        test::<ContextWording>().expect_offense(indoc! {r#"
                context 'whenever x' do; end
                        ^^^^^^^^^^^^ Context description should match /^when\b/, /^with\b/, or /^without\b/.
            "#});
    }

    #[test]
    fn flags_empty_prefix_config() {
        test::<ContextWording>()
            .with_options(&no_prefixes())
            .expect_offense(indoc! {r#"
                context 'when x' do; end
                        ^^^^^^^^ Current settings will always report an offense. Please add allowed words to `Prefixes` or `AllowedPatterns`.
            "#});
    }

    #[test]
    fn matches_allowed_pattern() {
        test::<ContextWording>()
            .with_options(&with_patterns())
            .expect_no_offenses(indoc! {r#"
                context '条件を満たすとき' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_rspec_context_prefix() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                RSpec.context 'when x' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_context() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                describe 'the display' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_string_arg() {
        test::<ContextWording>().expect_no_offenses(indoc! {r#"
                context foo do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ContextWording);
