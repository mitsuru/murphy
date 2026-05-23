//! Parse adapter: thin wrapper around [`murphy_translate::translate`].
//!
//! Returns an owned [`murphy_ast::Ast`] arena (ADR 0037); there is no
//! lifetime parameter on the success type — the host owns the source +
//! arena as siblings, but the arena does not borrow from the source.
//!
//! ## Syntax errors (ADR 0006)
//!
//! prism is error-tolerant: a malformed source yields a partial tree
//! plus a non-empty error list. Murphy reports the *first* error as a
//! single `Murphy/Syntax` offense (design §6); the file does not run
//! cops. To keep that contract, this function re-parses with prism
//! solely to harvest the first error before delegating to translate.
//! prism parse cost is well below dispatch cost on every Ruby corpus we
//! profile against, so re-parsing twice is acceptable; surfacing errors
//! through translate is a follow-up (out of murphy-9cr.22 scope).

use murphy_ast::Ast;
use ruby_prism as prism;
use std::path::PathBuf;

use crate::Range;
use std::fmt;

/// A structured parse failure. Mirrors the legacy `parse::ParseError` so
/// the CLI's `Murphy/Syntax` path is unchanged across the .22 cutover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable description of the syntax error.
    pub message: String,
    /// Byte-offset span of the offending source.
    pub range: Range,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "parse error at bytes {}..{}: {}",
            self.range.start_offset, self.range.end_offset, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Whether a source of `len` bytes exceeds the `u32` byte-offset domain.
///
/// ADR 0001 fixes offsets at `u32` byte positions; `murphy-ast::Range`
/// stores them in `u32`. A source larger than `u32::MAX` bytes cannot be
/// addressed without truncation, so [`parse`] rejects it as a structured
/// error rather than corrupting spans. Factored out so the bound is
/// unit-testable without allocating a multi-gigabyte string.
fn exceeds_offset_domain(len: usize) -> bool {
    len > u32::MAX as usize
}

/// Parse Ruby `source` (from `path`) into an arena [`Ast`].
///
/// On a syntax error, returns the first prism error as [`ParseError`].
/// The caller (CLI) maps that into a `Murphy/Syntax` offense and skips
/// the cop dispatch path for the file (ADR 0006).
pub fn parse(source: &str, path: impl Into<PathBuf>) -> Result<Ast, ParseError> {
    if exceeds_offset_domain(source.len()) {
        return Err(ParseError {
            message: "source exceeds the u32 byte-offset limit (ADR 0001): \
                      sources larger than u32::MAX bytes cannot be addressed \
                      without corrupting offsets"
                .to_owned(),
            range: Range {
                start_offset: 0,
                end_offset: 0,
            },
        });
    }

    {
        let result = prism::parse(source.as_bytes());
        if let Some(err) = result.errors().next() {
            let loc = err.location();
            let range = Range::from_prism_location(&loc);
            let message = String::from_utf8_lossy(err.message().as_bytes()).into_owned();
            return Err(ParseError { message, range });
        }
        // `result` borrows `source`; drop the borrow before translate
        // re-parses (translate does its own `prism::parse`).
    }

    Ok(murphy_translate::translate(source, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_ruby_to_arena_ast() {
        let ast = parse("puts 1\n", "t.rb").expect("should parse");
        assert!(!ast.is_empty(), "non-empty source yields non-empty arena");
    }

    #[test]
    fn syntax_error_is_structured_not_panic() {
        let err = parse("def (\n", "t.rb").unwrap_err();
        assert!(!err.message.is_empty(), "error carries a message");
    }

    #[test]
    fn offset_domain_guard_rejects_oversized_source() {
        assert!(!exceeds_offset_domain(0));
        assert!(!exceeds_offset_domain(u32::MAX as usize));
        assert!(exceeds_offset_domain(u32::MAX as usize + 1));
        assert!(exceeds_offset_domain(usize::MAX));

        let ast = parse("x = 1\n", "t.rb").expect("small source parses");
        assert!(!ast.is_empty());
    }
}
