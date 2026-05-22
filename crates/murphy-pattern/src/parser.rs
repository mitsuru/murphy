//! Recursive-descent parser: token stream -> `PatternAst`.
//!
//! Task 5 (murphy-9cr.17) implements the atom/prefix skeleton: `_`,
//! literals, `nil?`, bare kind names, `#predicate`, and the `!`/`^`/backtick
//! prefixes. Task 6 adds node match `(head child*)` with `Exact`/`Any`/`OneOf`
//! heads. Task 7 adds union `{a b ...}`. Task 8 implements `$` captures —
//! named (`$ident`), anonymous (`$<pattern>`), and seq (`$...`) forms.

use crate::lexer::{Spanned, Token, tokenize};
use crate::{CaptureKind, Head, Lit, ParseError, Pat, PatKind, PatSpan, PatternAst};
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
    // The root is not a node child, so a rest-like root is correctly rejected.
    validate_rest_placement(&root, false)?;
    Ok(PatternAst {
        root,
        captures: parser.captures,
    })
}

/// Whether `pat` is a "rest-like" element — one that matches zero-or-more
/// sibling nodes. There are two forms: a bare `...` ([`PatKind::Rest`]) and an
/// anonymous seq capture `$...` (a [`PatKind::Capture`] whose `body` is
/// [`PatKind::Rest`]).
fn is_rest_like(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Rest => true,
        PatKind::Capture { body, .. } => matches!(body.kind, PatKind::Rest),
        _ => false,
    }
}

