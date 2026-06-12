//! Shared logic for `Layout/SpaceBeforeComma` and
//! `Layout/SpaceBeforeSemicolon`, mirroring RuboCop's
//! `SpaceBeforePunctuation` mixin.
//!
//! This module is **not a cop** — it holds the common `each_missing_space`
//! walk so the two punctuation cops stay byte-for-byte in sync with the
//! upstream mixin. It is auto-discovered by `automod::dir!` but defines no
//! `#[cop]`, so it registers nothing.

use murphy_plugin_api::{Cx, Range, SourceToken, SourceTokenKind};

/// Walk consecutive token pairs and, for every pair `(token1, token2)` where
/// `token2` is a target punctuation token preceded by inline whitespace on
/// the same line, emit an offense over that whitespace gap and an autocorrect
/// edit that removes it.
///
/// `is_target` identifies the punctuation token (comma / semicolon). `kind`
/// is the human-readable token name interpolated into the offense message
/// (`"comma"` / `"semicolon"`), matching RuboCop's `format(MSG, token: ...)`.
pub fn check_space_before_punctuation(
    cx: &Cx<'_>,
    is_target: impl Fn(&Cx<'_>, SourceToken) -> bool,
    kind: &str,
) {
    for pair in cx.sorted_tokens().windows(2) {
        let token1 = pair[0];
        let token2 = pair[1];

        if !is_target(cx, token2) {
            continue;
        }
        if !space_missing(token1, token2) {
            continue;
        }
        if space_required_after(cx, token1) {
            continue;
        }

        let gap = Range {
            start: token1.range.end,
            end: token2.range.start,
        };
        // Defensive: only treat a pure-whitespace inline gap as removable.
        // `space_missing` already enforces same-line (no `\n`); guard against
        // any non-space bytes (e.g. an unexpected token quirk) before editing.
        if !cx
            .raw_source(gap)
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t'))
        {
            continue;
        }

        cx.emit_offense(gap, &format!("Space found before {kind}."), None);
        cx.emit_edit(gap, "");
    }
}

/// RuboCop's `space_missing?`: there is at least one byte between the two
/// tokens (`token2.begin_pos > token1.end_pos`). The same-line requirement is
/// enforced by the caller's whitespace-only gap check — a gap containing a
/// `\n` (a leading-punctuation continuation shape like `,\n  next`) fails that
/// check and is not flagged.
fn space_missing(token1: SourceToken, token2: SourceToken) -> bool {
    token2.range.start > token1.range.end
}

/// RuboCop's `space_required_after?`: a `{` immediately before the
/// punctuation is exempt when `Layout/SpaceInsideBlockBraces` uses the
/// `space` style (its default). Murphy cannot read another cop's config from
/// inside a cop, so it applies the default-style behavior unconditionally and
/// exempts any preceding `{`. A `,`/`;` directly after `{` is a near-impossible
/// Ruby shape, so this divergence is inert in practice.
fn space_required_after(_cx: &Cx<'_>, token1: SourceToken) -> bool {
    token1.kind == SourceTokenKind::LeftBrace
}
