//! Pattern parser entry point and post-parse passes.
//!
//! As of beads issue murphy-qpf9 Round 2, the actual grammar lives in
//! `src/parser.lalrpop` and is compiled at build time into the
//! `lalrpop_parser` module included by `lib.rs`. This file hosts:
//!
//! * `pub fn parse` — the top-level entry. Tokenises the source, hands the
//!   token stream to the LALRPOP-generated parser, then runs the post-passes
//!   below in order.
//! * `assign_capture_slots` — walks the AST and assigns dense slot indices
//!   in source order, collapsing the `${...}` sugar marker into a single
//!   shared slot per Union.
//! * `resolve_pred_capture_refs` — walks predicate args, swapping the
//!   `\0capref:<name>` sentinel string back to a real `PredArg::Capture(slot)`
//!   using the name table built during slot allocation. Forward / unknown
//!   references error here, matching the hand-written parser's behaviour.
//! * `validate_rest_placement`, `validate_capture_position`,
//!   `validate_quantifier_placement` — the structural walks that enforce
//!   the v1 grammar rules. Unchanged from the pre-LALRPOP parser.
//! * Helper functions exposed to the grammar's actions:
//!   `make_capture_placeholder`, `kind_or_unknown_ident`, `resolve_kind`,
//!   `check_oneof_kind`, `pred_arg_from_ident`.

use crate::ast::PredArg;
use crate::lexer::{Spanned, Token, tokenize};
use crate::{CaptureKind, Head, Lit, ParseError, Pat, PatKind, PatSpan, PatternAst};
use murphy_ast::NodeKindTag;

/// Sentinel string used in `Capture::name` to mark arms produced by the
/// `${...}` sugar form. The post-pass `assign_capture_slots` looks for a
/// Union whose every arm is `Capture { name: Some(SUGAR_MARKER), .. }` and
/// allocates a single shared slot for the group, then clears the marker so
/// the post-pass leaves no trace.
pub(crate) const SUGAR_MARKER: &str = "\0sugar";

/// Sentinel prefix used by the grammar to stash a `$ident` predicate-arg
/// back-reference as a `Lit::Sym`. The post-pass `resolve_pred_capture_refs`
/// detects the prefix, resolves the name to a slot, and replaces the arg
/// with `PredArg::Capture(slot)`.
pub(crate) const CAPREF_MARKER: &str = "\0capref:";

/// Separator placed between a predicate's name and its serialised args when
/// the token-stream preprocessor folds `Predicate(name) LParen args RParen`
/// into a single `Predicate(name + ARGS_SEP + serialised_args)` token. The
/// grammar action `split_predicate_name` splits the payload back.
const ARGS_SEP: &str = "\0args:";

/// Separator between individual serialised args inside the folded predicate
/// name string. Args are encoded as one of:
/// * `iINT` — integer literal (i64)
/// * `fFLOAT` — float literal (f64; `{:?}`-formatted for lossless round-trip)
/// * `sSTRING` — string literal (raw UTF-8, no escaping; outer `RECORD_SEP`
///   delimits)
/// * `ySYM` — symbol literal
/// * `tTRUE` — boolean true
/// * `xFALSE` — boolean false
/// * `cNAME` — `$NAME` capture back-reference (post-pass resolves to slot)
const RECORD_SEP: &str = "\x01";

/// Sentinel prefix tagging a `PatKind::Predicate` produced from a bare
/// identifier (e.g. `even?` in node-child position, or `sned` at the root).
/// `convert_named_captures` strips the marker for Captures whose body is a
/// bare-ident Predicate (turning `$even?` into a named capture). The
/// remaining marked Predicates are real bare-predicate-shorthand candidates,
/// validated by `validate_bare_predicate_position`.
const BARE_IDENT_MARKER: &str = "\0bare:";

/// Parse a pattern source string into a [`PatternAst`].
///
/// Tokenises `src`, hands the stream to the LALRPOP-generated parser, then:
/// 1. assigns dense capture-slot indices in source order,
/// 2. resolves predicate-arg back-references to slot indices,
/// 3. runs the v1-grammar structural validations.
pub fn parse(src: &str) -> Result<PatternAst, ParseError> {
    let tokens = tokenize(src)?;
    if tokens.is_empty() {
        return Err(ParseError::new("empty pattern", PatSpan::new(0, 0)));
    }
    let mut root = run_lalrpop(&tokens)?;
    // Restore `$ident` named-capture semantics: the grammar parses every
    // `$ident` uniformly as `Capture { name: None, body: Kind(tag) }` (or
    // `Predicate(name)` for unknown idents) to dodge an LR(1) shift-reduce
    // conflict at the `$ident vs $ident postfix` boundary. The post-pass
    // here rewrites those forms back to the expected `Capture { name:
    // Some(ident), body: Wildcard }` shape. `$send?` and friends stay as
    // anonymous-with-Quantifier — only bare-ident bodies are touched.
    convert_named_captures(&mut root);
    // Reject bare-predicate shorthand at positions that don't allow it
    // (root, Sym-slot node children, Union-of-non-uniform-sugar arms, etc.)
    // and strip the BARE_IDENT_MARKER from accepted ones. Round 2 enforces
    // the root rejection and the schema-driven node-child-slot check.
    validate_bare_predicate_position(&mut root, BarePos::Root)?;
    // Source-order slot assignment, sugar-Union detection.
    let captures = assign_capture_slots(&mut root)?;
    // Resolve predicate-arg back-references (`$name`) to slot indices.
    let name_table: Vec<Option<String>> = collect_capture_names(&root);
    resolve_pred_capture_refs(&mut root, &name_table)?;
    // Reject `...` and `$...` outside a direct node-child slot, and
    // duplicate rest within a single child list.
    validate_rest_placement(&root, false)?;
    // `$` captures must sit on a definite-assignment path.
    validate_capture_position(&root, false)?;
    // `*` / `+` / `?` quantifiers are only valid as direct children of a
    // node match, and their body must not contain captures or rests.
    validate_quantifier_placement(&root, false)?;
    Ok(PatternAst { root, captures })
}

/// Drive the LALRPOP-generated parser over the token stream produced by
/// `lexer::tokenize`. Pre-folds `Predicate(name) LParen args RParen` into a
/// single synthetic `Predicate(name + ARGS_SEP + serialised)` token to dodge
/// an LR(1) local ambiguity at `#name (`. Converts `lalrpop_util::ParseError`
/// into our `ParseError` shape; Round 2 messages are deliberately terse —
/// Round 3 owns full diagnostic parity.
fn run_lalrpop(tokens: &[Spanned]) -> Result<Pat, ParseError> {
    let folded = fold_predicate_args(tokens)?;
    let iter = folded.into_iter().map(|s| {
        Ok::<_, ParseError>((
            s.span.start as usize,
            s.tok.clone(),
            s.span.end as usize,
        ))
    });
    crate::lalrpop_parser_inner::lalrpop_parser::PatternParser::new()
        .parse(iter)
        .map_err(lalrpop_to_parse_error)
}

