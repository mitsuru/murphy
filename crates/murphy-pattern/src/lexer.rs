//! Hand-written scanner: pattern source string -> `Vec<Spanned>`.
//!
//! Spans are byte offsets into the source string; `span.end` is exclusive.
//! The parser (`parser.rs`) consumes the token stream produced here.

// The parser (murphy-9cr.17 Task 5) is the in-crate consumer of these items;
// until it lands, the lib target sees them as unused. Matches the staging
// pattern in `murphy-core::mruby::state`.
#![allow(dead_code)]

use crate::{ParseError, PatSpan};

/// A lexical token of the S-expression pattern grammar (v1 subset).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `_` standing alone.
    Underscore,
    /// `...`
    Ellipsis,
    /// `!`
    Bang,
    /// `$`
    Dollar,
    /// `^`
    Caret,
    /// `` ` ``
    Backtick,
    /// `nil?` — the built-in nil test.
    NilQuestion,
    /// `#name` — predicate call; payload is the name without the leading `#`.
    Predicate(String),
    /// `[a-z_][a-z0-9_]*` — a bare identifier (`true`/`false`/`nil` included).
    Ident(String),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// `"..."` — a string literal; payload is the unescaped contents.
    Str(String),
    /// `:name` — a symbol literal; payload is the name without the leading `:`.
    Sym(String),
}

/// A [`Token`] paired with its byte-offset span in the source string.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Spanned {
    pub tok: Token,
    pub span: PatSpan,
}

