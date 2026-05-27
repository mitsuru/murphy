//! `Style/StringLiterals` — enforces a single quote style for plain
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringLiterals
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-j59g
//! notes: >
//!   Known gaps remain around EnforcedStyle naming, runtime config, dstr handling, message parity, and autocorrect breadth.
//! ```
//!
//! string literals. Mirrors RuboCop's same-named cop.
//!
//! Subscribes to `NodeKind::Str` (plain literal). Interpolated strings
//! (`NodeKind::Dstr`, `"a#{b}"`) are intentionally not subscribed: they
//! cannot be expressed with single quotes at all, so they are never a
//! `Style/StringLiterals` offense.
//!
//! ## Option (`preferred_quote`)
//!
//! Declared via `#[derive(CopOptions)]` and wired through the cop's
//! `Cop::Options` associated type. v1 ships the default
//! `preferred_quote = "single"` (matching RuboCop). The host-side
//! config-validation gate (murphy-9cr.9) consumes the generated
//! `SCHEMA` to enforce the enum at config-load time; the runtime
//! behaviour here uses the cop's `Default` until §12d's sibling
//! validation work lands.
//!
//! ## Autocorrect
//!
//! Range-edit replacing the surrounding quotes. The cop only emits an
//! autocorrect when the body content is unambiguously safe to swap:
//!
//! - **No backslashes** in the body (any `\` is an escape that means
//!   different things between `'…'` and `"…"`, e.g. `'\n'` = backslash-n
//!   vs `"\n"` = newline).
//! - **No `#`** in the body when converting *to* double quotes — `#{`
//!   in a double-quoted literal becomes interpolation rather than a
//!   literal `#`. The conservative rule is "any `#`" to keep the gate
//!   trivially correct.
//! - **No matching quote character** that would have to be re-escaped
//!   in the target style.
//!
//! When any of those fail the cop still emits the offense (the style
//! violation stands) but skips the edit so the user can hand-fix without
//! risk of a wrong autocorrect.

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct StringLiterals;

/// Cop options for [`StringLiterals`]. The `preferred_quote` value is
/// constrained to the `single` / `double` enum by the generated
/// [`OptionSpec::enum_values_json`](murphy_plugin_api::OptionSpec); the
/// `#[derive(CopOptions)]` macro builds the JSON schema entry that the
/// validation gate (murphy-9cr.9) reads.
#[derive(CopOptions)]
pub struct StringLiteralsOptions {
    #[option(
        default = "single",
        enum_values = ["single", "double"],
        description = "Preferred quote style for plain string literals."
    )]
    pub preferred_quote: String,
}

#[cop(
    name = "Style/StringLiterals",
    description = "Prefer one quote style (single / double) for plain string literals.",
    default_severity = "warning",
    default_enabled = true,
    options = StringLiteralsOptions
)]
impl StringLiterals {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Runtime option access (murphy-9cr.9) is not yet wired through
        // `Cx`; v1 honours the `Default` (`preferred_quote = "single"`).
        // The schema is exported regardless so the validation gate can
        // already enforce the enum at config-load time, and so the
        // future runtime-options path has nothing else to add here.
        let opts = StringLiteralsOptions::default();
        let prefer_single = opts.preferred_quote == "single";

        let range = cx.range(node);
        let src = cx.raw_source(range);
        let Some((actual, body)) = parse_quote_form(src) else {
            // %q / %Q / `?` char literal / similar — not a basic Str
            // literal even though the translator dropped it here.
            // Skip rather than guess.
            return;
        };

        let preferred = if prefer_single {
            QuoteStyle::Single
        } else {
            QuoteStyle::Double
        };
        if actual == preferred {
            return;
        }

        let (message, replacement) = match preferred {
            QuoteStyle::Single => (
                "Prefer single-quoted strings unless interpolation is needed",
                safe_swap(body, b'\'', b'"').map(|s| format!("'{s}'")),
            ),
            QuoteStyle::Double => (
                "Prefer double-quoted strings",
                safe_swap(body, b'"', b'\'').map(|s| format!("\"{s}\"")),
            ),
        };

        cx.emit_offense(range, message, None);
        if let Some(text) = replacement {
            cx.emit_edit(range, &text);
        }
        // Touch `Range` so the use stays load-bearing if a refactor drops it.
        let _ = std::mem::size_of::<Range>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteStyle {
    Single,
    Double,
}

/// Recognise `'…'` and `"…"` raw forms and split off the body. Returns
/// `None` for any other source shape (`%q[…]`, `?x`, heredoc head, …).
fn parse_quote_form(src: &str) -> Option<(QuoteStyle, &str)> {
    let bytes = src.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    match (first, last) {
        (b'\'', b'\'') => Some((QuoteStyle::Single, &src[1..src.len() - 1])),
        (b'"', b'"') => Some((QuoteStyle::Double, &src[1..src.len() - 1])),
        _ => None,
    }
}

/// Conservative quote-swap predicate. Returns the body unchanged when
/// safe to re-wrap with the *target* quote; `None` otherwise. Safety
/// rules are intentionally tight — see the module doc comment for the
/// invariants we are protecting.
fn safe_swap(body: &str, target_quote: u8, source_quote: u8) -> Option<&str> {
    // Any backslash: escapes have different meanings between the two
    // quote styles. Don't try to be clever.
    if body.as_bytes().contains(&b'\\') {
        return None;
    }
    // The target quote would have to be re-escaped if it appears in the
    // body, but we just decided to disallow backslashes — so the only
    // way to keep the swap byte-for-byte is to rule out the target
    // quote character entirely.
    if body.as_bytes().contains(&target_quote) {
        return None;
    }
    // `#` in the body when going to double quotes means risking
    // interpolation (`#{`, `#@…`, `#$…`). Conservatively forbid any
    // `#` so the rule is one line.
    if target_quote == b'"' && body.as_bytes().contains(&b'#') {
        return None;
    }
    // The source quote (about to vanish) was already a literal in the
    // body — if it appears the resulting target form would now contain
    // a bare quote. Rule it out: `'foo"bar'` → not a clean swap to
    // `"foo"bar"`.
    if body.as_bytes().contains(&source_quote) {
        return None;
    }
    Some(body)
}
