//! `Layout/TrailingWhitespace` — flags space / tab characters between
//! the last non-whitespace character on a line and the line's terminator.
//! Mirrors RuboCop's same-named cop.
//!
//! This is the raw-source vector of §12d: the cop scans `cx.source()`
//! directly rather than walking the arena. The dispatch surface is
//! `NodeCop::KINDS = &[]`, the file-visit form documented on
//! [`NodeCop`](murphy_plugin_api::NodeCop) — invoked exactly once per
//! file with `node == cx.root()`.
//!
//! ## Edge cases
//!
//! - **CRLF / Mac-style endings**: `\r\n` is the de-facto Ruby line
//!   terminator on Windows-written files; `\r` alone is essentially
//!   dead history. We treat `\r` as ordinary whitespace before a `\n` —
//!   trailing `\r` before EOL is a `Layout/TrailingWhitespace` offense
//!   too, so editors that auto-strip get pointed at it.
//! - **No final newline**: the last line still counts; trailing
//!   whitespace at EOF is reported on its own range.
//! - **Whitespace-only lines**: the whole line is trailing whitespace
//!   and reported as such.

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TrailingWhitespace;

#[cop(
    name = "Layout/TrailingWhitespace",
    description = "Flag space or tab characters between the last non-whitespace character on a line and the line terminator.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl TrailingWhitespace {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source();
        // Walk byte-by-byte so range offsets stay in the file's byte
        // index space (ADR 0001: offense ranges are byte offsets).
        let bytes = src.as_bytes();
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                emit_if_trailing(cx, bytes, line_start, i);
                line_start = i + 1;
            }
            i += 1;
        }
        // Last line — only flag if it has trailing whitespace. (A line
        // with zero whitespace at the end is clean; an unterminated
        // final line with no whitespace at all just means "no final
        // newline" which is a different cop's concern.)
        if line_start < bytes.len() {
            emit_if_trailing(cx, bytes, line_start, bytes.len());
        }
    }
}

/// Inspect bytes `[line_start, line_end)` (exclusive of the `\n` itself)
/// and emit an offense + edit if there is trailing whitespace.
fn emit_if_trailing(cx: &Cx<'_>, bytes: &[u8], line_start: usize, line_end: usize) {
    let mut trim = line_end;
    while trim > line_start && is_trailing_ws(bytes[trim - 1]) {
        trim -= 1;
    }
    if trim == line_end {
        return;
    }
    let range = Range {
        start: trim as u32,
        end: line_end as u32,
    };
    cx.emit_offense(range, "Trailing whitespace detected", None);
    cx.emit_edit(range, "");
}

/// Bytes that count as trailing whitespace for this cop. `\r` is in the
/// set so CRLF files get the leftover `\r` flagged before the `\n`.
fn is_trailing_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r')
}
