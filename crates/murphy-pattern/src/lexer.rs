//! Hand-written scanner: pattern source string -> `Vec<Spanned>`.
//!
//! Spans are byte offsets into the source string; `span.end` is exclusive.
//! The parser (`parser.rs`) consumes the token stream produced here.

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
    /// `_name` — a unification atom; payload is the name **without** the
    /// leading `_` (e.g. `_x` → `Unify("x")`). Only matches `_` + one or
    /// more lowercase-letter-starting `[a-z][a-zA-Z0-9_]*` names. A bare `_`
    /// stays [`Token::Underscore`]; `_X` (uppercase after `_`) is left as an
    /// `Ident("_X")` (tPARAM_CONST is uppercase-start only, not `_`-start).
    ///
    /// D4 (murphy-nnr8): NodeId-equality unification.
    Unify(String),
    /// `*` — postfix quantifier (`0..`).
    Star,
    /// `+` — postfix quantifier (`1..`).
    Plus,
    /// `?` — postfix quantifier (`0..=1`).
    Question,
    /// `<` — opening angle bracket for any-order sequence `<...>`.
    LAngle,
    /// `>` — closing angle bracket for any-order sequence `<...>`.
    RAngle,
    /// `[` — opening bracket for intersection AND-pattern `[...]`.
    LBracket,
    /// `]` — closing bracket for intersection AND-pattern `[...]`.
    RBracket,
    /// `|` — pipe separator between union alternatives in `{...}`.
    Pipe,
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
            b'*' => self.single(Token::Star),
            b'+' => self.single(Token::Plus),
            b'?' => self.single(Token::Question),
            b'.' => self.scan_ellipsis(),
            b'#' => self.scan_predicate(),
            b':' => self.scan_symbol(),
            b'"' => self.scan_string(),
            b'-' => self.scan_number(),
            b'0'..=b'9' => self.scan_number(),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_ident(),
            b'<' => self.single(Token::LAngle),
            b'>' => self.single(Token::RAngle),
            b'[' => self.single(Token::LBracket),
            b']' => self.single(Token::RBracket),
            b'|' => self.single(Token::Pipe),
            b'%' => {
                // `%Foo` (uppercase-start) is tPARAM_CONST sugar — consume the
                // `%` prefix and lex the rest as a normal uppercase ident.
                // Lowercase-start `%var` (tPARAM_NAMED) is out of v1 scope.
                if matches!(self.src.get(self.pos + 1), Some(b'A'..=b'Z')) {
                    self.pos += 1; // consume `%`
                    self.scan_ident()
                } else {
                    let (ch, len) = self.char_at_cursor();
                    Err(self.err_at(
                        self.pos,
                        self.pos + len,
                        format!("`{ch}` is not supported in v1"),
                    ))
                }
            }
            _ => {
                let (ch, len) = self.char_at_cursor();
                Err(self.err_at(
                    self.pos,
                    self.pos + len,
                    format!("unexpected character `{ch}`"),
                ))
            }
        }
    }

    /// The full `char` at the cursor and its UTF-8 byte length.
    ///
    /// `scan_token` dispatches on a single byte, but a byte `>= 0x80` is the
    /// lead of a multi-byte `char`; recovering it from the source string keeps
    /// error messages correct for non-ASCII (malformed) input.
    fn char_at_cursor(&self) -> (char, usize) {
        let rest = std::str::from_utf8(&self.src[self.pos..]).expect("source is valid UTF-8");
        match rest.chars().next() {
            Some(ch) => (ch, ch.len_utf8()),
            None => ('\u{FFFD}', 1),
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

    /// Scan `#name` where `name` is `[A-Za-z_][A-Za-z0-9_]*[?!=]?`.
    fn scan_predicate(&mut self) -> Result<Token, ParseError> {
        let hash = self.pos;
        self.pos += 1; // consume `#`
        let name = self
            .take_method_name()
            .ok_or_else(|| self.err_at(hash, hash + 1, "expected a predicate name after `#`"))?;
        Ok(Token::Predicate(name))
    }

    /// Scan `:name` — one of:
    /// - a variable-style name `:@x`, `:@@x`, `:$x` (the sigil is part of the
    ///   payload so AST matchers can compare against `(ivar :@foo)` etc.),
    /// - an identifier-style name
    ///   (`[A-Za-z_][A-Za-z0-9_]*[?!=]?`, including setter names like `foo=`),
    /// - or a Ruby operator-method name (`+`, `[]`, `<=>`, ...).
    fn scan_symbol(&mut self) -> Result<Token, ParseError> {
        let colon = self.pos;
        self.pos += 1; // consume `:`
        let name = self
            .take_var_symbol_name()
            .or_else(|| self.take_method_name())
            .or_else(|| self.take_operator_name())
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

    /// Scan a `[A-Za-z_][A-Za-z0-9_]*` identifier.
    ///
    /// Uppercase-start names (e.g. `Foo`, `MyClass`) are valid here and are
    /// translated to `(const _ :Name)` at the parser level (D3, murphy-kq57).
    /// `_` alone -> `Underscore`; `nil?` -> `NilQuestion`. `!` and `?`
    /// suffixes are *not* consumed: `int?` lexes to `Ident("int")` and
    /// `Question`, and `save!` lexes to `Ident("save")` and `Bang`.
    fn scan_ident(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        // First byte is already known to be `[A-Za-z_]`.
        self.pos += 1;
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        let text = self.slice_str(start, self.pos);
        if self.peek() == Some(&b'?') && text == "nil" {
            // `nil?` is the only bare identifier whose `?` is part of the
            // token. All other idents leave the `?` to the parser.
            self.pos += 1;
            return Ok(Token::NilQuestion);
        }
        if text == "_" {
            return Ok(Token::Underscore);
        }
        // D4 (murphy-nnr8): `_name` where `name` starts with a lowercase
        // letter → `Token::Unify(name)`. The payload excludes the leading `_`.
        // `_X` (uppercase after `_`) stays `Ident("_X")` — it is not tUNIFY.
        if let Some(rest) = text.strip_prefix('_')
            && rest.starts_with(|c: char| c.is_ascii_lowercase())
        {
            return Ok(Token::Unify(rest.to_string()));
        }
        Ok(Token::Ident(text.to_string()))
    }

    /// Read a Ruby-method-ish name `[A-Za-z_][A-Za-z0-9_]*[?!=]?` at the cursor.
    /// Returns `None` (leaving the cursor unmoved) when no name is present.
    ///
    /// The optional trailing suffix is one of `?` (predicate), `!` (bang), or
    /// `=` (setter method name, e.g. `foo=`).
    fn take_method_name(&mut self) -> Option<String> {
        match self.peek() {
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => {}
            _ => return None,
        }
        let start = self.pos;
        self.pos += 1;
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        if matches!(self.peek(), Some(b'?' | b'!' | b'=')) {
            self.pos += 1;
        }
        Some(self.slice_str(start, self.pos).to_string())
    }

    /// Ruby operator-method names, ordered longest-first so a shorter operator
    /// never shadows a longer one sharing its prefix (`<` vs `<=` vs `<=>`).
    const OPERATOR_NAMES: &'static [&'static str] = &[
        // 3-byte
        "[]=", "<=>", "===", //
        // 2-byte
        "[]", "==", "!=", "<=", ">=", "<<", ">>", "**", "=~", "!~", "+@", "-@", //
        // 1-byte
        "+", "-", "*", "/", "%", "<", ">", "&", "|", "^", "~", "!",
    ];

    /// Read a variable-style symbol name `@x`, `@@x`, or `$x` at the cursor.
    /// Returns `None` (leaving the cursor unmoved) when none matches.
    ///
    /// The sigil bytes (`@`, `@@`, `$`) are included in the returned name —
    /// pattern matchers compare against `(ivar :@foo)` / `(cvar :@@foo)` /
    /// `(gvar :$foo)`, where the AST node's first child carries the sigil.
    /// At least one `[A-Za-z_]` byte must follow the sigil; numeric globals
    /// (`$1`, `$~`, ...) are out of scope.
    fn take_var_symbol_name(&mut self) -> Option<String> {
        let start = self.pos;
        let after_sigil = match self.peek() {
            Some(b'@') => {
                if self.src.get(start + 1) == Some(&b'@') {
                    start + 2
                } else {
                    start + 1
                }
            }
            Some(b'$') => start + 1,
            _ => return None,
        };
        if !matches!(
            self.src.get(after_sigil),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_')
        ) {
            return None;
        }
        let mut end = after_sigil + 1;
        while matches!(
            self.src.get(end),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            end += 1;
        }
        self.pos = end;
        Some(self.slice_str(start, end).to_string())
    }

    /// Read a Ruby operator-method name (`+`, `[]=`, `<=>`, ...) at the cursor.
    /// Returns `None` (leaving the cursor unmoved) when none matches.
    fn take_operator_name(&mut self) -> Option<String> {
        let rest = &self.src[self.pos..];
        for &op in Self::OPERATOR_NAMES {
            if rest.starts_with(op.as_bytes()) {
                self.pos += op.len();
                return Some(op.to_string());
            }
        }
        None
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
        // `%` is not supported in v1 (`<` is now LAngle for any-order,
        // `[` / `]` are LBracket / RBracket for intersection).
        let src = "(send %1)";
        let e = tokenize(src).expect_err("must reject `%`");
        assert!(
            e.message.contains('%') && e.message.contains("not supported in v1"),
            "message for `%` was: {}",
            e.message
        );
        // span points at `%` which is at byte offset 6.
        assert_eq!(e.span.start, 6);
    }

    #[test]
    fn lex_bracket_tokens() {
        // `[` → LBracket, `]` → RBracket; no error.
        let t = tokenize("[int]").expect("bracket tokens must lex ok");
        assert_eq!(t[0].tok, Token::LBracket);
        assert_eq!((t[0].span.start, t[0].span.end), (0, 1));
        assert_eq!(t[2].tok, Token::RBracket);
        assert_eq!((t[2].span.start, t[2].span.end), (4, 5));
    }

    #[test]
    fn lex_angle_brackets() {
        // `<` → LAngle, `>` → RAngle; no error.
        let t = tokenize("<int>").expect("angle brackets must lex ok");
        assert_eq!(t[0].tok, Token::LAngle);
        assert_eq!((t[0].span.start, t[0].span.end), (0, 1));
        assert_eq!(t[2].tok, Token::RAngle);
        assert_eq!((t[2].span.start, t[2].span.end), (4, 5));
    }

    #[test]
    fn lex_error_on_non_ascii_char() {
        // A non-ASCII char is malformed input; the message must render the
        // real char, not a Latin-1 mojibake of its UTF-8 lead byte.
        let e = tokenize("café").expect_err("must reject non-ASCII char");
        assert!(
            e.message.contains('é'),
            "message should name the real char, was: {}",
            e.message
        );
    }

    #[test]
    fn bare_predicate_name_lexes_as_ident_then_question() {
        // `even?` no longer errors at the lexer — the trailing `?` is a
        // postfix [`Token::Question`] quantifier, so this lexes as
        // `Ident("even") Question`. (Whether `even?` is a *valid* pattern is
        // a parser concern: `even` is not a known node kind.)
        assert_eq!(
            toks("even?"),
            vec![Token::Ident("even".into()), Token::Question]
        );
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
        // D4 (murphy-nnr8): `_x` lexes as `Token::Unify("x")`, not `Ident`.
        assert_eq!(toks("_x"), vec![Token::Unify("x".into())]);
    }

    // --- D4 (murphy-nnr8): tUNIFY — `_name` named wildcard ----------------

    #[test]
    fn unify_token_lexes_for_underscore_lowercase_ident() {
        // `_x`, `_foo`, `_a1` → Token::Unify (name without leading `_`).
        assert_eq!(toks("_x"), vec![Token::Unify("x".into())]);
        assert_eq!(toks("_foo"), vec![Token::Unify("foo".into())]);
        assert_eq!(toks("_a1"), vec![Token::Unify("a1".into())]);
        assert_eq!(toks("_my_var"), vec![Token::Unify("my_var".into())]);
    }

    #[test]
    fn underscore_alone_stays_underscore_not_unify() {
        assert_eq!(toks("_"), vec![Token::Underscore]);
    }

    #[test]
    fn underscore_uppercase_stays_ident_not_unify() {
        // `_X` is NOT tUNIFY — uppercase after `_` remains Ident.
        assert_eq!(toks("_X"), vec![Token::Ident("_X".into())]);
        assert_eq!(toks("_Foo"), vec![Token::Ident("_Foo".into())]);
    }

    #[test]
    fn unify_in_node_pattern_context() {
        // `(send _x _ _x)` — unification example.
        assert_eq!(
            toks("(send _x _ _x)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::Unify("x".into()),
                Token::Underscore,
                Token::Unify("x".into()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn predicate_with_question_suffix() {
        assert_eq!(toks("#odd?"), vec![Token::Predicate("odd?".into())]);
        assert_eq!(toks("#has!"), vec![Token::Predicate("has!".into())]);
    }

    #[test]
    fn bang_identifier_lexes_as_bare_ident_plus_bang() {
        assert_eq!(
            toks("save!"),
            vec![Token::Ident("save".into()), Token::Bang]
        );
    }

    // --- murphy-ycx: postfix quantifier tokens (`*`, `+`, `?`) -------------

    #[test]
    fn quantifier_tokens_lex_standalone() {
        assert_eq!(toks("*"), vec![Token::Star]);
        assert_eq!(toks("+"), vec![Token::Plus]);
        assert_eq!(toks("?"), vec![Token::Question]);
    }

    #[test]
    fn ident_with_question_lexes_as_two_tokens() {
        // `int?` -> `Ident("int") Question`. The `?` is *not* absorbed into
        // the ident; the parser reads it as a postfix quantifier.
        assert_eq!(
            toks("int?"),
            vec![Token::Ident("int".into()), Token::Question]
        );
        assert_eq!(
            toks("hash?"),
            vec![Token::Ident("hash".into()), Token::Question]
        );
    }

    #[test]
    fn ident_with_quantifier_suffix_lexes_as_two_tokens() {
        assert_eq!(toks("int+"), vec![Token::Ident("int".into()), Token::Plus]);
        assert_eq!(toks("sym*"), vec![Token::Ident("sym".into()), Token::Star]);
    }

    #[test]
    fn quantifier_inside_node_child_list() {
        // The whole sequence `(send _ :foo int*)` reaches the parser as a
        // sensible token stream.
        assert_eq!(
            toks("(send _ :foo int*)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::Underscore,
                Token::Sym("foo".into()),
                Token::Ident("int".into()),
                Token::Star,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn nil_question_still_lexes_as_special_token() {
        // The `nil?` -> `NilQuestion` carve-out must survive: the `?` is part
        // of the ident in *this one case*, not a separate Question token.
        assert_eq!(toks("nil?"), vec![Token::NilQuestion]);
    }

    #[test]
    fn predicate_with_question_suffix_still_works() {
        // `#odd?` / `#has!` are predicate forms — the `?`/`!` belong to the
        // method name and are consumed by `scan_predicate`, not surfaced as
        // separate tokens.
        assert_eq!(toks("#odd?"), vec![Token::Predicate("odd?".into())]);
        assert_eq!(toks("#has!"), vec![Token::Predicate("has!".into())]);
    }

    #[test]
    fn symbol_with_question_or_bang_suffix_still_works() {
        // `:foo?` / `:foo!` lex as the corresponding `Sym` — the `?`/`!`
        // belong to the symbol name (`scan_symbol` -> `take_method_name`),
        // not surfaced as separate tokens.
        assert_eq!(toks(":foo?"), vec![Token::Sym("foo?".into())]);
        assert_eq!(toks(":foo!"), vec![Token::Sym("foo!".into())]);
    }

    // --- murphy-wsep: pipe `|` as union group separator --------------------

    #[test]
    fn pipe_token_lexes_standalone() {
        // `|` is now a valid Token::Pipe — no longer an "unexpected character".
        assert_eq!(toks("|"), vec![Token::Pipe]);
    }

    #[test]
    fn pipe_token_in_union_context() {
        // `{a | b}` produces LBrace Ident(a) Pipe Ident(b) RBrace.
        assert_eq!(
            toks("{a | b}"),
            vec![
                Token::LBrace,
                Token::Ident("a".into()),
                Token::Pipe,
                Token::Ident("b".into()),
                Token::RBrace,
            ]
        );
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

    // --- symbol grammar: operators and uppercase (murphy-ke0) --------------

    #[test]
    fn lexes_operator_symbols() {
        for op in [
            "+", "-", "*", "/", "%", "<", ">", "&", "|", "^", "~", "!", "[]", "==", "!=", "<=",
            ">=", "<<", ">>", "**", "=~", "!~", "+@", "-@", "[]=", "<=>", "===",
        ] {
            assert_eq!(
                toks(&format!(":{op}")),
                vec![Token::Sym(op.into())],
                "`:{op}` should lex as an operator symbol",
            );
        }
    }

    #[test]
    fn operator_symbol_takes_longest_match() {
        // A shorter operator must never shadow a longer one at the cursor.
        assert_eq!(toks(":<=>"), vec![Token::Sym("<=>".into())]);
        assert_eq!(toks(":<="), vec![Token::Sym("<=".into())]);
        assert_eq!(toks(":<"), vec![Token::Sym("<".into())]);
        assert_eq!(toks(":[]="), vec![Token::Sym("[]=".into())]);
        assert_eq!(toks(":[]"), vec![Token::Sym("[]".into())]);
        assert_eq!(toks(":+@"), vec![Token::Sym("+@".into())]);
        assert_eq!(toks(":**"), vec![Token::Sym("**".into())]);
    }

    #[test]
    fn operator_symbol_in_node_match() {
        assert_eq!(
            toks("(send _ :<=> _)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::Underscore,
                Token::Sym("<=>".into()),
                Token::Underscore,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn operator_symbol_does_not_absorb_trailing_digit() {
        // `:+`/`:-` are operator symbols; the digit is a separate token, and
        // `:-1` does not become a negative-number literal.
        assert_eq!(toks(":+1"), vec![Token::Sym("+".into()), Token::Int(1)]);
        assert_eq!(toks(":-1"), vec![Token::Sym("-".into()), Token::Int(1)]);
    }

    #[test]
    fn colon_bang_is_a_symbol_not_bang_token() {
        // After `:`, `!` is the operator-method symbol, distinct from the
        // standalone `Token::Bang` sigil.
        assert_eq!(toks(":!"), vec![Token::Sym("!".into())]);
    }

    #[test]
    fn bare_equals_symbol_is_error() {
        // `=` alone is not a Ruby operator-method name.
        let e = tokenize(":=").expect_err("must reject `:=`");
        assert!(e.message.contains("expected a symbol name"));
    }

    #[test]
    fn lexes_uppercase_symbols() {
        assert_eq!(toks(":Foo"), vec![Token::Sym("Foo".into())]);
        assert_eq!(toks(":CONST"), vec![Token::Sym("CONST".into())]);
        assert_eq!(toks(":fooBar"), vec![Token::Sym("fooBar".into())]);
    }

    #[test]
    fn uppercase_predicate_name_is_accepted() {
        // `take_method_name` is shared with `#name`; allowing uppercase there
        // is harmless — Ruby permits uppercase method names.
        assert_eq!(toks("#Foo"), vec![Token::Predicate("Foo".into())]);
    }

    // --- symbol grammar: setter names (murphy-m7q) -------------------------

    #[test]
    fn lexes_setter_symbols() {
        // An identifier-style name with a trailing `=` is a Ruby setter
        // method name (`foo=`); patterns match it as `(send _ :foo= _)`.
        assert_eq!(toks(":foo="), vec![Token::Sym("foo=".into())]);
        assert_eq!(toks(":bar="), vec![Token::Sym("bar=".into())]);
        assert_eq!(toks(":Foo="), vec![Token::Sym("Foo=".into())]);
    }

    #[test]
    fn setter_symbol_in_node_match() {
        assert_eq!(
            toks("(send _ :foo= _)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::Underscore,
                Token::Sym("foo=".into()),
                Token::Underscore,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn setter_symbol_takes_exactly_one_equals() {
        // The `=` suffix is consumed once; a following character is its own
        // token, so `:foo=1` is a setter symbol then an integer.
        assert_eq!(
            toks(":foo=1"),
            vec![Token::Sym("foo=".into()), Token::Int(1)]
        );
    }

    #[test]
    fn predicate_setter_name_is_accepted() {
        // `take_method_name` is shared with `#name`; allowing a trailing `=`
        // there is harmless — Ruby permits setter method names.
        assert_eq!(toks("#foo="), vec![Token::Predicate("foo=".into())]);
    }

    // --- symbol grammar: variable-style names (murphy-afl) -----------------

    #[test]
    fn lexes_ivar_symbols() {
        // `:@x` matches the first child of an `(ivar :@foo)` AST node — the
        // `@` sigil is part of the symbol's name, not stripped.
        assert_eq!(toks(":@x"), vec![Token::Sym("@x".into())]);
        assert_eq!(toks(":@foo"), vec![Token::Sym("@foo".into())]);
        assert_eq!(toks(":@Foo"), vec![Token::Sym("@Foo".into())]);
        assert_eq!(toks(":@_name"), vec![Token::Sym("@_name".into())]);
        assert_eq!(toks(":@x1"), vec![Token::Sym("@x1".into())]);
    }

    #[test]
    fn lexes_cvar_symbols() {
        assert_eq!(toks(":@@x"), vec![Token::Sym("@@x".into())]);
        assert_eq!(toks(":@@foo"), vec![Token::Sym("@@foo".into())]);
        assert_eq!(toks(":@@Foo"), vec![Token::Sym("@@Foo".into())]);
    }

    #[test]
    fn lexes_gvar_symbols() {
        assert_eq!(toks(":$x"), vec![Token::Sym("$x".into())]);
        assert_eq!(toks(":$foo"), vec![Token::Sym("$foo".into())]);
        assert_eq!(toks(":$LOAD_PATH"), vec![Token::Sym("$LOAD_PATH".into())]);
    }

    #[test]
    fn variable_symbols_in_node_match() {
        assert_eq!(
            toks("(ivar :@foo)"),
            vec![
                Token::LParen,
                Token::Ident("ivar".into()),
                Token::Sym("@foo".into()),
                Token::RParen,
            ]
        );
        assert_eq!(
            toks("(cvar :@@foo)"),
            vec![
                Token::LParen,
                Token::Ident("cvar".into()),
                Token::Sym("@@foo".into()),
                Token::RParen,
            ]
        );
        assert_eq!(
            toks("(gvar :$foo)"),
            vec![
                Token::LParen,
                Token::Ident("gvar".into()),
                Token::Sym("$foo".into()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn variable_symbol_span_covers_sigil_and_name() {
        let t = tokenize(":@foo").expect("ok");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].tok, Token::Sym("@foo".into()));
        assert_eq!((t[0].span.start, t[0].span.end), (0, 5));

        let t = tokenize(":@@foo").expect("ok");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].tok, Token::Sym("@@foo".into()));
        assert_eq!((t[0].span.start, t[0].span.end), (0, 6));

        let t = tokenize(":$foo").expect("ok");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].tok, Token::Sym("$foo".into()));
        assert_eq!((t[0].span.start, t[0].span.end), (0, 5));
    }

    #[test]
    fn bare_ivar_sigil_is_error() {
        // `:@` alone has no name; must error rather than yield `Sym("@")`.
        let e = tokenize(":@").expect_err("must reject `:@`");
        assert!(e.message.contains("expected a symbol name"));
    }

    #[test]
    fn bare_cvar_sigil_is_error() {
        let e = tokenize(":@@").expect_err("must reject `:@@`");
        assert!(e.message.contains("expected a symbol name"));
    }

    #[test]
    fn bare_gvar_sigil_is_error() {
        let e = tokenize(":$").expect_err("must reject `:$`");
        assert!(e.message.contains("expected a symbol name"));
    }

    #[test]
    fn variable_symbol_with_digit_after_sigil_is_error() {
        // `:@1` and `:$1` aren't supported in v1 — variable-style symbol
        // names must start with `[A-Za-z_]` after the sigil.
        let e = tokenize(":@1").expect_err("must reject `:@1`");
        assert!(e.message.contains("expected a symbol name"));
        let e = tokenize(":$1").expect_err("must reject `:$1`");
        assert!(e.message.contains("expected a symbol name"));
    }

    // --- D3 (murphy-kq57): tPARAM_CONST — uppercase-start ident ---------------

    #[test]
    fn lexes_uppercase_ident_as_ident_token() {
        // Bare `Foo` (no `%`) lexes as `Ident("Foo")`.
        assert_eq!(toks("Foo"), vec![Token::Ident("Foo".into())]);
        assert_eq!(toks("MyClass"), vec![Token::Ident("MyClass".into())]);
        assert_eq!(toks("FOO"), vec![Token::Ident("FOO".into())]);
    }

    #[test]
    fn percent_uppercase_lexes_as_ident_without_percent() {
        // `%Foo` — tPARAM_CONST sugar: `%` is consumed and the name lexes as
        // `Ident("Foo")`, identical to the bare `Foo` form.
        assert_eq!(toks("%Foo"), vec![Token::Ident("Foo".into())]);
        assert_eq!(toks("%MyClass"), vec![Token::Ident("MyClass".into())]);
    }

    #[test]
    fn percent_lowercase_is_still_rejected() {
        // `%var` (tPARAM_NAMED) remains out of v1 scope — must error.
        let e = tokenize("%var").expect_err("must reject `%var`");
        assert!(
            e.message.contains('%') && e.message.contains("not supported in v1"),
            "message for `%var` was: {}",
            e.message
        );
    }

    #[test]
    fn uppercase_ident_in_node_pattern() {
        // `(send _ :raise Foo)` produces correct token stream.
        assert_eq!(
            toks("(send _ :raise Foo)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::Underscore,
                Token::Sym("raise".into()),
                Token::Ident("Foo".into()),
                Token::RParen,
            ]
        );
    }
}
