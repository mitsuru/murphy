//! Recursive-descent parser: token stream -> `PatternAst`.
//!
//! Task 5 (murphy-9cr.17) implements the atom/prefix skeleton: `_`,
//! literals, `nil?`, bare kind names, `#predicate`, and the `!`/`^`/backtick
//! prefixes. Task 6 adds node match `(head child*)` with `Exact`/`Any`/`OneOf`
//! heads. Union `{}` and `$` captures land in Tasks 7-8 and currently produce
//! a "not yet supported" error.

use crate::lexer::{Spanned, Token, tokenize};
use crate::{Head, Lit, ParseError, Pat, PatKind, PatSpan, PatternAst};
use murphy_ast::NodeKindTag;

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

    /// `primary := '_' | 'nil?' | literal | '#' name | IDENT | node-match`.
    ///
    /// `{`, `$`, and a top-level `...` are deferred to later tasks and produce
    /// a descriptive error here; `(` parses a node match (see [`node_match`]).
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
            Token::LParen => return self.node_match(span),
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

    /// `node_match := '(' head child* ')'`.
    ///
    /// `open_span` is the span of the already-consumed `(`. The resulting
    /// `Pat`'s span covers `(` through the closing `)`. Children are parsed
    /// with [`prefixed`], except a `...` in a child slot becomes [`PatKind::Rest`]
    /// (at most one per child list).
    fn node_match(&mut self, open_span: PatSpan) -> Result<Pat, ParseError> {
        let head = self.node_head(open_span)?;
        let mut children: Vec<Pat> = Vec::new();
        let mut has_rest = false;
        loop {
            // Peek (not `next`) so we can dispatch on the token — closing `)`,
            // a `...`, or a child — before deciding whether to consume it.
            let Some(tok) = self.peek() else {
                // Ran out of input before the closing `)`.
                return Err(ParseError::new("unclosed `(`: expected `)`", open_span));
            };
            match tok.tok {
                Token::RParen => {
                    let close_span = tok.span;
                    self.pos += 1; // consume `)`
                    return Ok(Pat {
                        kind: PatKind::Node { head, children },
                        span: PatSpan {
                            start: open_span.start,
                            end: close_span.end,
                        },
                    });
                }
                Token::Ellipsis => {
                    let ell_span = tok.span;
                    self.pos += 1; // consume `...`
                    if has_rest {
                        return Err(ParseError::new(
                            "`...` may appear at most once in a node child list",
                            ell_span,
                        ));
                    }
                    has_rest = true;
                    children.push(Pat {
                        kind: PatKind::Rest,
                        span: ell_span,
                    });
                }
                _ => children.push(self.prefixed()?),
            }
        }
    }

    /// Parse the head of a node match: `IDENT` -> [`Head::Exact`], `_` ->
    /// [`Head::Any`], `{ IDENT+ }` -> [`Head::OneOf`]. `open_span` is the span
    /// of the node's `(`, used when the stream ends before a head is seen.
    fn node_head(&mut self, open_span: PatSpan) -> Result<Head, ParseError> {
        let Some(spanned) = self.next() else {
            return Err(ParseError::new("unclosed `(`: expected `)`", open_span));
        };
        match &spanned.tok {
            Token::Ident(name) => Ok(Head::Exact(resolve_kind(name, spanned.span)?)),
            Token::Underscore => Ok(Head::Any),
            Token::LBrace => self.oneof_head(spanned.span),
            _ => Err(ParseError::new(
                "a node match needs a head: a node type, `_`, or `{...}`",
                spanned.span,
            )),
        }
    }

    /// Parse a `{ IDENT+ }` head into [`Head::OneOf`]. `open_span` is the span
    /// of the `{`. An empty `{}` or a non-`IDENT` token inside is an error.
    fn oneof_head(&mut self, open_span: PatSpan) -> Result<Head, ParseError> {
        let mut tags: Vec<NodeKindTag> = Vec::new();
        loop {
            let Some(spanned) = self.next() else {
                return Err(ParseError::new("unclosed `{`: expected `}`", open_span));
            };
            match &spanned.tok {
                Token::RBrace => {
                    if tags.is_empty() {
                        return Err(ParseError::new(
                            "`{...}` head needs at least one node type",
                            PatSpan {
                                start: open_span.start,
                                end: spanned.span.end,
                            },
                        ));
                    }
                    return Ok(Head::OneOf(tags));
                }
                Token::Ident(name) => tags.push(resolve_kind(name, spanned.span)?),
                _ => {
                    return Err(ParseError::new(
                        "a `{...}` head may only contain node types",
                        spanned.span,
                    ));
                }
            }
        }
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

/// Resolve a node-type `name` to its [`NodeKindTag`], or a span-carrying
/// error naming the unknown type. Shared by the `Head::Exact`/`Head::OneOf`
/// paths. Unlike [`Parser::ident_pat`], `true`/`false`/`nil` are *not* special
/// here: a node head must name a real node type.
fn resolve_kind(name: &str, span: PatSpan) -> Result<NodeKindTag, ParseError> {
    murphy_ast::tag_from_pattern_name(name)
        .ok_or_else(|| ParseError::new(format!("unknown node type `{name}`"), span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Head, Lit, PatKind};

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

    // --- Task 6: node match `(...)` --------------------------------------

    #[test]
    fn parses_node_with_children() {
        let p = parse("(send nil :puts)").expect("ok");
        match p.root.kind {
            PatKind::Node { head, children } => {
                assert_eq!(head, Head::Exact(murphy_ast::NodeKindTag(17)));
                assert_eq!(children.len(), 2);
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn parses_any_head() {
        let p = parse("(_ _)").expect("ok");
        assert!(matches!(
            p.root.kind,
            PatKind::Node {
                head: Head::Any,
                ..
            }
        ));
    }

    #[test]
    fn parses_oneof_head() {
        let p = parse("({send csend} _)").expect("ok");
        match p.root.kind {
            PatKind::Node {
                head: Head::OneOf(tags),
                ..
            } => {
                assert_eq!(
                    tags,
                    vec![murphy_ast::NodeKindTag(17), murphy_ast::NodeKindTag(18)]
                );
            }
            other => panic!("expected OneOf head, got {other:?}"),
        }
    }

    #[test]
    fn parses_rest_in_child_list() {
        let p = parse("(array ... _)").expect("ok");
        match p.root.kind {
            PatKind::Node { children, .. } => {
                assert_eq!(children[0].kind, PatKind::Rest);
                assert_eq!(children[1].kind, PatKind::Wildcard);
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn rejects_multiple_rest() {
        let e = parse("(array ... ...)").expect_err("two rests");
        assert!(e.message.to_lowercase().contains("..."));
    }

    #[test]
    fn rejects_unbalanced_paren() {
        let e = parse("(send").expect_err("unclosed");
        // An unclosed `(` must not surface as the generic empty-pattern error.
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_paren_after_open() {
        // `(` alone runs out of input while parsing the head.
        let e = parse("(").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_oneof_head() {
        // `({send` runs out of input inside the `{...}` head scan.
        let e = parse("({send").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_empty_node() {
        // `()` has no head.
        assert!(parse("()").is_err());
    }

    // --- additional Task 6 coverage --------------------------------------

    #[test]
    fn parses_nested_node() {
        // `(send (send nil :a) :b)` — the first child is itself a node match.
        let p = parse("(send (send nil :a) :b)").expect("ok");
        match p.root.kind {
            PatKind::Node { head, children } => {
                assert_eq!(head, Head::Exact(murphy_ast::NodeKindTag(17)));
                assert_eq!(children.len(), 2);
                assert!(matches!(children[0].kind, PatKind::Node { .. }));
                assert_eq!(children[1].kind, PatKind::Lit(Lit::Sym("b".into())));
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn parses_rest_last_in_child_list() {
        // `Rest` may also be the final child.
        let p = parse("(array _ ...)").expect("ok");
        match p.root.kind {
            PatKind::Node { children, .. } => {
                assert_eq!(children[0].kind, PatKind::Wildcard);
                assert_eq!(children[1].kind, PatKind::Rest);
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn parses_node_with_no_children() {
        // A head with no children is still a valid node match.
        let p = parse("(send)").expect("ok");
        match p.root.kind {
            PatKind::Node { head, children } => {
                assert_eq!(head, Head::Exact(murphy_ast::NodeKindTag(17)));
                assert!(children.is_empty());
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_head_kind() {
        let e = parse("(sned _)").expect_err("unknown head kind");
        assert!(
            e.message.contains("sned") && e.message.contains("unknown node type"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unknown_oneof_kind() {
        let e = parse("({send sned} _)").expect_err("unknown OneOf kind");
        assert!(
            e.message.contains("sned") && e.message.contains("unknown node type"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_empty_oneof_head() {
        // `{}` as a head has no alternatives.
        assert!(parse("({} _)").is_err());
    }

    #[test]
    fn rejects_non_ident_in_oneof_head() {
        // A `{...}` head may only contain node-type names, not literals or `_`.
        // Exercises the "may only contain node types" arm of `oneof_head`.
        let e = parse("({send :sym} _)").expect_err("symbol in OneOf head");
        assert!(
            e.message.contains("may only contain node types"),
            "message was: {}",
            e.message
        );
        let e = parse("({send _} _)").expect_err("wildcard in OneOf head");
        assert!(
            e.message.contains("may only contain node types"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn node_span_covers_open_through_close() {
        // `(send)` — `(` at 0, `)` at 5; the Node span must cover 0..6.
        let p = parse("(send)").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 6));
    }

    #[test]
    fn rejects_rparen_as_head() {
        // `()` reports a missing-head error, not an empty-pattern one.
        let e = parse("()").expect_err("no head");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }
}