/// Post-parse walk enforcing the v1 grammar rule for rest-like elements
/// (`...` and `$...`): each is valid *only* as a direct child of a node match
/// `(...)`, and *at most one* per node child list.
///
/// This is the single source of truth for both invariants — `node_match`
/// parses `...` into [`PatKind::Rest`] but does not itself reject duplicates.
///
/// `is_node_child` is `true` only when `pat` is being visited as a direct
/// child of a [`PatKind::Node`]. The root, union alternatives, and the bodies
/// of `!`/`^`/`` ` ``/`$` are all *not* node children.
fn validate_rest_placement(pat: &Pat, is_node_child: bool) -> Result<(), ParseError> {
    if is_rest_like(pat) && !is_node_child {
        return Err(ParseError::new(
            "`...` / `$...` is only valid as a direct child of a node match",
            pat.span,
        ));
    }
    match &pat.kind {
        PatKind::Node { children, .. } => {
            // At most one rest-like element per child list. Point the error at
            // the SECOND such child.
            if let Some(second) = children.iter().filter(|c| is_rest_like(c)).nth(1) {
                return Err(ParseError::new(
                    "at most one `...` / `$...` per node child list",
                    second.span,
                ));
            }
            for child in children {
                validate_rest_placement(child, true)?;
            }
        }
        PatKind::Union(alts) => {
            for alt in alts {
                validate_rest_placement(alt, false)?;
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            validate_rest_placement(b, false)?;
        }
        PatKind::Capture { body, .. } => {
            // A `$...` seq capture is one rest-like unit: the inner `Rest` is
            // not an independently-validated element, so do not recurse into
            // it. Any other capture (`$_`, `$(...)`, `$name`, …) recurses.
            if !is_rest_like(pat) {
                validate_rest_placement(body, false)?;
            }
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate(_)
        | PatKind::Kind(_) => {}
    }
    Ok(())
}

/// A cursor over a token slice for recursive-descent parsing.
struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
    /// One entry per `$` capture, indexed by slot. A slot is reserved (with a
    /// placeholder `CaptureKind::Node`) the instant its `$` token is consumed,
    /// so slots are assigned in source order — left-to-right, outer-before-inner.
    captures: Vec<CaptureKind>,
    /// Names of `$ident` captures seen so far, used to reject duplicates.
    capture_names: Vec<String>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned]) -> Parser<'a> {
        Parser {
            tokens,
            pos: 0,
            captures: Vec::new(),
            capture_names: Vec::new(),
        }
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

    /// `prefixed := '!' prefixed | '^' prefixed | '`' prefixed
    /// | '$' capture-tail | primary`.
    ///
    /// `$` is a prefix dispatched here (see [`Parser::capture`]), not in
    /// [`Parser::primary`].
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
            Token::Dollar => return self.capture(prefix_span),
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

    /// Parse a `$` capture tail. `dollar_span` is the span of the `$` token,
    /// still at the cursor.
    ///
    /// The capture's `slot` is reserved — appended to `self.captures` with a
    /// placeholder [`CaptureKind::Node`] — the instant the `$` is consumed,
    /// *before* the body is parsed, so slots follow source order
    /// (left-to-right, outer-before-inner) even for nested captures.
    ///
    /// - `$ident` → a named capture; `name` is `Some`, `body` is an implicit
    ///   [`PatKind::Wildcard`] spanning the `$`. Duplicate names are rejected.
    /// - `$...` → an anonymous seq capture; `body` is [`PatKind::Rest`] and the
    ///   slot's kind is upgraded to [`CaptureKind::Seq`].
    /// - `$<anything else>` → an anonymous capture whose `body` is a full
    ///   [`prefixed`](Self::prefixed) pattern parsed recursively.
    /// - `$` at end of input → a span-carrying error.
    fn capture(&mut self, dollar_span: PatSpan) -> Result<Pat, ParseError> {
        self.pos += 1; // consume `$`
        // Reserve the slot now — before the body — so source order holds.
        let slot = u16::try_from(self.captures.len())
            .map_err(|_| ParseError::new("too many captures in one pattern", dollar_span))?;
        self.captures.push(CaptureKind::Node);

        let Some(next) = self.peek() else {
            return Err(ParseError::new(
                "dangling `$`: nothing to capture",
                dollar_span,
            ));
        };

        // The Capture node spans `$` through the end of whatever names it.
        // For most forms that is `body.span.end`, but for `$ident` the body is
        // a synthetic Wildcard spanning only the `$`, so the identifier token's
        // span end is tracked separately and used for the outer Capture span.
        let (name, body, capture_end) = match &next.tok {
            Token::Ident(ident) => {
                let name = ident.clone();
                let ident_span = next.span;
                self.pos += 1; // consume the ident
                if self.capture_names.contains(&name) {
                    return Err(ParseError::new(
                        format!("duplicate capture name `{name}`"),
                        ident_span,
                    ));
                }
                self.capture_names.push(name.clone());
                // The implicit Wildcard body is synthetic; per spec it spans
                // the `$` token, not the identifier.
                let body = Pat {
                    kind: PatKind::Wildcard,
                    span: dollar_span,
                };
                (Some(name), body, ident_span.end)
            }
            Token::Ellipsis => {
                let ell_span = next.span;
                self.pos += 1; // consume `...`
                self.captures[slot as usize] = CaptureKind::Seq;
                let body = Pat {
                    kind: PatKind::Rest,
                    span: ell_span,
                };
                (None, body, ell_span.end)
            }
            _ => {
                let body = self.prefixed()?;
                let end = body.span.end;
                (None, body, end)
            }
        };

        let span = PatSpan {
            start: dollar_span.start,
            end: capture_end,
        };
        Ok(Pat {
            kind: PatKind::Capture {
                slot,
                name,
                body: Box::new(body),
            },
            span,
        })
    }

    /// `primary := '_' | 'nil?' | literal | '#' name | IDENT | node-match
    /// | union`.
    ///
    /// `(` parses a node match (see [`node_match`]); `{` parses a union (see
    /// [`union`]). A top-level `...` is invalid and produces a descriptive
    /// error here. Prefix sigils (`!`/`^`/`` ` ``/`$`) never reach `primary`:
    /// [`prefixed`](Self::prefixed) consumes them first.
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
            Token::LBrace => return self.union(span),
            Token::Ellipsis => {
                return Err(ParseError::new(
                    "`...` is only valid inside a node child list",
                    span,
                ));
            }
            Token::RParen => return Err(ParseError::new("unexpected `)`", span)),
            Token::RBrace => return Err(ParseError::new("unexpected `}`", span)),
            Token::Bang | Token::Caret | Token::Backtick | Token::Dollar => {
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
    /// with [`prefixed`], except a `...` in a child slot becomes [`PatKind::Rest`].
    /// `node_match` only *builds* the child list — the "at most one rest-like
    /// element per child list" and placement rules are enforced afterward by
    /// the [`validate_rest_placement`] walk, the single source of truth.
    fn node_match(&mut self, open_span: PatSpan) -> Result<Pat, ParseError> {
        let head = self.node_head(open_span)?;
        let mut children: Vec<Pat> = Vec::new();
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
                    children.push(Pat {
                        kind: PatKind::Rest,
                        span: ell_span,
                    });
                }
                _ => children.push(self.prefixed()?),
            }
        }
    }

    /// `union := '{' prefixed+ '}'`.
    ///
    /// `open_span` is the span of the already-consumed `{`. Each alternative is
    /// a full [`prefixed`] pattern — arbitrary patterns are allowed, unlike the
    /// node-type-only `{...}` head handled by [`oneof_head`]. The resulting
    /// `Pat`'s span covers `{` through the closing `}`. An empty `{}` or a
    /// stream that ends before `}` is a span-carrying error.
    fn union(&mut self, open_span: PatSpan) -> Result<Pat, ParseError> {
        let mut alts: Vec<Pat> = Vec::new();
        loop {
            // Peek (not `next`) so we can dispatch on the token — a closing `}`
            // or an alternative — before deciding whether to consume it.
            let Some(tok) = self.peek() else {
                // Ran out of input before the closing `}`.
                return Err(ParseError::new("unclosed `{`: expected `}`", open_span));
            };
            match tok.tok {
                Token::RBrace => {
                    let close_span = tok.span;
                    self.pos += 1; // consume `}`
                    let span = PatSpan {
                        start: open_span.start,
                        end: close_span.end,
                    };
                    if alts.is_empty() {
                        return Err(ParseError::new(
                            "empty union `{}` needs at least one alternative",
                            span,
                        ));
                    }
                    return Ok(Pat {
                        kind: PatKind::Union(alts),
                        span,
                    });
                }
                _ => alts.push(self.prefixed()?),
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
    use crate::{CaptureKind, Head, Lit, PatKind};

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
    fn non_capture_patterns_have_zero_captures() {
        // A pattern containing no `$` reserves no capture slots, so
        // `n_captures()` must report zero. `parse` threads the real capture
        // list, so this pins that a capture-free pattern stays capture-free.
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

    // --- Task 7: union `{}` ----------------------------------------------

    #[test]
    fn parses_union() {
        let p = parse("{send csend}").expect("ok");
        match p.root.kind {
            PatKind::Union(alts) => assert_eq!(alts.len(), 2),
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parses_union_of_subpatterns() {
        let p = parse("{(send _ :a) (send _ :b)}").expect("ok");
        assert!(matches!(p.root.kind, PatKind::Union(alts) if alts.len() == 2));
    }

    #[test]
    fn rejects_empty_union() {
        let e = parse("{}").expect_err("empty union");
        assert!(e.message.to_lowercase().contains("union"));
    }

    // --- additional Task 7 coverage --------------------------------------

    #[test]
    fn parses_single_alternative_union() {
        // A `{...}` with one alternative is still a union.
        let p = parse("{send}").expect("ok");
        match p.root.kind {
            PatKind::Union(alts) => assert_eq!(alts.len(), 1),
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_union() {
        // `{{send csend} array}` — the first alternative is itself a union.
        let p = parse("{{send csend} array}").expect("ok");
        match p.root.kind {
            PatKind::Union(alts) => {
                assert_eq!(alts.len(), 2);
                assert!(matches!(alts[0].kind, PatKind::Union(ref inner) if inner.len() == 2));
                assert!(matches!(alts[1].kind, PatKind::Kind(_)));
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parses_union_with_prefixed_alternative() {
        // Alternatives are full `prefixed` patterns, so `!_` is allowed.
        let p = parse("{!send _}").expect("ok");
        match p.root.kind {
            PatKind::Union(alts) => {
                assert_eq!(alts.len(), 2);
                assert!(matches!(alts[0].kind, PatKind::Not(_)));
                assert_eq!(alts[1].kind, PatKind::Wildcard);
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unclosed_union() {
        // `{send` runs out of input before the closing `}`.
        let e = parse("{send").expect_err("unclosed");
        // An unclosed `{` must not surface as the generic empty-pattern error.
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_empty_union() {
        // `{` alone runs out of input immediately.
        let e = parse("{").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn union_span_covers_open_through_close() {
        // `{send csend}` — `{` at 0, `}` at 11; the Union span must cover 0..12.
        let p = parse("{send csend}").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 12));
    }

    // --- Task 8: `$` captures --------------------------------------------

    #[test]
    fn parses_anonymous_capture() {
        let p = parse("(send $_ :puts)").expect("ok");
        assert_eq!(p.n_captures(), 1);
        assert_eq!(p.capture_kinds(), &[CaptureKind::Node]);
    }

    #[test]
    fn parses_seq_capture() {
        let p = parse("(send nil :puts $...)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
    }

    #[test]
    fn parses_named_capture_body_is_wildcard() {
        let p = parse("(send $receiver :puts)").expect("ok");
        assert_eq!(p.n_captures(), 1);
        match &p.root.kind {
            PatKind::Node { children, .. } => match &children[0].kind {
                PatKind::Capture { slot, name, body } => {
                    assert_eq!(*slot, 0);
                    assert_eq!(name.as_deref(), Some("receiver"));
                    assert_eq!(body.kind, PatKind::Wildcard);
                }
                other => panic!("expected Capture, got {other:?}"),
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn capture_of_subpattern_uses_parens() {
        // `:foo` (not `:Foo`): the lexer's symbol grammar is lowercase-only,
        // a pre-existing constraint outside Task 8's parser-only scope. The
        // test asserts only that the capture body is a `Node`, so the symbol
        // payload is incidental.
        let p = parse("$(const _ :foo)").expect("ok");
        match p.root.kind {
            PatKind::Capture { slot, name, body } => {
                assert_eq!(slot, 0);
                assert!(name.is_none());
                assert!(matches!(body.kind, PatKind::Node { .. }));
            }
            other => panic!("expected Capture, got {other:?}"),
        }
    }

    #[test]
    fn capture_slots_are_left_to_right() {
        let p = parse("(send $_ $...)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::Node, CaptureKind::Seq]);
    }

    #[test]
    fn nested_captures_are_source_order() {
        // outer `$(...)` = slot 0, inner `$inner` = slot 1 — source order,
        // NOT post-order. Guards the nested-capture slot-numbering bug.
        let p = parse("$(send $inner _)").expect("ok");
        assert_eq!(p.n_captures(), 2);
        assert_eq!(p.capture_kinds(), &[CaptureKind::Node, CaptureKind::Node]);
        match &p.root.kind {
            PatKind::Capture { slot, body, .. } => {
                assert_eq!(*slot, 0, "outer capture is slot 0");
                match &body.kind {
                    PatKind::Node { children, .. } => match &children[0].kind {
                        PatKind::Capture { slot, name, .. } => {
                            assert_eq!(*slot, 1, "inner capture is slot 1");
                            assert_eq!(name.as_deref(), Some("inner"));
                        }
                        other => panic!("expected inner Capture, got {other:?}"),
                    },
                    _ => unreachable!(),
                }
            }
            other => panic!("expected outer Capture, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_capture_name() {
        let e = parse("(send $x $x)").expect_err("dup name");
        assert!(e.message.contains('x'));
    }

    // --- additional Task 8 coverage --------------------------------------

    #[test]
    fn dollar_at_end_of_input_is_error() {
        // A `$` with nothing to capture.
        let e = parse("$").expect_err("dangling `$`");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
        // The error span points at the `$` token (byte 0..1).
        assert_eq!((e.span.start, e.span.end), (0, 1));
    }

    #[test]
    fn double_capture_nests() {
        // `$$_` — the outer `$` is anonymous and its body is recursively
        // parsed via `prefixed`, which sees the inner `$_`. Two captures,
        // outer is slot 0, inner is slot 1.
        let p = parse("$$_").expect("ok");
        assert_eq!(p.n_captures(), 2);
        assert_eq!(p.capture_kinds(), &[CaptureKind::Node, CaptureKind::Node]);
        match p.root.kind {
            PatKind::Capture { slot, name, body } => {
                assert_eq!(slot, 0);
                assert!(name.is_none());
                match body.kind {
                    PatKind::Capture {
                        slot: inner_slot,
                        body: inner_body,
                        ..
                    } => {
                        assert_eq!(inner_slot, 1);
                        assert_eq!(inner_body.kind, PatKind::Wildcard);
                    }
                    other => panic!("expected inner Capture, got {other:?}"),
                }
            }
            other => panic!("expected outer Capture, got {other:?}"),
        }
    }

    #[test]
    fn capture_span_covers_dollar_through_body() {
        // `$_` — `$` at 0, `_` at 1; the Capture span must cover 0..2.
        let p = parse("$_").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 2));
    }

    #[test]
    fn named_capture_span_covers_dollar_through_ident() {
        // `$x` — `$` at 0, `x` at 1; the Capture span must cover `$` through
        // the identifier (0..2). The implicit Wildcard body is synthetic and,
        // per spec, spans only the `$` token (0..1).
        let p = parse("$x").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 2));
        match p.root.kind {
            PatKind::Capture { body, .. } => {
                assert_eq!((body.span.start, body.span.end), (0, 1));
            }
            other => panic!("expected Capture, got {other:?}"),
        }
    }

    #[test]
    fn capture_of_prefixed_pattern() {
        // `$!_` — the outer `$`'s body is recursively parsed via `prefixed`,
        // so a prefix like `!` is allowed in the body.
        let p = parse("$!_").expect("ok");
        match p.root.kind {
            PatKind::Capture { slot, name, body } => {
                assert_eq!(slot, 0);
                assert!(name.is_none());
                assert!(matches!(body.kind, PatKind::Not(_)));
            }
            other => panic!("expected Capture, got {other:?}"),
        }
    }

    #[test]
    fn distinct_capture_names_are_allowed() {
        // Sibling captures with different names are fine.
        let p = parse("(send $recv $arg)").expect("ok");
        assert_eq!(p.n_captures(), 2);
    }

    #[test]
    fn duplicate_name_message_names_the_duplicate() {
        let e = parse("(send $dup $dup)").expect_err("dup name");
        assert!(
            e.message.contains("dup"),
            "message should name the duplicate, was: {}",
            e.message
        );
    }

    // --- rest / seq-capture placement validation -------------------------

    #[test]
    fn rejects_two_seq_captures_in_child_list() {
        assert!(parse("(send $... $...)").is_err());
    }

    #[test]
    fn rejects_bare_rest_and_seq_capture_mixed() {
        assert!(parse("(send ... $...)").is_err());
        assert!(parse("(send $... ...)").is_err());
    }

    #[test]
    fn rejects_seq_capture_at_top_level() {
        assert!(parse("$...").is_err());
    }

    #[test]
    fn rejects_seq_capture_in_union() {
        assert!(parse("{$... _}").is_err());
    }

    #[test]
    fn allows_single_seq_capture_in_child_list() {
        assert!(parse("(send $receiver $...)").is_ok());
        assert!(parse("(array ... _)").is_ok());
        assert!(parse("(array _ $...)").is_ok());
    }
}
