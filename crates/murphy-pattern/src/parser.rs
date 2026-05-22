//! Recursive-descent parser: token stream -> `PatternAst`.
//!
//! Task 5 (murphy-9cr.17) implements the atom/prefix skeleton: `_`,
//! literals, `nil?`, bare kind names, `#predicate`, and the `!`/`^`/backtick
//! prefixes. Node match `(...)`, union `{}`, and `$` captures land in
//! Tasks 6-8 and currently produce a "not yet supported" error.

use crate::lexer::{Spanned, Token, tokenize};
use crate::{Lit, ParseError, Pat, PatKind, PatSpan, PatternAst};

/// Parse a pattern source string into a [`PatternAst`].
///
/// Tokenizes `src`, parses exactly one top-level pattern, and requires the
/// token stream to be fully consumed — leftover tokens are an error.
pub fn parse(src: &str) -> Result<PatternAst, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser::new(&tokens);
    let root = parser.prefixed()?;
    if let Some(extra) = parser.peek() {
        return Err(ParseError::new("unexpected trailing input", extra.span));
    }
    Ok(PatternAst {
        root,
        captures: Vec::new(),
    })
}

/// A cursor over a token slice for recursive-descent parsing.
struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned]) -> Parser<'a> {
        Parser { tokens, pos: 0 }
    }

    /// The token at the cursor, if any.
    fn peek(&self) -> Option<&'a Spanned> {
        self.tokens.get(self.pos)
    }

    /// Consume and return the token at the cursor, advancing the cursor.
    fn next(&mut self) -> Option<&'a Spanned> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// `prefixed := '!' prefixed | '^' prefixed | '`' prefixed | primary`.
    ///
    /// `$` capture-tail belongs here too but is deferred to Task 8.
    fn prefixed(&mut self) -> Result<Pat, ParseError> {
        let Some(head) = self.peek() else {
            // No source byte to point at for an empty stream.
            return Err(ParseError::new("empty pattern", PatSpan::new(0, 0)));
        };
        let prefix_span = head.span;
        let wrap: fn(Box<Pat>) -> PatKind = match head.tok {
            Token::Bang => PatKind::Not,
            Token::Caret => PatKind::Parent,
            Token::Backtick => PatKind::Descend,
            _ => return self.primary(),
        };
        self.pos += 1; // consume the prefix sigil
        let inner = self.prefixed()?;
        let span = PatSpan {
            start: prefix_span.start,
            end: inner.span.end,
        };
        Ok(Pat {
            kind: wrap(Box::new(inner)),
            span,
        })
    }

    /// `primary := '_' | 'nil?' | literal | '#' name | IDENT`.
    ///
    /// `(`, `{`, `$`, and a top-level `...` are deferred to later tasks and
    /// produce a descriptive error here.
    fn primary(&mut self) -> Result<Pat, ParseError> {
        let Some(spanned) = self.next() else {
            return Err(ParseError::new("empty pattern", PatSpan::new(0, 0)));
        };
        let span = spanned.span;
        let kind = match &spanned.tok {
            Token::Underscore => PatKind::Wildcard,
            Token::NilQuestion => PatKind::NilTest,
            Token::Int(v) => PatKind::Lit(Lit::Int(*v)),
            Token::Float(v) => PatKind::Lit(Lit::Float(*v)),
            Token::Str(s) => PatKind::Lit(Lit::Str(s.clone())),
            Token::Sym(s) => PatKind::Lit(Lit::Sym(s.clone())),
            Token::Predicate(name) => PatKind::Predicate(name.clone()),
            Token::Ident(name) => return self.ident_pat(name, span),
            Token::LParen => {
                return Err(ParseError::new(
                    "node match `(...)` is not yet supported",
                    span,
                ));
            }
            Token::LBrace => {
                return Err(ParseError::new("union `{...}` is not yet supported", span));
            }
            Token::Dollar => {
                return Err(ParseError::new("capture `$` is not yet supported", span));
            }
            Token::Ellipsis => {
                return Err(ParseError::new(
                    "`...` is only valid inside a node child list",
                    span,
                ));
            }
            Token::RParen => return Err(ParseError::new("unexpected `)`", span)),
            Token::RBrace => return Err(ParseError::new("unexpected `}`", span)),
            Token::Bang | Token::Caret | Token::Backtick => {
                // `prefixed` dispatches these; `primary` never sees them.
                unreachable!("prefix sigils are handled by `prefixed`");
            }
        };
        Ok(Pat { kind, span })
    }

    /// Classify a bare `IDENT`: the `true`/`false`/`nil` keywords become
    /// literals; any other name resolves to a node-kind tag, or errors.
    fn ident_pat(&self, name: &str, span: PatSpan) -> Result<Pat, ParseError> {
        let kind = match name {
            "true" => PatKind::Lit(Lit::True),
            "false" => PatKind::Lit(Lit::False),
            "nil" => PatKind::Lit(Lit::Nil),
            _ => match murphy_ast::tag_from_pattern_name(name) {
                Some(tag) => PatKind::Kind(tag),
                None => {
                    return Err(ParseError::new(format!("unknown node type `{name}`"), span));
                }
            },
        };
        Ok(Pat { kind, span })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lit, PatKind};

    fn k(src: &str) -> PatKind {
        parse(src).expect("parse ok").root.kind
    }

    #[test]
    fn parses_wildcard() {
        assert_eq!(k("_"), PatKind::Wildcard);
    }

    #[test]
    fn parses_literals() {
        assert_eq!(k("42"), PatKind::Lit(Lit::Int(42)));
        assert_eq!(k("-1"), PatKind::Lit(Lit::Int(-1)));
        assert_eq!(k("1.5"), PatKind::Lit(Lit::Float(1.5)));
        assert_eq!(k("\"s\""), PatKind::Lit(Lit::Str("s".into())));
        assert_eq!(k(":puts"), PatKind::Lit(Lit::Sym("puts".into())));
        assert_eq!(k("true"), PatKind::Lit(Lit::True));
        assert_eq!(k("false"), PatKind::Lit(Lit::False));
        assert_eq!(k("nil"), PatKind::Lit(Lit::Nil));
    }

    #[test]
    fn parses_nil_test_distinct_from_nil_literal() {
        assert_eq!(k("nil?"), PatKind::NilTest);
        assert_eq!(k("nil"), PatKind::Lit(Lit::Nil));
    }

    #[test]
    fn parses_bare_kind_name() {
        assert_eq!(k("send"), PatKind::Kind(murphy_ast::NodeKindTag(17)));
    }

    #[test]
    fn parses_predicate() {
        assert_eq!(k("#odd?"), PatKind::Predicate("odd?".into()));
    }

    #[test]
    fn parses_prefixes() {
        assert!(matches!(k("!_"), PatKind::Not(_)));
        assert!(matches!(k("^_"), PatKind::Parent(_)));
        assert!(matches!(k("`_"), PatKind::Descend(_)));
    }

    #[test]
    fn unknown_kind_name_is_span_error() {
        let e = parse("sned").expect_err("unknown kind");
        assert!(e.message.contains("sned"));
        assert_eq!((e.span.start, e.span.end), (0, 4));
    }

    #[test]
    fn rest_at_top_level_is_error() {
        assert!(parse("...").is_err());
    }

    // --- additional coverage --------------------------------------------

    #[test]
    fn nested_prefixes_compose() {
        // `!!_` is Not(Not(Wildcard)).
        let p = parse("!!_").expect("parse ok").root;
        let PatKind::Not(inner) = &p.kind else {
            panic!("outer should be Not, was {:?}", p.kind);
        };
        let PatKind::Not(innermost) = &inner.kind else {
            panic!("inner should be Not, was {:?}", inner.kind);
        };
        assert_eq!(innermost.kind, PatKind::Wildcard);
    }

    #[test]
    fn prefixed_span_spans_prefix_through_inner() {
        // `!_` — `!` at 0, `_` at 1; the Not span must cover 0..2.
        let p = parse("!_").expect("parse ok").root;
        assert_eq!((p.span.start, p.span.end), (0, 2));
    }

    #[test]
    fn unknown_kind_message_names_node_type() {
        let e = parse("sned").expect_err("unknown kind");
        assert!(
            e.message.contains("unknown node type"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn leftover_tokens_are_error() {
        // Two top-level patterns: the second `_` is leftover.
        let e = parse("_ _").expect_err("leftover tokens");
        assert_eq!((e.span.start, e.span.end), (2, 3));
    }

    #[test]
    fn empty_input_is_error() {
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
    }

    #[test]
    fn dangling_prefix_is_error() {
        // A prefix with nothing to apply to.
        assert!(parse("!").is_err());
    }

    #[test]
    fn no_captures_before_task8() {
        // Captures (`$`) are not implemented until Task 8; `parse` hardcodes
        // `captures: Vec::new()`, so every pattern parsed so far must report
        // zero captures. This pins the contract until Task 8 changes it.
        assert_eq!(parse("_").unwrap().n_captures(), 0);
        assert_eq!(parse("!send").unwrap().n_captures(), 0);
        assert_eq!(parse(":sym").unwrap().n_captures(), 0);
    }

    #[test]
    fn node_match_not_yet_supported() {
        let e = parse("(send)").expect_err("node match deferred");
        assert!(
            e.message.contains("not yet supported"),
            "message was: {}",
            e.message
        );
    }
}