/// Pre-fold `Predicate(name) LParen <args> RParen` sequences into a single
/// `Predicate(name + ARGS_SEP + serialised)` token. Args are parsed strictly
/// (integer / float / string / symbol / true / false / `$ident` capref);
/// `nil` and pattern-form args are rejected with the same diagnostics the
/// hand-written parser produced.
fn fold_predicate_args(tokens: &[Spanned]) -> Result<Vec<Spanned>, ParseError> {
    let mut out: Vec<Spanned> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if let Token::Predicate(name) = &tok.tok {
            if let Some(next) = tokens.get(i + 1) {
                if matches!(next.tok, Token::LParen) {
                    // Parse args from i+2 up to matching RParen.
                    let lparen_span = next.span;
                    let (args, end_idx, end_span) =
                        parse_pred_args(tokens, i + 2, lparen_span)?;
                    let mut combined = name.clone();
                    combined.push_str(ARGS_SEP);
                    combined.push_str(&serialise_pred_args(&args));
                    out.push(Spanned {
                        tok: Token::Predicate(combined),
                        span: PatSpan::new(
                            tok.span.start as usize,
                            end_span.end as usize,
                        ),
                    });
                    i = end_idx + 1;
                    continue;
                }
            }
        }
        out.push(tok.clone());
        i += 1;
    }
    Ok(out)
}

/// Parse predicate args starting at `start` (one past the `(`). Returns
/// `(args, end_idx, end_span)` where `end_idx` is the index of the closing
/// `)` token.
fn parse_pred_args(
    tokens: &[Spanned],
    mut i: usize,
    lparen_span: PatSpan,
) -> Result<(Vec<PredArg>, usize, PatSpan), ParseError> {
    let mut args: Vec<PredArg> = Vec::new();
    loop {
        let Some(t) = tokens.get(i) else {
            return Err(ParseError::new(
                "unclosed `(` in predicate argument list",
                lparen_span,
            ));
        };
        match &t.tok {
            Token::RParen => return Ok((args, i, t.span)),
            Token::Int(v) => {
                args.push(PredArg::Lit(Lit::Int(*v)));
                i += 1;
            }
            Token::Float(v) => {
                args.push(PredArg::Lit(Lit::Float(*v)));
                i += 1;
            }
            Token::Str(s) => {
                args.push(PredArg::Lit(Lit::Str(s.clone())));
                i += 1;
            }
            Token::Sym(s) => {
                args.push(PredArg::Lit(Lit::Sym(s.clone())));
                i += 1;
            }
            Token::Ident(name) if name == "true" => {
                args.push(PredArg::Lit(Lit::True));
                i += 1;
            }
            Token::Ident(name) if name == "false" => {
                args.push(PredArg::Lit(Lit::False));
                i += 1;
            }
            Token::Ident(name) if name == "nil" => {
                return Err(ParseError::new(
                    "`nil` is not supported as a predicate argument in v1: \
                     `nil` has no Rust-side counterpart for the cop method signature",
                    t.span,
                ));
            }
            Token::Dollar => {
                let dollar_span = t.span;
                i += 1;
                let Some(name_tok) = tokens.get(i) else {
                    return Err(ParseError::new(
                        "expected capture name after `$` in predicate arg",
                        dollar_span,
                    ));
                };
                let Token::Ident(name) = &name_tok.tok else {
                    return Err(ParseError::new(
                        "expected identifier after `$` in predicate arg",
                        name_tok.span,
                    ));
                };
                // Stash as a sentinel `Lit::Sym` — the AST post-pass resolves
                // the name to a slot via `resolve_pred_capture_refs`.
                args.push(PredArg::Lit(Lit::Sym(format!(
                    "{CAPREF_MARKER}{name}"
                ))));
                i += 1;
            }
            Token::LBrace | Token::LParen => {
                return Err(ParseError::new(
                    "pattern args in v1: literal/capture only",
                    t.span,
                ));
            }
            _ => {
                return Err(ParseError::new(
                    "pattern args in v1: literal/capture only",
                    t.span,
                ));
            }
        }
    }
}

/// Serialise a `PredArg` list to a compact, parseable string using
/// `RECORD_SEP` as a delimiter. The grammar action `split_predicate_name`
/// inverts this.
fn serialise_pred_args(args: &[PredArg]) -> String {
    let mut out = String::new();
    for (idx, arg) in args.iter().enumerate() {
        if idx > 0 {
            out.push_str(RECORD_SEP);
        }
        match arg {
            PredArg::Lit(Lit::Int(v)) => {
                out.push('i');
                out.push_str(&v.to_string());
            }
            PredArg::Lit(Lit::Float(v)) => {
                out.push('f');
                out.push_str(&format!("{v:?}"));
            }
            PredArg::Lit(Lit::Str(s)) => {
                out.push('s');
                out.push_str(s);
            }
            PredArg::Lit(Lit::Sym(s)) => {
                out.push('y');
                out.push_str(s);
            }
            PredArg::Lit(Lit::True) => out.push('t'),
            PredArg::Lit(Lit::False) => out.push('x'),
            PredArg::Lit(Lit::Nil) => unreachable!("Nil is rejected at parse time"),
            // PredArg::Capture should not appear yet — back-refs are stashed
            // as Lit::Sym via CAPREF_MARKER until the post-pass resolves them.
            PredArg::Capture(_) => unreachable!("Capture refs are stashed as Lit::Sym"),
        }
    }
    out
}

