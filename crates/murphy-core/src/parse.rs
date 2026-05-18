//! Parse adapter over the prism Rust binding (ADR 0001).
//!
//! Murphy parses each target file with prism **once** and exposes the shared
//! immutable AST to the rest of the engine. This module is the single entry
//! point for that parse.
//!
//! ## Ownership / lifetimes (ADR 0002, item 2)
//!
//! `ruby_prism::parse` returns a `ParseResult<'pr>` that *borrows* the source
//! bytes. A struct owning both the source and the `ParseResult` would be
//! self-referential and will not compile (E0106). Therefore [`Ast`] **borrows**
//! the caller's source: the Core owns the source buffer and the AST as
//! siblings for the file's processing scope, with the source outliving the AST.
//!
//! ## Offsets (ADR 0001)
//!
//! All offsets are **byte** offsets into the source (`u8` positions), never
//! char indices. [`ParseError::range`] reuses [`crate::Range`] and is in
//! bytes; slicing source must index by byte.

use crate::Range;
use std::fmt;

/// A successfully parsed Ruby AST, borrowing the source it was parsed from.
///
/// `'src` ties this tree to the source buffer the Core owns; the source must
/// outlive the `Ast` (ADR 0002).
pub struct Ast<'src> {
    result: ruby_prism::ParseResult<'src>,
}

impl fmt::Debug for Ast<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ParseResult is not Debug; the AST is opaque here. A bare marker is
        // enough for `Result::unwrap_err` and test diagnostics.
        f.write_str("Ast { .. }")
    }
}

impl<'src> Ast<'src> {
    /// Whether the AST has a root program node.
    ///
    /// prism returns a root program node for every successful parse, and an
    /// [`Ast`] is only constructed when the parse produced no errors, so this
    /// is truthfully always `true`.
    pub fn has_root(&self) -> bool {
        true
    }

    /// The root prism node, for traversal.
    ///
    /// This is the minimal node-visiting entry point Task 4 (cop dispatch)
    /// will use to walk the tree (e.g. via `ruby_prism::Visit`).
    pub fn root(&self) -> ruby_prism::Node<'_> {
        self.result.node()
    }

    /// The source bytes this AST was parsed from. Offense byte offsets
    /// (ADR 0001) index into exactly these bytes.
    pub fn source(&self) -> &'src [u8] {
        self.result.source()
    }
}

/// A structured parse failure.
///
/// Built from the **first** prism error when a parse is not error-free. The
/// `range` is in **byte** offsets (ADR 0001), reusing [`crate::Range`].
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
/// ADR 0001 fixes offsets at `u32` byte positions. A source larger than
/// `u32::MAX` bytes cannot be addressed without silently truncating offsets,
/// so [`parse`] rejects it as a structured error rather than corrupting
/// spans. Factored out so the bound is unit-testable without allocating a
/// multi-gigabyte string.
fn exceeds_offset_domain(len: usize) -> bool {
    len > u32::MAX as usize
}

/// Parse Ruby source into an [`Ast`].
///
/// prism is error-tolerant: on a syntax error it returns a *partial tree plus*
/// a non-empty error list. Per design §6 ("syntax-error file → 1 offense, skip
/// cops"), a non-empty error list yields `Err` built from the first error.
/// Never panics on any input.
///
/// The returned `Ast` borrows `src` (ADR 0002); the caller must keep `src`
/// alive for the AST's lifetime.
pub fn parse(src: &str) -> Result<Ast<'_>, ParseError> {
    // ADR 0001: offsets are `u32` byte positions. Reject oversized sources up
    // front so the `as u32` narrowing below is provably sound and never
    // silently truncates an offset. Structured error, not a panic/assert: the
    // contract is "total and panic-free", and release builds must catch this.
    if exceeds_offset_domain(src.len()) {
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

    let result = ruby_prism::parse(src.as_bytes());

    if let Some(err) = result.errors().next() {
        // prism `warnings()` are intentionally NOT surfaced in Phase 1: design
        // §6 centers offense reporting on syntax errors only.
        let loc = err.location();
        // The guard above proved `src.len() <= u32::MAX`, so every offset into
        // this source fits in `u32`; the narrowing is now enforced, not assumed.
        #[allow(clippy::cast_possible_truncation)]
        let range = Range {
            start_offset: loc.start_offset() as u32,
            end_offset: loc.end_offset() as u32,
        };
        let message = String::from_utf8_lossy(err.message().as_bytes()).into_owned();
        return Err(ParseError { message, range });
    }

    Ok(Ast { result })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_ruby_to_ast() {
        let ast = parse("puts 1\n").expect("should parse");
        assert!(ast.has_root());
    }

    #[test]
    fn syntax_error_is_structured_not_panic() {
        let err = parse("def (\n").unwrap_err();
        assert!(matches!(err, ParseError { .. }));
    }

    #[test]
    fn offset_domain_guard_is_correct_without_allocating() {
        // The 4GB+ case is verified via the unit-testable `exceeds_offset_domain`
        // boundary instead of allocating `u32::MAX` bytes (which would be a
        // ~4 GiB string): the guard in `parse` is a thin call to this fn.
        assert!(!exceeds_offset_domain(0));
        assert!(!exceeds_offset_domain(1024));
        assert!(!exceeds_offset_domain(u32::MAX as usize));
        assert!(exceeds_offset_domain(u32::MAX as usize + 1));
        assert!(exceeds_offset_domain(usize::MAX));

        // Normal/small source still parses successfully past the guard.
        let ast = parse("x = 1\n").expect("small source parses");
        assert!(ast.has_root());
        assert_eq!(ast.source(), b"x = 1\n");
    }
}