/// Scan `src` into a token stream, or return the first lexing error.
pub(crate) fn tokenize(src: &str) -> Result<Vec<Spanned>, ParseError> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn run(mut self) -> Result<Vec<Spanned>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(&b) = self.peek() else { break };
            let start = self.pos;
            let tok = self.scan_token(b)?;
            out.push(Spanned {
                tok,
                span: PatSpan::new(start, self.pos),
            });
        }
        Ok(out)
    }

    /// Dispatch on the first byte of a token (`b` == `self.src[self.pos]`).
    fn scan_token(&mut self, b: u8) -> Result<Token, ParseError> {
        match b {
            b'(' => self.single(Token::LParen),
            b')' => self.single(Token::RParen),
            b'{' => self.single(Token::LBrace),
            b'}' => self.single(Token::RBrace),
            b'!' => self.single(Token::Bang),
            b'$' => self.single(Token::Dollar),
            b'^' => self.single(Token::Caret),
            b'`' => self.single(Token::Backtick),
            b'.' => self.scan_ellipsis(),
            b'#' => self.scan_predicate(),
            b':' => self.scan_symbol(),
            b'"' => self.scan_string(),
            b'-' => self.scan_number(),
            b'0'..=b'9' => self.scan_number(),
            b'a'..=b'z' | b'_' => self.scan_ident(),
            b'%' | b'[' | b'<' => Err(self.err_at(
                self.pos,
                self.pos + 1,
                format!("`{}` is not supported in v1", b as char),
            )),
            _ => Err(self.err_at(
                self.pos,
                self.pos + 1,
                format!("unexpected character `{}`", b as char),
            )),
        }
    }

    /// Consume a one-byte token and return it.
    fn single(&mut self, tok: Token) -> Result<Token, ParseError> {
        self.pos += 1;
        Ok(tok)
    }

    /// Scan `...`; a bare `.` or `..` is a lex error.
    fn scan_ellipsis(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        if self.src[start..].starts_with(b"...") {
            self.pos += 3;
            Ok(Token::Ellipsis)
        } else {
            // Take the run of dots so the span covers what was actually seen.
            let mut end = start;
            while self.src.get(end) == Some(&b'.') {
                end += 1;
            }
            self.pos = end;
            Err(self.err_at(start, end, "expected `...`"))
        }
    }

    /// Scan `#name` where `name` is `[a-z_][a-z0-9_]*[?!]?`.
    fn scan_predicate(&mut self) -> Result<Token, ParseError> {
        let hash = self.pos;
        self.pos += 1; // consume `#`
        let name = self
            .take_method_name()
            .ok_or_else(|| self.err_at(hash, hash + 1, "expected a predicate name after `#`"))?;
        Ok(Token::Predicate(name))
    }

    /// Scan `:name` where `name` is `[a-z_][a-z0-9_]*[?!]?`.
    fn scan_symbol(&mut self) -> Result<Token, ParseError> {
        let colon = self.pos;
        self.pos += 1; // consume `:`
        let name = self
            .take_method_name()
            .ok_or_else(|| self.err_at(colon, colon + 1, "expected a symbol name after `:`"))?;
        Ok(Token::Sym(name))
    }

    /// Scan a `"..."` string with `\"` and `\\` escapes only.
    fn scan_string(&mut self) -> Result<Token, ParseError> {
        let open = self.pos;
        self.pos += 1; // consume opening `"`
        let mut value = String::new();
        loop {
            let Some(&b) = self.peek() else {
                return Err(self.err_at(open, self.pos, "unterminated string literal"));
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(Token::Str(value));
                }
                b'\\' => {
                    let esc_start = self.pos;
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => {
                            value.push('"');
                            self.pos += 1;
                        }
                        Some(b'\\') => {
                            value.push('\\');
                            self.pos += 1;
                        }
                        Some(&other) => {
                            return Err(self.err_at(
                                esc_start,
                                self.pos + 1,
                                format!(
                                    "unsupported escape `\\{}` in string literal",
                                    other as char
                                ),
                            ));
                        }
                        None => {
                            return Err(self.err_at(open, self.pos, "unterminated string literal"));
                        }
                    }
                }
                _ => {
                    // Append the raw byte run up to the next `"` or `\`.
                    let chunk_start = self.pos;
                    while let Some(&c) = self.peek() {
                        if c == b'"' || c == b'\\' {
                            break;
                        }
                        self.pos += 1;
                    }
                    // The scanned chunk is a contiguous slice of valid UTF-8.
                    value.push_str(
                        std::str::from_utf8(&self.src[chunk_start..self.pos])
                            .expect("source is valid UTF-8"),
                    );
                }
            }
        }
    }

    /// Scan an integer or float literal, optionally `-`-prefixed.
    ///
    /// Only reached when the first byte is a digit, or `-` followed by a digit
    /// is expected; a bare `-` (or `-` before a non-digit) is a lex error.
    fn scan_number(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        if self.peek() == Some(&b'-') {
            // A `-` only starts a number when a digit follows.
            if !matches!(self.src.get(self.pos + 1), Some(b'0'..=b'9')) {
                self.pos += 1;
                return Err(self.err_at(start, self.pos, "stray `-`: expected a digit after `-`"));
            }
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let is_float =
            self.peek() == Some(&b'.') && matches!(self.src.get(self.pos + 1), Some(b'0'..=b'9'));
        if is_float {
            self.pos += 1; // consume `.`
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            let text = self.slice_str(start, self.pos);
            let value = text
                .parse::<f64>()
                .map_err(|_| self.err_at(start, self.pos, "invalid float literal"))?;
            Ok(Token::Float(value))
        } else {
            let text = self.slice_str(start, self.pos);
            let value = text
                .parse::<i64>()
                .map_err(|_| self.err_at(start, self.pos, "invalid integer literal"))?;
            Ok(Token::Int(value))
        }
    }

    /// Scan a `[a-z_][a-z0-9_]*` identifier with an optional trailing `?`/`!`.
    ///
    /// `_` alone -> `Underscore`; `nil?` -> `NilQuestion`; any other bare
    /// identifier ending in `?`/`!` is a lex error.
    fn scan_ident(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        // First byte is already known to be `[a-z_]`.
        self.pos += 1;
        while matches!(self.peek(), Some(b'a'..=b'z' | b'0'..=b'9' | b'_')) {
            self.pos += 1;
        }
        let has_suffix = matches!(self.peek(), Some(b'?' | b'!'));
        if has_suffix {
            self.pos += 1;
        }
        let text = self.slice_str(start, self.pos);
        if has_suffix {
            if text == "nil?" {
                return Ok(Token::NilQuestion);
            }
            return Err(self.err_at(start, self.pos, "expected '#' before a predicate name"));
        }
        if text == "_" {
            return Ok(Token::Underscore);
        }
        Ok(Token::Ident(text.to_string()))
    }

    /// Read a Ruby-method-ish name `[a-z_][a-z0-9_]*[?!]?` at the cursor.
    /// Returns `None` (leaving the cursor unmoved) when no name is present.
    fn take_method_name(&mut self) -> Option<String> {
        match self.peek() {
            Some(b'a'..=b'z' | b'_') => {}
            _ => return None,
        }
        let start = self.pos;
        self.pos += 1;
        while matches!(self.peek(), Some(b'a'..=b'z' | b'0'..=b'9' | b'_')) {
            self.pos += 1;
        }
        if matches!(self.peek(), Some(b'?' | b'!')) {
            self.pos += 1;
        }
        Some(self.slice_str(start, self.pos).to_string())
    }

    /// Advance past any ASCII whitespace.
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// The byte at the cursor, or `None` at end of input.
    fn peek(&self) -> Option<&u8> {
        self.src.get(self.pos)
    }

    /// Borrow `src[start..end]` as `&str` (the source is always valid UTF-8
    /// and these ranges fall on ASCII-only token boundaries).
    fn slice_str(&self, start: usize, end: usize) -> &str {
        std::str::from_utf8(&self.src[start..end]).expect("token slice is valid UTF-8")
    }

    /// Build a `ParseError` spanning `start..end`.
    fn err_at(&self, start: usize, end: usize, message: impl Into<String>) -> ParseError {
        ParseError::new(message, PatSpan::new(start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Token> {
        tokenize(src)
            .expect("lex ok")
            .into_iter()
            .map(|s| s.tok)
            .collect()
    }

    #[test]
    fn lexes_node_match() {
        assert_eq!(
            toks("(send nil? :puts $...)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::NilQuestion,
                Token::Sym("puts".into()),
                Token::Dollar,
                Token::Ellipsis,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn lexes_literals_and_sigils() {
        assert_eq!(
            toks("{ !_ ^x `y #pred 42 -1 1.5 \"s\" true }"),
            vec![
                Token::LBrace,
                Token::Bang,
                Token::Underscore,
                Token::Caret,
                Token::Ident("x".into()),
                Token::Backtick,
                Token::Ident("y".into()),
                Token::Predicate("pred".into()),
                Token::Int(42),
                Token::Int(-1),
                Token::Float(1.5),
                Token::Str("s".into()),
                Token::Ident("true".into()),
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn span_points_at_token() {
        let t = tokenize("(send)").expect("ok");
        // `send` occupies bytes 1..5.
        assert_eq!(t[1].tok, Token::Ident("send".into()));
        assert_eq!((t[1].span.start, t[1].span.end), (1, 5));
    }

    #[test]
    fn lex_error_on_unsupported_sigil() {
        let e = tokenize("(send %1)").expect_err("must reject %");
        assert!(e.message.contains('%'));
        // span points at the `%`
        assert_eq!(e.span.start, 6);
    }

    #[test]
    fn lex_error_on_bare_predicate_name() {
        assert!(tokenize("even?").is_err());
    }

    // --- additional coverage ----------------------------------------------

    #[test]
    fn negative_float_and_int() {
        assert_eq!(toks("-0.5"), vec![Token::Float(-0.5)]);
        assert_eq!(toks("-123"), vec![Token::Int(-123)]);
        assert_eq!(toks("0.25 3"), vec![Token::Float(0.25), Token::Int(3)]);
    }

    #[test]
    fn string_escapes() {
        assert_eq!(toks(r#""a\"b\\c""#), vec![Token::Str("a\"b\\c".into())]);
    }

    #[test]
    fn unsupported_string_escape_is_error() {
        let e = tokenize(r#""bad\n""#).expect_err("must reject \\n");
        assert!(e.message.contains("escape"));
    }

    #[test]
    fn unterminated_string_is_error() {
        let e = tokenize("\"oops").expect_err("must reject unterminated");
        // span starts at the opening quote
        assert_eq!(e.span.start, 0);
    }

    #[test]
    fn nil_versus_nil_question() {
        assert_eq!(toks("nil"), vec![Token::Ident("nil".into())]);
        assert_eq!(toks("nil?"), vec![Token::NilQuestion]);
    }

    #[test]
    fn underscore_alone_versus_in_ident() {
        assert_eq!(toks("_"), vec![Token::Underscore]);
        assert_eq!(toks("block_pass"), vec![Token::Ident("block_pass".into())]);
        assert_eq!(toks("_x"), vec![Token::Ident("_x".into())]);
    }

    #[test]
    fn predicate_with_question_suffix() {
        assert_eq!(toks("#odd?"), vec![Token::Predicate("odd?".into())]);
        assert_eq!(toks("#has!"), vec![Token::Predicate("has!".into())]);
    }

    #[test]
    fn bang_identifier_is_error() {
        // a bare identifier ending in `!` (not `nil?`) is rejected
        assert!(tokenize("save!").is_err());
    }

    #[test]
    fn bare_dash_is_error() {
        let e = tokenize("(- 1)").expect_err("must reject bare -");
        assert_eq!(e.span.start, 1);
    }

    #[test]
    fn spans_track_byte_offsets_across_tokens() {
        // "(a -1 :s)" — byte offsets:
        //  ( 0 | a 1 | sp 2 | - 3 .. 5 | sp 5 | : 6 .. 8 | ) 8
        let t = tokenize("(a -1 :s)").expect("ok");
        let spans: Vec<(u32, u32)> = t.iter().map(|s| (s.span.start, s.span.end)).collect();
        assert_eq!(spans, vec![(0, 1), (1, 2), (3, 5), (6, 8), (8, 9)]);
    }

    #[test]
    fn two_dots_is_error() {
        let e = tokenize("(..)").expect_err("must reject ..");
        assert!(e.message.contains("..."));
        assert_eq!((e.span.start, e.span.end), (1, 3));
    }

    #[test]
    fn float_followed_by_dot_is_separate_error() {
        // `1.` — the trailing `.` is not part of a float and is not `...`.
        let e = tokenize("1.").expect_err("must reject trailing dot");
        assert!(e.message.contains("..."));
    }
}