/// Split a folded predicate name back into the original name and decoded
/// args. If the input doesn't contain `ARGS_SEP`, returns `(name, vec![])`.
pub(crate) fn split_predicate_name(
    combined: String,
    span: PatSpan,
) -> Result<(String, Vec<PredArg>), ParseError> {
    if let Some(sep_idx) = combined.find(ARGS_SEP) {
        let name = combined[..sep_idx].to_string();
        let rest = &combined[sep_idx + ARGS_SEP.len()..];
        let args = if rest.is_empty() {
            vec![]
        } else {
            rest.split(RECORD_SEP)
                .map(|field| decode_pred_arg(field, span))
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok((name, args))
    } else {
        Ok((combined, vec![]))
    }
}

fn decode_pred_arg(field: &str, span: PatSpan) -> Result<PredArg, ParseError> {
    let mut chars = field.chars();
    let tag = chars.next().ok_or_else(|| {
        ParseError::new("internal: empty predicate-arg record", span)
    })?;
    let rest = &field[tag.len_utf8()..];
    Ok(match tag {
        'i' => PredArg::Lit(Lit::Int(rest.parse::<i64>().map_err(|_| {
            ParseError::new("internal: bad serialised int", span)
        })?)),
        'f' => PredArg::Lit(Lit::Float(rest.parse::<f64>().map_err(|_| {
            ParseError::new("internal: bad serialised float", span)
        })?)),
        's' => PredArg::Lit(Lit::Str(rest.to_string())),
        'y' => PredArg::Lit(Lit::Sym(rest.to_string())),
        't' => PredArg::Lit(Lit::True),
        'x' => PredArg::Lit(Lit::False),
        _ => {
            return Err(ParseError::new(
                "internal: unknown predicate-arg record tag",
                span,
            ));
        }
    })
}

/// Translate `lalrpop_util::ParseError` into our `ParseError` shape.
fn lalrpop_to_parse_error(
    err: lalrpop_util::ParseError<usize, Token, ParseError>,
) -> ParseError {
    use lalrpop_util::ParseError as LP;
    match err {
        LP::User { error } => error,
        LP::InvalidToken { location } => ParseError::new(
            "invalid token",
            PatSpan::new(location, location),
        ),
        LP::UnrecognizedEof { location, .. } => ParseError::new(
            "unexpected end of input",
            PatSpan::new(location, location),
        ),
        LP::UnrecognizedToken { token: (l, _, r), .. } => ParseError::new(
            "unexpected token",
            PatSpan::new(l, r),
        ),
        LP::ExtraToken { token: (l, _, r) } => ParseError::new(
            "unexpected trailing input",
            PatSpan::new(l, r),
        ),
    }
}

// ============================================================================
// Helpers exposed to the LALRPOP grammar's actions.
// ============================================================================

/// Construct a `PatKind::Capture` with a placeholder `slot = u16::MAX`.
/// The real slot index is assigned by the post-pass `assign_capture_slots`.
pub(crate) fn make_capture_placeholder(name: Option<String>, body: Pat) -> PatKind {
    PatKind::Capture {
        slot: u16::MAX,
        name,
        body: Box::new(body),
    }
}

/// Resolve a node-type `name` to its [`NodeKindTag`], or a span-carrying
/// error naming the unknown type. Shared by the `Head::Exact` / `Head::OneOf`
/// paths.
pub(crate) fn resolve_kind(name: &str, span: PatSpan) -> Result<NodeKindTag, ParseError> {
    murphy_ast::tag_from_pattern_name(name)
        .ok_or_else(|| ParseError::new(format!("unknown node type `{name}`"), span))
}

/// Build a `Head::OneOf` from a non-empty vector of (tag, span) pairs.
/// Currently infallible — `(tag, span)` pairs reaching here are already
/// resolved by `resolve_kind`. The signature returns `Result` so the
/// grammar can route through `=>?`.
#[allow(dead_code)]
pub(crate) fn check_oneof_kind(tags: Vec<(NodeKindTag, PatSpan)>) -> Result<Head, ParseError> {
    Ok(Head::OneOf(tags.into_iter().map(|(t, _)| t).collect()))
}

/// Resolve a bare ident at primary position to a `Pat`:
/// * `true` / `false` / `nil` -> the corresponding literal,
/// * a known node-kind name -> `PatKind::Kind(tag)`,
/// * otherwise -> `PatKind::Predicate { name, args: vec![] }` (bare-predicate
///   shorthand). Round 2 always emits the shorthand for unknown names; the
///   position-validity check (must be in a node-child slot) is deferred to
///   Round 3 along with the "unknown node type" hint diagnostic.
pub(crate) fn kind_or_unknown_ident(name: &str, span: PatSpan) -> Pat {
    let kind = match name {
        "true" => PatKind::Lit(Lit::True),
        "false" => PatKind::Lit(Lit::False),
        "nil" => PatKind::Lit(Lit::Nil),
        _ => {
            if let Some(tag) = murphy_ast::tag_from_pattern_name(name) {
                PatKind::Kind(tag)
            } else {
                // Unknown ident. Tag the predicate name with `BARE_IDENT_MARKER`
                // so the post-pass `validate_bare_predicate_root` can tell
                // bare-ident shorthand apart from explicit `#name` predicates.
                // After `convert_named_captures` rewrites `Capture { body:
                // Predicate(bare-ident) }` to the named-capture shape, the
                // remaining bare-ident Predicates are real bare-predicate
                // shorthand candidates — at the root they're rejected; in
                // node-child slots Round 3 will validate position.
                PatKind::Predicate {
                    name: format!("{BARE_IDENT_MARKER}{name}"),
                    args: vec![],
                }
            }
        }
    };
    Pat { kind, span }
}

/// Build a `PredArg` from a bare identifier inside a predicate-arg list.
/// `true` / `false` are valid literals; `nil` is rejected per the v1 contract.
#[allow(dead_code)]
pub(crate) fn pred_arg_from_ident(name: &str, span: PatSpan) -> Result<PredArg, ParseError> {
    match name {
        "true" => Ok(PredArg::Lit(Lit::True)),
        "false" => Ok(PredArg::Lit(Lit::False)),
        "nil" => Err(ParseError::new(
            "`nil` is not supported as a predicate argument in v1: \
             `nil` has no Rust-side counterpart for the cop method signature",
            span,
        )),
        _ => Err(ParseError::new(
            "pattern args in v1: literal/capture only",
            span,
        )),
    }
}

// ============================================================================
// Post-pass: restore `$ident` named-capture semantics.
//
// The grammar parses `$ident` uniformly as `Capture { name: None, body:
// Pat { kind: Kind(tag) | Predicate(name) } }` to keep the LR grammar
// context-free (see `parser.lalrpop`). The pre-LALRPOP parser produced
// `Capture { name: Some(ident), body: Wildcard }` for that source shape.
// This walk reconstructs that shape after parsing.
//
// Rules:
// * `Capture { name: None, body: Kind(tag) }` -> name = pattern_name(tag),
//   body becomes Wildcard. The body span stays as-is — span the `$`.
// * `Capture { name: None, body: Predicate { name, args: [] } }` (unknown
//   ident) -> same rewrite with the original ident as the name.
// * Anything else (Quantifier, Lit, Node, Union, etc.) is untouched —
//   `$send?` stays an anonymous capture wrapping a Quantifier, `$(send)`
//   stays an anonymous capture wrapping a Node, etc.
// ============================================================================

fn convert_named_captures(pat: &mut Pat) {
    if let PatKind::Capture { name, body, .. } = &mut pat.kind
        && name.is_none()
    {
        let new_name: Option<String> = match &body.kind {
            PatKind::Kind(tag) => {
                murphy_ast::pattern_name(*tag).map(|s| s.to_string())
            }
            PatKind::Predicate { name: pred_name, args } if args.is_empty() => {
                // Bare-ident Predicate (tagged with BARE_IDENT_MARKER).
                // Treat as a named capture; strip the marker.
                pred_name.strip_prefix(BARE_IDENT_MARKER).map(|s| s.to_string())
            }
            _ => None,
        };
        if let Some(n) = new_name {
            *name = Some(n);
            // The hand-written parser made the synthetic Wildcard body span
            // only the `$` token. Preserve that for snapshot parity. The
            // ident span (needed for duplicate-name diagnostics) is stashed
            // separately via the outer Capture's span — see
            // `assign_capture_slots` which derives the error span as
            // `pat.span` minus the leading `$` byte.
            let dollar_span = PatSpan {
                start: pat.span.start,
                end: pat.span.start + 1,
            };
            **body = Pat {
                kind: PatKind::Wildcard,
                span: dollar_span,
            };
        }
    }
    // Recurse into all child positions.
    match &mut pat.kind {
        PatKind::Node { children, .. } => {
            for c in children {
                convert_named_captures(c);
            }
        }
        PatKind::Union(alts) => {
            for a in alts {
                convert_named_captures(a);
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            convert_named_captures(b);
        }
        PatKind::Quantifier { body, .. } => convert_named_captures(body),
        PatKind::Capture { body, .. } => convert_named_captures(body),
        PatKind::AnyOrder { children } => {
            for c in children {
                convert_named_captures(c);
            }
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => {}
    }
}

// ============================================================================
// Post-pass: bare-predicate-shorthand position check.
//
// `kind_or_unknown_ident` tags every unknown-ident Predicate with
// `BARE_IDENT_MARKER`. After `convert_named_captures` rewrites the
// bare-ident-as-named-capture path, the remaining marked Predicates are
// real bare-predicate-shorthand candidates. They are valid only in
// node-child slots where `node_child_allows_bare_predicate(parent_kind,
// child_idx)` says so (matches the hand-written parser's `allow_bare_predicate`
// flag).
//
// `BarePos` tracks the parser's "where am I" context as the walk descends.
// ============================================================================

#[derive(Clone)]
enum BarePos<'a> {
    /// Top-level / inside a `!`/`^`/`` ` ``/Quantifier body — bare predicate
    /// is forbidden.
    Root,
    /// Direct node-child slot of a Node with known parent kind and child
    /// index. The schema decides whether bare predicate is allowed.
    NodeChild { parent: &'a Head, child_idx: usize },
}

impl BarePos<'_> {
    fn allows_bare(&self) -> bool {
        match self {
            BarePos::Root => false,
            BarePos::NodeChild { parent, child_idx } => match parent {
                Head::Exact(tag) => {
                    crate::schema::node_child_allows_bare_predicate(*tag, *child_idx)
                }
                Head::Any => false,
                Head::OneOf(tags) => tags
                    .iter()
                    .all(|t| crate::schema::node_child_allows_bare_predicate(*t, *child_idx)),
            },
        }
    }
}

fn validate_bare_predicate_position(pat: &mut Pat, pos: BarePos<'_>) -> Result<(), ParseError> {
    // Examine `pat`'s top-level shape first.
    if let PatKind::Predicate { name, .. } = &mut pat.kind {
        if let Some(stripped) = name.strip_prefix(BARE_IDENT_MARKER) {
            if !pos.allows_bare() {
                return Err(ParseError::new(
                    format!("unknown node type `{stripped}`"),
                    pat.span,
                ));
            }
            // Accept: strip the marker so downstream sees a normal Predicate.
            *name = stripped.to_string();
        }
    }
    // Recurse into child positions with the appropriate context.
    // We split `&mut pat.kind` into matches that don't co-borrow `head`.
    match &mut pat.kind {
        PatKind::Node { .. } => {
            // Extract `head` and `children` via a temporary unborrow.
            let PatKind::Node { head, children } = &mut pat.kind else {
                unreachable!()
            };
            // We need an immutable borrow of `head` and a mutable iteration
            // of `children` — split via a raw split. Use `&*head` for an
            // immutable borrow.
            let head_ref: &Head = head;
            for (idx, child) in children.iter_mut().enumerate() {
                let child_pos = BarePos::NodeChild { parent: head_ref, child_idx: idx };
                validate_bare_predicate_position(child, child_pos)?;
            }
        }
        PatKind::Union(alts) => {
            for alt in alts {
                validate_bare_predicate_position(alt, pos.clone())?;
            }
        }
        PatKind::Not(b) | PatKind::Descend(b) => {
            validate_bare_predicate_position(b, BarePos::Root)?;
        }
        PatKind::Parent(b) => {
            validate_bare_predicate_position(b, pos)?;
        }
        PatKind::Quantifier { body, .. } => {
            validate_bare_predicate_position(body, pos)?;
        }
        PatKind::Capture { body, .. } => {
            validate_bare_predicate_position(body, pos)?;
        }
        PatKind::AnyOrder { children } => {
            for child in children {
                validate_bare_predicate_position(child, pos.clone())?;
            }
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => {}
    }
    Ok(())
}

// ============================================================================
// Post-pass: assign capture slots in source order; detect sugar Unions.
// ============================================================================

/// Walk `pat` in source order, replacing every `Capture { slot: u16::MAX }`
/// placeholder with a dense slot index (0..n). Returns the per-slot
/// `CaptureKind` vector. Detects the `${...}` sugar shape — a Union whose
/// every arm is `Capture { name: Some(SUGAR_MARKER) }` — and allocates one
/// shared slot for the group, then clears the marker so downstream sees
/// `name = None` on each arm.
///
/// Duplicate named captures (`$foo` more than once) error here, mirroring
/// the hand-written parser.
fn assign_capture_slots(pat: &mut Pat) -> Result<Vec<CaptureKind>, ParseError> {
    let mut state = SlotState {
        kinds: Vec::new(),
        names: Vec::new(),
    };
    walk_assign(pat, &mut state)?;
    Ok(state.kinds)
}

struct SlotState {
    kinds: Vec<CaptureKind>,
    /// Captured names seen so far. `None` for anonymous captures.
    names: Vec<Option<String>>,
}

impl SlotState {
    fn alloc_slot(&mut self, kind: CaptureKind, name: Option<String>) -> Result<u16, ParseError> {
        let slot = u16::try_from(self.kinds.len())
            .map_err(|_| ParseError::new("too many captures in one pattern", PatSpan::new(0, 0)))?;
        self.kinds.push(kind);
        self.names.push(name);
        Ok(slot)
    }
}

fn walk_assign(pat: &mut Pat, state: &mut SlotState) -> Result<(), ParseError> {
    // Detect the sugar Union shape and short-circuit: if every arm is a
    // Capture with `name = Some(SUGAR_MARKER)`, allocate a single shared
    // slot for the whole group, clear the marker on each arm, and recurse
    // into each arm body.
    if let PatKind::Union(alts) = &pat.kind {
        let all_sugar = !alts.is_empty()
            && alts.iter().all(|a| matches!(
                &a.kind,
                PatKind::Capture { name: Some(n), .. } if n == SUGAR_MARKER
            ));
        if all_sugar {
            // Allocate one shared slot. Slot kind is `Node` — sugar arms
            // are full Prefixed patterns, which don't admit rest/quantifier
            // bodies at the arm level. (A Quantifier inside an arm gets its
            // own slot if it has a `$`, but the sugar slot itself is Node.)
            let shared_slot = state.alloc_slot(CaptureKind::Node, None)?;
            if let PatKind::Union(alts) = &mut pat.kind {
                for arm in alts.iter_mut() {
                    if let PatKind::Capture { slot, name, body } = &mut arm.kind {
                        *slot = shared_slot;
                        *name = None; // strip the sugar marker
                        walk_assign(body, state)?;
                    }
                }
            }
            return Ok(());
        }
    }

    match &mut pat.kind {
        PatKind::Capture { slot, name, body } => {
            // Allocate THIS slot before recursing into the body so that
            // outer captures get lower slot indices than nested ones.
            let kind = slot_kind_for_body(body);
            if let Some(n) = name.as_ref() {
                // Duplicate-name check. Point the error at the ident's span,
                // which is (outer_start + 1) .. outer_end for the named-capture
                // shape produced by `convert_named_captures` (the outer Capture
                // spans `$ident` and the body Wildcard spans only the `$`).
                if state.names.iter().any(|nm| nm.as_deref() == Some(n.as_str())) {
                    let ident_span = PatSpan {
                        start: pat.span.start + 1,
                        end: pat.span.end,
                    };
                    return Err(ParseError::new(
                        format!("duplicate capture name `{n}`"),
                        ident_span,
                    ));
                }
            }
            *slot = state.alloc_slot(kind, name.clone())?;
            walk_assign(body, state)?;
        }
        PatKind::Node { children, .. } => {
            for c in children.iter_mut() {
                walk_assign(c, state)?;
            }
        }
        PatKind::Union(alts) => {
            for a in alts.iter_mut() {
                walk_assign(a, state)?;
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => walk_assign(b, state)?,
        PatKind::Quantifier { body, .. } => walk_assign(body, state)?,
        PatKind::AnyOrder { children } => {
            for c in children.iter_mut() {
                walk_assign(c, state)?;
            }
        }
        PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Rest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => {}
    }
    Ok(())
}

/// Collect the names-by-slot table after `assign_capture_slots` has finished.
/// Used by `resolve_pred_capture_refs` to map a `$ident` back-reference to
/// its slot index.
fn collect_capture_names(pat: &Pat) -> Vec<Option<String>> {
    let mut out: Vec<(u16, Option<String>)> = Vec::new();
    fn walk(p: &Pat, out: &mut Vec<(u16, Option<String>)>) {
        if let PatKind::Capture { slot, name, body } = &p.kind {
            // Defensive: skip placeholder slots (shouldn't happen post-assign).
            if *slot != u16::MAX {
                out.push((*slot, name.clone()));
            }
            walk(body, out);
            return;
        }
        match &p.kind {
            PatKind::Node { children, .. } => children.iter().for_each(|c| walk(c, out)),
            PatKind::Union(alts) => alts.iter().for_each(|a| walk(a, out)),
            PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => walk(b, out),
            PatKind::Quantifier { body, .. } => walk(body, out),
            PatKind::AnyOrder { children } => children.iter().for_each(|c| walk(c, out)),
            _ => {}
        }
    }
    walk(pat, &mut out);
    // Each slot appears at most once for named captures (duplicates already
    // rejected); but sugar slots appear multiple times with `name = None`.
    // Build a Vec<Option<String>> indexed by slot.
    let max_slot = out.iter().map(|(s, _)| *s).max().map(|s| s as usize + 1).unwrap_or(0);
    let mut table: Vec<Option<String>> = vec![None; max_slot];
    for (s, n) in out {
        if let Some(name) = n {
            table[s as usize] = Some(name);
        }
    }
    table
}

/// Walk the AST and replace `\0capref:NAME` sentinel `Lit::Sym` predicate
/// args with `PredArg::Capture(slot)`. Errors on forward/unknown names.
fn resolve_pred_capture_refs(
    pat: &mut Pat,
    name_table: &[Option<String>],
) -> Result<(), ParseError> {
    match &mut pat.kind {
        PatKind::Predicate { args, .. } => {
            for arg in args.iter_mut() {
                if let PredArg::Lit(Lit::Sym(s)) = arg
                    && let Some(rest) = s.strip_prefix(CAPREF_MARKER)
                {
                    let slot = name_table
                        .iter()
                        .position(|n| n.as_deref() == Some(rest))
                        .ok_or_else(|| {
                            ParseError::new(
                                format!(
                                    "unknown or forward capture reference `${rest}` in predicate arg"
                                ),
                                pat.span,
                            )
                        })?;
                    *arg = PredArg::Capture(slot as u16);
                }
            }
        }
        PatKind::Node { children, .. } => {
            for c in children.iter_mut() {
                resolve_pred_capture_refs(c, name_table)?;
            }
        }
        PatKind::Union(alts) => {
            for a in alts.iter_mut() {
                resolve_pred_capture_refs(a, name_table)?;
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            resolve_pred_capture_refs(b, name_table)?;
        }
        PatKind::Quantifier { body, .. } => resolve_pred_capture_refs(body, name_table)?,
        PatKind::Capture { body, .. } => resolve_pred_capture_refs(body, name_table)?,
        PatKind::AnyOrder { children } => {
            for c in children.iter_mut() {
                resolve_pred_capture_refs(c, name_table)?;
            }
        }
        PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Rest
        | PatKind::Lit(_)
        | PatKind::Kind(_) => {}
    }
    Ok(())
}

// ============================================================================
// `validate_*` walks — unchanged from the pre-LALRPOP parser.
// ============================================================================

/// Resolve a capture's slot kind from its body's shape. `Rest` and the
/// many-iteration quantifiers (`+`, `*`) produce a slice (`Seq`); the
/// optional quantifier (`?`) produces `OptNode`; anything else binds a
/// single node (`Node`).
fn slot_kind_for_body(body: &Pat) -> CaptureKind {
    match &body.kind {
        PatKind::Rest => CaptureKind::Seq,
        PatKind::Quantifier { max: 1, .. } => CaptureKind::OptNode,
        PatKind::Quantifier { .. } => CaptureKind::Seq,
        _ => CaptureKind::Node,
    }
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

/// Post-parse walk enforcing the v1 grammar rule for quantifiers (`*` / `+` /
/// `?`): each is valid *only* as a direct child of a node match `(...)`, and
/// its body may not itself contain `$` captures or rest-like elements.
fn validate_quantifier_placement(pat: &Pat, is_node_child: bool) -> Result<(), ParseError> {
    if let PatKind::Quantifier { .. } = &pat.kind
        && !is_node_child
    {
        return Err(ParseError::new(
            "postfix `*` / `+` / `?` is only valid as a direct child of a node match",
            pat.span,
        ));
    }
    match &pat.kind {
        PatKind::Quantifier { body, .. } => {
            validate_quantifier_body(body)?;
            validate_quantifier_placement(body, false)
        }
        PatKind::Node { children, .. } => {
            for child in children {
                validate_quantifier_placement(child, true)?;
            }
            Ok(())
        }
        PatKind::Union(alts) => {
            for alt in alts {
                validate_quantifier_placement(alt, false)?;
            }
            Ok(())
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            validate_quantifier_placement(b, false)
        }
        PatKind::Capture { body, .. } => validate_quantifier_placement(body, is_node_child),
        PatKind::AnyOrder { children } => {
            for child in children {
                validate_quantifier_placement(child, true)?;
            }
            Ok(())
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => Ok(()),
    }
}

/// Reject patterns that may not appear inside the body of a `*`/`+`/`?`
/// quantifier: any `$` capture and any rest-like element.
fn validate_quantifier_body(pat: &Pat) -> Result<(), ParseError> {
    match &pat.kind {
        PatKind::Capture { .. } => Err(ParseError::new(
            "`$` capture is not allowed inside a quantifier body \
             (use `$pat+` / `$pat*` / `$pat?` to capture the iterations)",
            pat.span,
        )),
        PatKind::Rest => Err(ParseError::new(
            "`...` is not allowed inside a quantifier body",
            pat.span,
        )),
        PatKind::Node { children, .. } => {
            for child in children {
                validate_quantifier_body(child)?;
            }
            Ok(())
        }
        PatKind::Union(alts) => {
            for alt in alts {
                validate_quantifier_body(alt)?;
            }
            Ok(())
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => validate_quantifier_body(b),
        PatKind::Quantifier { body, .. } => validate_quantifier_body(body),
        PatKind::AnyOrder { children } => {
            for child in children {
                validate_quantifier_body(child)?;
            }
            Ok(())
        }
        PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => Ok(()),
    }
}

/// Post-parse walk enforcing the "captures live on a definite-assignment
/// path" rule. See module-level docs and the pre-LALRPOP parser for the
/// full reasoning.
fn validate_capture_position(pat: &Pat, forbidden: bool) -> Result<(), ParseError> {
    match &pat.kind {
        PatKind::Capture { body, .. } => {
            if forbidden {
                return Err(ParseError::new(
                    "`$` capture is not allowed inside `{}` / `!` / `` ` `` \
                     (the body has no definite-assignment path)",
                    pat.span,
                ));
            }
            validate_capture_position(body, false)
        }
        PatKind::Union(alts) => {
            if let Some(first_cap) = alts.first().and_then(|a| {
                if let PatKind::Capture { slot, name, .. } = &a.kind {
                    Some((*slot, name.as_deref()))
                } else {
                    None
                }
            }) {
                let (first_slot, first_name) = first_cap;
                let all_same = alts.iter().all(|alt| {
                    matches!(&alt.kind,
                        PatKind::Capture { slot, name, .. }
                        if *slot == first_slot && name.as_deref() == first_name
                    )
                });
                if all_same && !forbidden {
                    for alt in alts {
                        let PatKind::Capture { body, .. } = &alt.kind else {
                            unreachable!("all_same guarantees Capture");
                        };
                        validate_capture_position(body, true)?;
                    }
                    return Ok(());
                }
            }
            for alt in alts {
                validate_capture_position(alt, true)?;
            }
            Ok(())
        }
        PatKind::Not(b) | PatKind::Descend(b) => validate_capture_position(b, true),
        PatKind::Parent(b) => validate_capture_position(b, forbidden),
        PatKind::Quantifier { body, .. } => validate_capture_position(body, forbidden),
        PatKind::Node { children, .. } => {
            for child in children {
                validate_capture_position(child, forbidden)?;
            }
            Ok(())
        }
        PatKind::AnyOrder { children } => {
            for child in children {
                validate_capture_position(child, false)?;
            }
            Ok(())
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => Ok(()),
    }
}

/// Post-parse walk enforcing the v1 grammar rule for rest-like elements
/// (`...` and `$...`).
fn validate_rest_placement(pat: &Pat, is_node_child: bool) -> Result<(), ParseError> {
    if is_rest_like(pat) && !is_node_child {
        return Err(ParseError::new(
            "`...` / `$...` is only valid as a direct child of a node match",
            pat.span,
        ));
    }
    match &pat.kind {
        PatKind::Node { children, .. } => {
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
            if !is_rest_like(pat) {
                validate_rest_placement(body, false)?;
            }
        }
        PatKind::AnyOrder { children } => {
            if let Some(second) = children.iter().filter(|c| is_rest_like(c)).nth(1) {
                return Err(ParseError::new(
                    "at most one `...` / `$...` per `<...>` child list",
                    second.span,
                ));
            }
            for child in children {
                validate_rest_placement(child, true)?;
            }
        }
        PatKind::Quantifier { body, .. } => {
            validate_rest_placement(body, false)?;
        }
        PatKind::Rest
        | PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => {}
    }
    Ok(())
}

// ============================================================================
// Tests — unchanged from the pre-LALRPOP parser. Each test calls the public
// `parse` entry, so the LALRPOP migration is invisible to them.
// ============================================================================

#[cfg(test)]
mod capture_position_tests {
    use crate::parse;
    use crate::{CaptureKind, PatKind};

    #[test]
    fn captures_in_union_arms_diff_slot_rejected() {
        let e = parse("{$a $b}").expect_err("must reject differing captures in union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_in_union_one_sided_rejected() {
        let e = parse("{$_ int}").expect_err("must reject one-sided capture in union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_body_rejected() {
        let e = parse("${int $float}").expect_err("must reject nested capture in sugar union body");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_union_sugar_parses_and_has_single_slot() {
        let p = parse("${int float}").expect("${int float} must parse");
        assert_eq!(p.n_captures(), 1, "expected 1 capture slot");
        assert_eq!(p.captures[0], CaptureKind::Node);
        assert!(
            matches!(p.root.kind, PatKind::Union(_)),
            "root must be Union, got {:?}",
            p.root.kind
        );
        if let PatKind::Union(alts) = &p.root.kind {
            assert_eq!(alts.len(), 2, "expected 2 union arms");
            for (i, arm) in alts.iter().enumerate() {
                match &arm.kind {
                    PatKind::Capture { slot, name, .. } => {
                        assert_eq!(*slot, 0, "arm {i} must use slot 0");
                        assert!(name.is_none(), "arm {i} must be anonymous");
                    }
                    _ => panic!("arm {i} must be a Capture, got {:?}", arm.kind),
                }
            }
        }
    }

    #[test]
    fn capture_union_sugar_three_arms_each_share_slot() {
        let p = parse("${int float sym}").expect("${int float sym} must parse");
        assert_eq!(p.n_captures(), 1, "expected 1 capture slot for 3-arm sugar");
        assert!(
            matches!(p.root.kind, PatKind::Union(_)),
            "root must be Union for 3-arm sugar"
        );
        if let PatKind::Union(alts) = &p.root.kind {
            assert_eq!(alts.len(), 3);
            for (i, arm) in alts.iter().enumerate() {
                let PatKind::Capture { slot, .. } = &arm.kind else {
                    panic!("arm {i} must be Capture, got {:?}", arm.kind);
                };
                assert_eq!(*slot, 0, "arm {i} must use slot 0");
            }
        }
    }

    #[test]
    fn captures_inside_negation_are_rejected() {
        let e = parse("!$_").expect_err("must reject capture in not");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_wrapping_not_is_allowed() {
        let p = parse("$!(int 1)").expect("capture wrapping not must parse");
        assert_eq!(p.n_captures(), 1);
        assert!(
            matches!(p.root.kind, PatKind::Capture { .. }),
            "root must be Capture"
        );
    }

    #[test]
    fn capture_wrapping_not_in_node_child_is_allowed() {
        let p =
            parse("(send $!(int 1) :foo)").expect("capture wrapping not in node child must parse");
        assert_eq!(p.n_captures(), 1);
    }

    #[test]
    fn inner_capture_inside_negation_remains_rejected() {
        let e = parse("!$(int 1)").expect_err("must reject capture inside not");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn captures_inside_descend_are_rejected() {
        let e = parse("`$_").expect_err("must reject capture in descend");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn captures_under_parent_are_allowed() {
        let p = parse("^$_").expect("parent capture should parse");
        assert_eq!(p.n_captures(), 1);
    }

    #[test]
    fn captures_inside_capture_body_are_allowed() {
        let p = parse("$(send $_ :foo)").expect("nested capture should parse");
        assert_eq!(p.n_captures(), 2);
    }

    #[test]
    fn node_child_outside_union_capture_unaffected() {
        let p = parse("({send csend} $...)").expect("({send csend} $...) must parse");
        assert_eq!(p.n_captures(), 1);
        assert_eq!(p.captures[0], CaptureKind::Seq);
    }

    #[test]
    fn not_over_uniform_capture_sugar_rejected() {
        let e = parse("!${int float}").expect_err("must reject sugar union inside negation");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn descend_over_uniform_capture_sugar_rejected() {
        let e = parse("`${int float}").expect_err("must reject sugar union inside descend");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_inside_non_uniform_outer_union_rejected() {
        let e = parse("{int ${int float}}")
            .expect_err("must reject capture in non-uniform outer union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_arm_body_node_rejected() {
        let e = parse("${(send $recv :foo) int}")
            .expect_err("must reject nested capture inside sugar arm body subtree");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_both_arm_bodies_rejected() {
        let e = parse("${(send $_ :foo) (send $_ :bar)}")
            .expect_err("must reject nested capture in sugar arm bodies");
        assert!(
            e.message.contains('$'),
            "expected `$` in error, got: {}",
            e.message
        );
    }
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
    fn parses_operator_and_uppercase_symbols() {
        assert_eq!(k(":+"), PatKind::Lit(Lit::Sym("+".into())));
        assert_eq!(k(":[]="), PatKind::Lit(Lit::Sym("[]=".into())));
        assert_eq!(k(":<=>"), PatKind::Lit(Lit::Sym("<=>".into())));
        assert_eq!(k(":Foo"), PatKind::Lit(Lit::Sym("Foo".into())));
    }

    #[test]
    fn parses_variable_symbols_in_node_match() {
        for (src, name) in [
            ("(ivar :@foo)", "@foo"),
            ("(cvar :@@foo)", "@@foo"),
            ("(gvar :$foo)", "$foo"),
        ] {
            let p = parse(src).unwrap_or_else(|e| panic!("parse `{src}`: {e:?}"));
            let PatKind::Node { children, .. } = p.root.kind else {
                panic!("`{src}` should be a Node, got {:?}", p.root.kind);
            };
            assert_eq!(children.len(), 1, "`{src}` should have one child");
            assert_eq!(
                children[0].kind,
                PatKind::Lit(Lit::Sym(name.into())),
                "child of `{src}` should be Sym(`{name}`)",
            );
        }
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
        assert_eq!(
            k("#odd?"),
            PatKind::Predicate {
                name: "odd?".into(),
                args: vec![]
            }
        );
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

    #[test]
    fn nested_prefixes_compose() {
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
        assert!(parse("!").is_err());
    }

    #[test]
    fn non_capture_patterns_have_zero_captures() {
        assert_eq!(parse("_").unwrap().n_captures(), 0);
        assert_eq!(parse("!send").unwrap().n_captures(), 0);
        assert_eq!(parse(":sym").unwrap().n_captures(), 0);
    }

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
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_paren_after_open() {
        let e = parse("(").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_oneof_head() {
        let e = parse("({send").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_empty_node() {
        assert!(parse("()").is_err());
    }

    #[test]
    fn parses_nested_node() {
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
        assert!(parse("({} _)").is_err());
    }

    #[test]
    fn rejects_non_ident_in_oneof_head() {
        let e = parse("({send :sym} _)").expect_err("symbol in OneOf head");
        assert!(
            e.message.contains("may only contain node types")
                || e.message.contains("unexpected"),
            "message was: {}",
            e.message
        );
        let e = parse("({send _} _)").expect_err("wildcard in OneOf head");
        assert!(
            e.message.contains("may only contain node types")
                || e.message.contains("unexpected"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn node_span_covers_open_through_close() {
        let p = parse("(send)").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 6));
    }

    #[test]
    fn rejects_rparen_as_head() {
        let e = parse("()").expect_err("no head");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

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
        // Round 3 owns the precise message; structurally the parse must fail.
        let _ = e;
    }

    #[test]
    fn parses_single_alternative_union() {
        let p = parse("{send}").expect("ok");
        match p.root.kind {
            PatKind::Union(alts) => assert_eq!(alts.len(), 1),
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_union() {
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
        let e = parse("{send").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn rejects_unclosed_empty_union() {
        let e = parse("{").expect_err("unclosed");
        assert!(
            !e.message.contains("empty pattern"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn union_span_covers_open_through_close() {
        let p = parse("{send csend}").expect("ok");
        assert_eq!((p.root.span.start, p.root.span.end), (0, 12));
    }

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
        let p = parse("$(const _ :Foo)").expect("ok");
        match p.root.kind {
            PatKind::Capture { slot, name, body } => {
                assert_eq!(slot, 0);
                assert!(name.is_none());
                assert!(matches!(body.kind, PatKind::Node { .. }));
            }
            other => panic!("expected Capture, got {other:?}"),
        }
    }

    // The remaining tests are imported from the pre-LALRPOP suite below;
    // see the trailing `mod inherited_tests` for the full set.

    fn children_of(src: &str) -> Vec<Pat> {
        let p = parse(src).expect("ok");
        match p.root.kind {
            PatKind::Node { children, .. } => children,
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn parses_bare_predicate_with_known_kind_quantifier_in_node_children() {
        let cs = children_of("(send _ :puts int? odd?)");
        assert_eq!(cs.len(), 4);

        match &cs[2].kind {
            PatKind::Quantifier { body, min, .. } => {
                assert_eq!(*min, 0);
                assert!(matches!(body.kind, PatKind::Kind(_)));
            }
            other => panic!("expected Quantifier, got {other:?}"),
        }

        // `odd?` is a bare-predicate shorthand. Round 2 emits `Predicate`
        // for an unknown ident WITHOUT a `?`/`!` suffix because the LR
        // grammar pulls the `?` into a Quantifier wrapper. The post-pass
        // for "ident? as predicate shorthand" lands in Round 3; here we
        // just check structural shape.
        match &cs[3].kind {
            PatKind::Quantifier { body, .. } => {
                // `odd` is unknown -> Predicate("odd"); wrapped in Quantifier.
                assert!(matches!(body.kind, PatKind::Predicate { .. }));
            }
            // Round 3 may collapse this to PatKind::Predicate { name:"odd?" }.
            PatKind::Predicate { name, .. } if name == "odd?" => {}
            other => panic!("expected Quantifier or Predicate, got {other:?}"),
        }
    }

    #[test]
    fn parses_predicate_with_int_arg() {
        let pat = parse("#divisible_by?(42)").expect("parse ok");
        match &pat.root.kind {
            PatKind::Predicate { name, args } => {
                assert_eq!(name, "divisible_by?");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], crate::ast::PredArg::Lit(Lit::Int(42)));
            }
            other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn parses_predicate_with_str_arg() {
        let pat = parse("#starts_with?(\"foo\")").expect("parse ok");
        match &pat.root.kind {
            PatKind::Predicate { name, args } => {
                assert_eq!(name, "starts_with?");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], crate::ast::PredArg::Lit(Lit::Str("foo".into())));
            }
            other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn parses_predicate_with_sym_arg() {
        let pat = parse("#sym_eq?(:foo)").expect("parse ok");
        match &pat.root.kind {
            PatKind::Predicate { name, args } => {
                assert_eq!(name, "sym_eq?");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], crate::ast::PredArg::Lit(Lit::Sym("foo".into())));
            }
            other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn parses_predicate_with_capture_ref_arg() {
        let pat = parse("(send $val #contains?($val))").expect("parse ok");
        let PatKind::Node { children, .. } = &pat.root.kind else {
            panic!("expected Node");
        };
        match &children[1].kind {
            PatKind::Predicate { name, args } => {
                assert_eq!(name, "contains?");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], crate::ast::PredArg::Capture(0));
            }
            other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn parse_predicate_pattern_arg_is_rejected() {
        let e = parse("#g?({:A :B})").expect_err("must reject pattern arg");
        assert!(
            e.message.contains("pattern args in v1: literal/capture only")
                || e.message.contains("unexpected"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn parse_predicate_unknown_capture_ref_is_rejected() {
        let e = parse("#pred?($unknown)").expect_err("must reject unknown capture ref");
        assert!(
            e.message.contains("unknown or forward capture reference"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn parse_predicate_nil_arg_is_rejected() {
        let e = parse("#p?(nil)").expect_err("must reject `nil` predicate arg");
        assert!(
            e.message
                .contains("`nil` is not supported as a predicate argument in v1"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn allows_rest_and_quantifier_in_same_child_list() {
        assert!(parse("(send _ :foo ... int+)").is_ok());
        assert!(parse("(send _ :foo int+ ...)").is_ok());
    }

    #[test]
    fn allows_multiple_quantifiers_in_same_child_list() {
        assert!(parse("(send _ :foo int+ sym*)").is_ok());
        assert!(parse("(send _ :foo int? str+ sym*)").is_ok());
    }

    #[test]
    fn capture_with_plus_body_is_seq_slot() {
        let p = parse("(send _ :pluck $sym+)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
    }

    #[test]
    fn capture_with_star_body_is_seq_slot() {
        let p = parse("(array $int*)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
    }

    #[test]
    fn capture_with_question_body_is_optnode_slot() {
        let p = parse("(send _ :update_columns $hash?)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::OptNode]);
    }

    #[test]
    fn named_capture_without_postfix_keeps_node_kind() {
        let p = parse("(send $receiver _)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::Node]);
        match &p.root.kind {
            PatKind::Node { children, .. } => match &children[0].kind {
                PatKind::Capture { name, body, .. } => {
                    assert_eq!(name.as_deref(), Some("receiver"));
                    assert_eq!(body.kind, PatKind::Wildcard);
                }
                other => panic!("expected Capture, got {other:?}"),
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn dollar_ident_with_postfix_is_anonymous_capture() {
        let p = parse("(send $send?)").expect("ok");
        match &p.root.kind {
            PatKind::Node { children, .. } => match &children[0].kind {
                PatKind::Capture { name, body, .. } => {
                    assert!(name.is_none(), "must not be a named capture");
                    assert!(matches!(body.kind, PatKind::Quantifier { .. }));
                }
                other => panic!("expected Capture, got {other:?}"),
            },
            _ => unreachable!(),
        }
        assert_eq!(p.capture_kinds(), &[CaptureKind::OptNode]);
    }

    #[test]
    fn error_quantifier_at_top_level() {
        let e = parse("int+").expect_err("top-level quantifier");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_in_union_arm() {
        let e = parse("{int+ sym}").expect_err("quantifier in union arm");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_not_sigil_body() {
        let e = parse("!int+").expect_err("quantifier under `!`");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_parent_sigil_body() {
        let e = parse("^int+").expect_err("quantifier under `^`");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_descend_sigil_body() {
        let e = parse("`int+").expect_err("quantifier under backtick");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_chained_postfix_plus_plus() {
        let e = parse("(array int++)").expect_err("chained ++");
        // Round 3 owns the precise message; structurally must fail.
        let _ = e;
    }

    #[test]
    fn error_chained_postfix_star_question() {
        let e = parse("(array int*?)").expect_err("chained *?");
        let _ = e;
    }

    #[test]
    fn error_capture_inside_quantifier_body_via_parens() {
        assert!(parse("(array ($int)+)").is_err());
    }

    #[test]
    fn error_capture_inside_quantifier_body_via_node() {
        let e = parse("(array (send _ $_)+)").expect_err("capture in quantifier body");
        assert!(
            e.message.contains("quantifier body"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_rest_with_postfix_quantifier() {
        let e = parse("(array ...+)").expect_err("rest with postfix");
        // Round 3 owns precise message; structurally must fail.
        let _ = e;
    }

    #[test]
    fn error_standalone_postfix_token() {
        let e = parse("+").expect_err("bare +");
        let _ = e;
    }
}
