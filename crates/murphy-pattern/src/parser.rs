//! Recursive-descent parser: token stream -> `PatternAst`.
//!
//! Task 5 (murphy-9cr.17) implements the atom/prefix skeleton: `_`,
//! literals, `nil?`, bare kind names, `#predicate`, and the `!`/`^`/backtick
//! prefixes. Task 6 adds node match `(head child*)` with `Exact`/`Any`/`OneOf`
//! heads. Task 7 adds union `{a b ...}`. Task 8 implements `$` captures —
//! named (`$ident`), anonymous (`$<pattern>`), and seq (`$...`) forms.

use crate::ast::PredArg;
use crate::lexer::{Spanned, Token, tokenize};
use crate::schema::node_child_allows_bare_predicate;
use crate::{CaptureKind, Head, Lit, ParseError, Pat, PatKind, PatSpan, PatternAst};
use murphy_ast::NodeKindTag;

/// Parse a pattern source string into a [`PatternAst`].
///
/// Tokenizes `src`, parses exactly one top-level pattern, and requires the
/// token stream to be fully consumed — leftover tokens are an error.
pub fn parse(src: &str) -> Result<PatternAst, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser::new(&tokens);
    let root = parser.prefixed(false)?;
    if let Some(extra) = parser.peek() {
        return Err(ParseError::new("unexpected trailing input", extra.span));
    }
    // The root is not a node child, so a rest-like root is correctly rejected.
    validate_rest_placement(&root, false)?;
    // Every `$` capture must sit on a definite-assignment path so it is
    // written on exactly the successful arm; `{}` / `!` / `` ` `` violate
    // this. Matches the B-backend `lower_bool` rejection (murphy-9cr.18).
    validate_capture_position(&root, false)?;
    // A postfix `*` / `+` / `?` quantifier is only valid as a direct child
    // of a node match (`(head c1 c2 ...)`); anywhere else the matcher has
    // no list to iterate. The body of a quantifier is itself constrained:
    // captures and rest-like elements are not allowed inside.
    validate_quantifier_placement(&root, false)?;
    Ok(PatternAst {
        root,
        captures: parser.captures,
    })
}

#[cfg(test)]
mod capture_position_tests {
    use crate::parse;
    use crate::{CaptureKind, PatKind};

    #[test]
    fn captures_in_union_arms_diff_slot_rejected() {
        // `{$a $b}` — arms have different capture names (different slots);
        // the parser must reject this because the losing arm's slot would
        // be unwritten at the matcher's `finish` step.
        let e = parse("{$a $b}").expect_err("must reject differing captures in union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_in_union_one_sided_rejected() {
        // `{$_ int}` — only one arm has a capture; must be rejected.
        let e = parse("{$_ int}").expect_err("must reject one-sided capture in union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_body_rejected() {
        // `${int $float}` — the sugar produces a union where one arm's
        // body is itself a `$` capture (the body of `$float`). Must be rejected.
        let e = parse("${int $float}").expect_err("must reject nested capture in sugar union body");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_union_sugar_parses_and_has_single_slot() {
        // `${int float}` — the sugar `${ ... }` produces exactly one slot
        // shared across all arms. The result is a Union of Captures
        // (each arm Capture{slot:0, body:Kind(...)}).
        let p = parse("${int float}").expect("${int float} must parse");
        // Exactly one capture slot (slot 0) of kind Node.
        assert_eq!(p.n_captures(), 1, "expected 1 capture slot");
        assert_eq!(p.captures[0], CaptureKind::Node);
        // Root node must be a Union.
        assert!(
            matches!(p.root.kind, PatKind::Union(_)),
            "root must be Union, got {:?}",
            p.root.kind
        );
        // Both arms must be Capture with slot 0.
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
        // `${int float sym}` — 3-arm sugar also uses a single slot 0.
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
    fn captures_inside_descend_are_rejected() {
        let e = parse("`$_").expect_err("must reject capture in descend");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn captures_under_parent_are_allowed() {
        // `^x` is definite — the parent direction is unique. Captures are
        // OK in that subtree (mirrors B's `lower_pat` route).
        let p = parse("^$_").expect("parent capture should parse");
        assert_eq!(p.n_captures(), 1);
    }

    #[test]
    fn captures_inside_capture_body_are_allowed() {
        // The body of an outer `$(...)` is a definite-assignment subtree.
        let p = parse("$(send $_ :foo)").expect("nested capture should parse");
        assert_eq!(p.n_captures(), 2);
    }

    #[test]
    fn node_child_outside_union_capture_unaffected() {
        // `({send csend} $...)` — the `$...` is outside the union, in the
        // node's child list. This must still parse without error.
        let p = parse("({send csend} $...)").expect("({send csend} $...) must parse");
        assert_eq!(p.n_captures(), 1);
        assert_eq!(p.captures[0], CaptureKind::Seq);
    }

    #[test]
    fn not_over_uniform_capture_sugar_rejected() {
        // `!${int float}` — the sugar union is inside a `!` negation, which
        // is a forbidden position. Even though all arms share the same slot,
        // the outer `!` means assignment is not guaranteed on the happy path.
        let e = parse("!${int float}").expect_err("must reject sugar union inside negation");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn descend_over_uniform_capture_sugar_rejected() {
        // `` `${int float} `` — same reasoning as the `!` case: the descend
        // prefix makes the inner union sit on a non-definite-assignment path.
        let e = parse("`${int float}").expect_err("must reject sugar union inside descend");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_inside_non_uniform_outer_union_rejected() {
        // `{int ${int float}}` — outer union has one arm without a capture
        // (`int`) and one arm that is uniform-capture sugar (`${int float}`).
        // This is not a uniform union at the outer level, so the inner sugar
        // is walked with forbidden=true and the capture inside it is rejected.
        let e = parse("{int ${int float}}")
            .expect_err("must reject capture in non-uniform outer union");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_arm_body_node_rejected() {
        // `${(send $recv :foo) int}` — uniform sugar where one arm's body is
        // a Node containing a nested `$recv` capture. If the `int` arm wins,
        // `$recv`'s slot is never written. The validator must reject this.
        let e = parse("${(send $recv :foo) int}")
            .expect_err("must reject nested capture inside sugar arm body subtree");
        assert!(e.message.contains('$'));
    }

    #[test]
    fn capture_sugar_nested_capture_in_both_arm_bodies_rejected() {
        // `${(send $_ :foo) (send $_ :bar)}` — both arms contain an
        // anonymous `$_` nested inside the arm body's Node. Even though
        // every arm carries a capture, each is in a *different* slot (1
        // and 2), so the losing arm's slot is unwritten. The validator
        // is conservative: nested captures inside sugar arm bodies are
        // forbidden regardless of whether all arms happen to have them.
        let e = parse("${(send $_ :foo) (send $_ :bar)}")
            .expect_err("must reject nested capture in sugar arm bodies");
        assert!(
            e.message.contains('$'),
            "expected `$` in error, got: {}",
            e.message
        );
    }
}

/// Resolve a capture's slot kind from its body's shape. `Rest` and the
/// many-iteration quantifiers (`+`, `*`) produce a slice (`Seq`); the
/// optional quantifier (`?`) produces `OptNode`; anything else binds a
/// single node (`Node`).
fn slot_kind_for_body(body: &Pat) -> CaptureKind {
    match &body.kind {
        PatKind::Rest => CaptureKind::Seq,
        // A `?` quantifier has `max == 1`; `+` / `*` have `max == u8::MAX`.
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
///
/// The error message names which rule was broken so the user can correct the
/// pattern without guessing. Mirrors [`validate_rest_placement`].
///
/// `is_node_child` is `true` only when `pat` is being visited as a direct
/// child of a [`PatKind::Node`]; the root, union alternatives, and the bodies
/// of `!`/`^`/`` ` ``/`$` are all *not* node children.
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
            // The body of a quantifier is itself not a node child, and it
            // must not contain captures or rest-like elements (those would
            // make the per-iteration semantics undefined — what slot does
            // each iteration write into? where does `...` end?).
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
        // A `$pat+` capture sits *as* a node child, so its quantifier body
        // is allowed at the same position the capture itself is allowed —
        // propagate `is_node_child` rather than reset it.
        PatKind::Capture { body, .. } => validate_quantifier_placement(body, is_node_child),
        PatKind::AnyOrder { children } => {
            // AnyOrder children behave like node children for quantifier purposes.
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
/// quantifier: any `$` capture (the per-iteration write would be ambiguous)
/// and any rest-like element (chaining `...` with `*`/`+`/`?` would create
/// an undefined match shape).
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
/// path" rule. A `$` capture must always be written by the matcher's
/// successful arm — if it could be missed, the runtime would surface an
/// unwritten slot.
///
/// The forbidden positions are exactly those the B-backend's `lower_bool`
/// route rejects at compile time: `{}` union, `!` negation, `` ` ``
/// descend. `^` parent is fine — it has a unique parent. The body of an
/// outer capture, the body of a node-pattern's slots, and the top level
/// are all definite-assignment positions and recurse with `forbidden =
/// false`.
///
/// One exception to the union rule: `${alt1 alt2 ...}` sugar desugars to a
/// `Union` whose every arm is `Capture{slot:S, body:b}` with a shared slot S.
/// This is safe because every arm writes slot S, so the winning arm's write
/// always happens. Validation accepts this form and recurses into each body
/// with `forbidden = false` (the body itself is a definite-assignment point
/// inside that arm).
///
/// `forbidden` is `true` only while traversing the subtree of a union /
/// not / descend node. A `Capture` reached with `forbidden = true` is the
/// error case, unless it is inside a uniform-capture union as above.
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
            // The body of a capture is itself a definite-assignment subtree.
            validate_capture_position(body, false)
        }
        PatKind::Union(alts) => {
            // If every arm is a Capture sharing the same (slot, name) —
            // i.e. this is a desugared `${alt1 alt2 ...}` — then the union
            // is safe: whichever arm wins will write slot S. Validate each
            // arm's body normally (forbidden = false) and accept.
            //
            // Any other arrangement (only some arms capture, or arms use
            // different slots/names) is rejected because the losing arm's
            // slot would be unwritten.
            if let Some(first_cap) = alts.first().and_then(|a| {
                if let PatKind::Capture { slot, name, .. } = &a.kind {
                    Some((*slot, name.as_deref()))
                } else {
                    None
                }
            }) {
                let (first_slot, first_name) = first_cap;
                // Check: ALL arms are Capture with the same (slot, name).
                let all_same = alts.iter().all(|alt| {
                    matches!(&alt.kind,
                        PatKind::Capture { slot, name, .. }
                        if *slot == first_slot && name.as_deref() == first_name
                    )
                });
                if all_same && !forbidden {
                    // Validate each arm's body with `forbidden = true`. Only
                    // the outer sugar slot is guaranteed to be written by the
                    // winning arm — any *nested* `$` capture inside an arm
                    // body would have its slot unwritten on the losing arms.
                    // The recursive call rejects:
                    //   - bare Capture body, e.g. `${int $float}`
                    //   - Capture inside Node body, e.g. `${(send $recv :foo) int}`
                    //   - Capture inside any other forbidden-propagating shape.
                    for alt in alts {
                        let PatKind::Capture { body, .. } = &alt.kind else {
                            unreachable!("all_same guarantees Capture");
                        };
                        validate_capture_position(body, true)?;
                    }
                    return Ok(());
                }
                // Not all arms match — fall through to individual rejection.
            }
            // Either some arms lack Capture, or arms use different slots/names.
            // Walk each arm with forbidden=true so the first Capture found
            // produces the right error.
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
        // AnyOrder children are definite-assignment (every permutation writes
        // all non-rest slots), so recurse with forbidden=false.
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
        PatKind::AnyOrder { children } => {
            // AnyOrder: at most one rest-like child, recurse with is_node_child=true.
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
            // The body of a quantifier is not a node child, so a rest-like
            // body would have been (and still is) rejected by the recursive
            // call. `validate_quantifier_body` is the single source of truth
            // for the stricter "no rest inside quantifier" message.
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
    /// | '$' capture-tail | postfixed`.
    ///
    /// `$` is a prefix dispatched here (see [`Parser::capture`]), not in
    /// [`Parser::primary`]. Postfix quantifiers wrap the [`primary`] that
    /// follows a chain of `!`/`^`/`` ` `` prefixes, NOT the prefixes
    /// themselves — `!int+` parses as `Not(Quantifier(int, +))` so that the
    /// downstream `validate_quantifier_placement` walk can flag the
    /// node-child-only rule on the quantifier subtree (its parent is `!`).
    /// `allow_bare_predicate` is true when this identifier position is a node-child
    /// slot where an unknown bare identifier may be parsed as predicate shorthand
    /// (`foo?` / `foo!`).
    fn prefixed(&mut self, allow_bare_predicate: bool) -> Result<Pat, ParseError> {
        let Some(head) = self.peek() else {
            // No source byte to point at for an empty stream.
            return Err(ParseError::new("empty pattern", PatSpan::new(0, 0)));
        };
        let prefix_span = head.span;
        let wrap: fn(Box<Pat>) -> PatKind = match head.tok {
            Token::Bang => PatKind::Not,
            Token::Caret => PatKind::Parent,
            Token::Backtick => PatKind::Descend,
            Token::Dollar => return self.capture(prefix_span, allow_bare_predicate),
            _ => return self.postfixed(allow_bare_predicate),
        };
        self.pos += 1; // consume the prefix sigil
        let inner = self.prefixed(allow_bare_predicate)?;
        let span = PatSpan {
            start: prefix_span.start,
            end: inner.span.end,
        };
        Ok(Pat {
            kind: wrap(Box::new(inner)),
            span,
        })
    }

    /// `postfixed := primary quantifier?`.
    ///
    /// Reads a [`primary`](Self::primary) and, if the next token is a
    /// postfix quantifier (`*` / `+` / `?`), wraps it in a
    /// [`PatKind::Quantifier`]. Chained postfixes (`int++`, `int*?`) are
    /// rejected here — exactly one quantifier per primary.
    ///
    /// Placement (only valid as a node child) and body restrictions (no
    /// captures or rest inside the body) are enforced post-parse by
    /// [`validate_quantifier_placement`] / [`validate_quantifier_body`], so
    /// every quantifier-bearing source position lands the same error.
    fn postfixed(&mut self, allow_bare_predicate: bool) -> Result<Pat, ParseError> {
        let primary = self.primary(allow_bare_predicate)?;
        let Some((min, max, q_span)) = self.try_quantifier() else {
            return Ok(primary);
        };
        // Reject a *second* quantifier immediately after (`int++`, `int*?`).
        if let Some((_, _, dup_span)) = self.try_quantifier() {
            return Err(ParseError::new(
                "postfix `*` / `+` / `?` cannot be chained — apply at most one quantifier per pattern",
                dup_span,
            ));
        }
        let span = PatSpan {
            start: primary.span.start,
            end: q_span.end,
        };
        Ok(Pat {
            kind: PatKind::Quantifier {
                body: Box::new(primary),
                min,
                max,
            },
            span,
        })
    }

    /// If the cursor is at a postfix quantifier token, consume it and return
    /// `(min, max, span)`; otherwise leave the cursor unmoved.
    fn try_quantifier(&mut self) -> Option<(u8, u8, PatSpan)> {
        let next = self.peek()?;
        let (min, max) = match next.tok {
            Token::Star => (0, u8::MAX),
            Token::Plus => (1, u8::MAX),
            Token::Question => (0, 1),
            _ => return None,
        };
        let span = next.span;
        self.pos += 1;
        Some((min, max, span))
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
    fn capture(
        &mut self,
        dollar_span: PatSpan,
        allow_bare_predicate: bool,
    ) -> Result<Pat, ParseError> {
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
            Token::Ident(_) => {
                // `$ident` is ambiguous on its own: it could be a named
                // capture (`$receiver`) or an anonymous capture whose body is
                // a kind name + quantifier (`$int+`). Look one token past the
                // ident to decide. The `postfixed()` route handles both the
                // bare kind case (`$int`) and the quantifier case (`$int+`)
                // uniformly — and validation later rejects `$int` at
                // positions where a bare-kind body is meaningless.
                let lookahead = self.tokens.get(self.pos + 1).map(|s| &s.tok);
                if matches!(lookahead, Some(Token::Star | Token::Plus | Token::Question)) {
                    let body = self.postfixed(allow_bare_predicate)?;
                    let end = body.span.end;
                    (None, body, end)
                } else {
                    let Token::Ident(ident) = &next.tok else {
                        unreachable!("matched outer Token::Ident");
                    };
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
                    // The implicit Wildcard body is synthetic; per spec it
                    // spans the `$` token, not the identifier.
                    let body = Pat {
                        kind: PatKind::Wildcard,
                        span: dollar_span,
                    };
                    (Some(name), body, ident_span.end)
                }
            }
            Token::Ellipsis => {
                let ell_span = next.span;
                self.pos += 1; // consume `...`
                let body = Pat {
                    kind: PatKind::Rest,
                    span: ell_span,
                };
                (None, body, ell_span.end)
            }
            Token::LBrace => {
                // Sugar form `${ alt1 alt2 ... }`:
                // `$` immediately before `{` means "capture whichever union
                // arm matches, into slot `S`". This desugars at parse time
                // to `{ Capture{slot:S, body:alt1} Capture{slot:S, body:alt2}
                // ... }` — a Union whose every arm carries the same Capture
                // slot. The capture kind is always `Node` (arms may not be
                // rest-like or quantified inside the sugar).
                //
                // Slot S is already reserved above as `CaptureKind::Node`
                // (the kind appropriate for a per-arm node capture). Do NOT
                // push another entry — reuse `slot` for every arm.
                let open_span = next.span;
                self.pos += 1; // consume `{`
                let mut alts: Vec<Pat> = Vec::new();
                loop {
                    let Some(tok) = self.peek() else {
                        return Err(ParseError::new("unclosed `{`: expected `}`", open_span));
                    };
                    match tok.tok {
                        Token::RBrace => {
                            let close_span = tok.span;
                            self.pos += 1; // consume `}`
                            let union_span = PatSpan {
                                start: open_span.start,
                                end: close_span.end,
                            };
                            if alts.is_empty() {
                                return Err(ParseError::new(
                                    "empty union `{}` needs at least one alternative",
                                    union_span,
                                ));
                            }
                            // Build `Union[Capture{slot, body:arm} ...]` where
                            // each arm is already wrapped. The outer span covers
                            // `$` through the closing `}`.
                            let outer_span = PatSpan {
                                start: dollar_span.start,
                                end: close_span.end,
                            };
                            // `captures[slot]` was set to `Node` at reservation
                            // — no kind upgrade needed (arms cannot be Rest or
                            // Quantifier inside the sugar body).
                            return Ok(Pat {
                                kind: PatKind::Union(alts),
                                span: outer_span,
                            });
                        }
                        _ => {
                            // Parse one arm body (not `prefixed` — the arm does
                            // not get its own `$`, `!`, etc. prefix). We call
                            // `prefixed` here so that arms may be node patterns
                            // or unions themselves, but we then wrap the result
                            // in a Capture for slot S.
                            let arm_body = self.prefixed(allow_bare_predicate)?;
                            let arm_span = PatSpan {
                                start: dollar_span.start,
                                end: arm_body.span.end,
                            };
                            alts.push(Pat {
                                kind: PatKind::Capture {
                                    slot,
                                    name: None,
                                    body: Box::new(arm_body),
                                },
                                span: arm_span,
                            });
                        }
                    }
                }
            }
            _ => {
                let body = self.prefixed(allow_bare_predicate)?;
                let end = body.span.end;
                (None, body, end)
            }
        };

        // Upgrade the slot kind based on the body's shape: `$...` and
        // `$pat+` / `$pat*` produce a slice (`Seq`), `$pat?` produces an
        // optional single node (`OptNode`), everything else captures one
        // node (`Node`, the placeholder set at slot reservation).
        self.captures[slot as usize] = slot_kind_for_body(&body);

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
    fn primary(&mut self, allow_bare_predicate: bool) -> Result<Pat, ParseError> {
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
            Token::Predicate(name) => {
                let name = name.clone();
                // Peek to see if predicate args follow: `#name(arg1 arg2 ...)`.
                let args = if matches!(self.peek().map(|t| &t.tok), Some(Token::LParen)) {
                    let lparen_span = self.peek().unwrap().span;
                    self.pos += 1; // consume `(`
                    self.predicate_args(lparen_span)?
                } else {
                    vec![]
                };
                PatKind::Predicate { name, args }
            }
            Token::Ident(name) => return self.ident_pat(name, span, allow_bare_predicate),
            Token::LParen => return self.node_match(span),
            Token::LBrace => return self.union(span, allow_bare_predicate),
            Token::Ellipsis => {
                return Err(ParseError::new(
                    "`...` is only valid inside a node child list",
                    span,
                ));
            }
            Token::RParen => return Err(ParseError::new("unexpected `)`", span)),
            Token::RBrace => return Err(ParseError::new("unexpected `}`", span)),
            Token::Star | Token::Plus | Token::Question => {
                return Err(ParseError::new(
                    "postfix `*` / `+` / `?` must follow a pattern",
                    span,
                ));
            }
            Token::LAngle => {
                return Err(ParseError::new(
                    "`<...>` any-order sequence is only valid as a direct child of a node match",
                    span,
                ));
            }
            Token::RAngle => return Err(ParseError::new("unexpected `>`", span)),
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
        let mut child_idx = 0;
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
                    // `...` is itself a 0+ rest, so chaining `*`/`+`/`?` on
                    // it is meaningless and an error.
                    if let Some((_, _, q_span)) = self.try_quantifier() {
                        return Err(ParseError::new(
                            "`...` cannot take a postfix `*` / `+` / `?` quantifier",
                            q_span,
                        ));
                    }
                    children.push(Pat {
                        kind: PatKind::Rest,
                        span: ell_span,
                    });
                    child_idx += 1;
                }
                Token::LAngle => {
                    let open_span = tok.span;
                    self.pos += 1; // consume `<`
                    children.push(self.any_order(open_span)?);
                    child_idx += 1;
                }
                _ => {
                    let allow_bare_predicate = match &head {
                        Head::Exact(tag) => node_child_allows_bare_predicate(*tag, child_idx),
                        Head::Any => false,
                        Head::OneOf(tags) => tags
                            .iter()
                            .all(|tag| node_child_allows_bare_predicate(*tag, child_idx)),
                    };
                    children.push(self.prefixed(allow_bare_predicate)?);
                    child_idx += 1;
                }
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
    ///
    /// `allow_bare_predicate` flows through to each alternative so that
    /// `(send _ :puts {odd? even?})` accepts bare predicate shorthand in
    /// every alt — the union sits in a node-child slot, so each alternative
    /// is at the same effective position as if it had been written without
    /// the `{...}` wrapper.
    fn union(&mut self, open_span: PatSpan, allow_bare_predicate: bool) -> Result<Pat, ParseError> {
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
                _ => alts.push(self.prefixed(allow_bare_predicate)?),
            }
        }
    }

    /// `any_order := '<' child+ '>'`
    ///
    /// `open_span` is the span of the already-consumed `<`. Children are parsed
    /// with [`prefixed`]; `...` is converted to [`PatKind::Rest`] as in
    /// `node_match`. The resulting `Pat`'s span covers `<` through the closing
    /// `>`. Rules enforced here:
    ///   - at least one child (empty `<>` → error)
    ///   - at most 10 non-rest children (v1 limit)
    ///   - at most one `...` (duplicate rest → error; position validated later)
    fn any_order(&mut self, open_span: PatSpan) -> Result<Pat, ParseError> {
        let mut children: Vec<Pat> = Vec::new();
        loop {
            let Some(tok) = self.peek() else {
                return Err(ParseError::new("unclosed `<`: expected `>`", open_span));
            };
            match tok.tok {
                Token::RAngle => {
                    let close_span = tok.span;
                    self.pos += 1; // consume `>`
                    let span = PatSpan {
                        start: open_span.start,
                        end: close_span.end,
                    };
                    if children.is_empty() {
                        return Err(ParseError::new(
                            "empty `<>` needs at least one child pattern",
                            span,
                        ));
                    }
                    // Count non-rest children and check the v1 limit.
                    let non_rest = children.iter().filter(|c| !is_rest_like(c)).count();
                    if non_rest > 10 {
                        return Err(ParseError::new(
                            "too many elements in <...>: max 10 in v1",
                            span,
                        ));
                    }
                    return Ok(Pat {
                        kind: PatKind::AnyOrder { children },
                        span,
                    });
                }
                Token::Ellipsis => {
                    let ell_span = tok.span;
                    self.pos += 1; // consume `...`
                    if let Some((_, _, q_span)) = self.try_quantifier() {
                        return Err(ParseError::new(
                            "`...` cannot take a postfix `*` / `+` / `?` quantifier",
                            q_span,
                        ));
                    }
                    children.push(Pat {
                        kind: PatKind::Rest,
                        span: ell_span,
                    });
                }
                _ => {
                    children.push(self.prefixed(false)?);
                }
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
    fn ident_pat(
        &mut self,
        name: &str,
        span: PatSpan,
        allow_bare_predicate: bool,
    ) -> Result<Pat, ParseError> {
        let kind = match name {
            "true" => PatKind::Lit(Lit::True),
            "false" => PatKind::Lit(Lit::False),
            "nil" => PatKind::Lit(Lit::Nil),
            _ => {
                if let Some(tag) = murphy_ast::tag_from_pattern_name(name) {
                    PatKind::Kind(tag)
                } else if allow_bare_predicate
                    && matches!(
                        self.peek().map(|tok| &tok.tok),
                        Some(Token::Question) | Some(Token::Bang)
                    )
                {
                    let Some(suffix_tok) = self.next() else {
                        unreachable!("checked by matches above");
                    };
                    let suffix_span = suffix_tok.span;
                    let suffix = match suffix_tok.tok {
                        Token::Question => "?",
                        Token::Bang => "!",
                        _ => unreachable!("checked by the matches arm above"),
                    };
                    let pred_name = format!("{name}{suffix}");
                    // Bare predicate shorthand also supports args: `foo?(42)`.
                    let args = if matches!(self.peek().map(|t| &t.tok), Some(Token::LParen)) {
                        let lparen_span = self.peek().unwrap().span;
                        self.pos += 1; // consume `(`
                        self.predicate_args(lparen_span)?
                    } else {
                        vec![]
                    };
                    return Ok(Pat {
                        kind: PatKind::Predicate {
                            name: pred_name,
                            args,
                        },
                        span: PatSpan::new(span.start as usize, suffix_span.end as usize),
                    });
                } else {
                    // A `?` / `!` follow on a name that isn't a node kind hints
                    // at predicate intent — surface the `#name?` / `#name!` form
                    // so users learn the explicit syntax, especially in
                    // positions (root, head, Sym slot) where bare predicate
                    // shorthand is not accepted.
                    let suffix = match self.peek().map(|tok| &tok.tok) {
                        Some(Token::Question) => Some('?'),
                        Some(Token::Bang) => Some('!'),
                        _ => None,
                    };
                    let msg = match suffix {
                        Some(c) => format!(
                            "unknown node type `{name}` — write `#{name}{c}` to call the host predicate",
                        ),
                        None => format!("unknown node type `{name}`"),
                    };
                    let span = match suffix {
                        Some(_) => {
                            let suffix_end = self.peek().map(|t| t.span.end).unwrap_or(span.end);
                            PatSpan {
                                start: span.start,
                                end: suffix_end,
                            }
                        }
                        None => span,
                    };
                    return Err(ParseError::new(msg, span));
                }
            }
        };
        Ok(Pat { kind, span })
    }

    /// Parse a predicate argument list that starts after the `(` has been
    /// consumed. Reads space-separated args until `)`.
    ///
    /// Valid arg forms (v1): integer literal, float literal, string literal,
    /// symbol literal, `$ident` back-reference capture.
    ///
    /// Pattern args (`#pred?({:A :B})`) are v1 scope-out and produce an error
    /// with the message `"pattern args in v1: literal/capture only"`.
    fn predicate_args(&mut self, lparen_span: PatSpan) -> Result<Vec<PredArg>, ParseError> {
        let mut args: Vec<PredArg> = Vec::new();
        loop {
            let Some(next) = self.peek() else {
                return Err(ParseError::new(
                    "unclosed `(` in predicate argument list",
                    lparen_span,
                ));
            };
            match &next.tok {
                Token::RParen => {
                    self.pos += 1; // consume `)`
                    break;
                }
                Token::Int(v) => {
                    let v = *v;
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::Int(v)));
                }
                Token::Float(v) => {
                    let v = *v;
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::Float(v)));
                }
                Token::Str(s) => {
                    let s = s.clone();
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::Str(s)));
                }
                Token::Sym(s) => {
                    let s = s.clone();
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::Sym(s)));
                }
                Token::Ident(name) if name == "true" => {
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::True));
                }
                Token::Ident(name) if name == "false" => {
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::False));
                }
                Token::Ident(name) if name == "nil" => {
                    self.pos += 1;
                    args.push(PredArg::Lit(Lit::Nil));
                }
                Token::Dollar => {
                    // `$ident` back-reference to an already-declared capture slot.
                    let dollar_span = next.span;
                    self.pos += 1; // consume `$`
                    let Some(ident_tok) = self.peek() else {
                        return Err(ParseError::new(
                            "expected capture name after `$` in predicate arg",
                            dollar_span,
                        ));
                    };
                    let Token::Ident(capture_name) = &ident_tok.tok else {
                        let err_span = ident_tok.span;
                        return Err(ParseError::new(
                            "expected identifier after `$` in predicate arg",
                            err_span,
                        ));
                    };
                    let capture_name = capture_name.clone();
                    let ident_span = ident_tok.span;
                    self.pos += 1; // consume the ident
                    // Resolve the name to an existing capture slot (back-reference only).
                    let slot = self
                        .capture_names
                        .iter()
                        .position(|n| n == &capture_name)
                        .map(|i| i as u16)
                        .ok_or_else(|| {
                            ParseError::new(
                                format!(
                                    "unknown or forward capture reference `${capture_name}` in predicate arg"
                                ),
                                ident_span,
                            )
                        })?;
                    args.push(PredArg::Capture(slot));
                }
                Token::LBrace => {
                    // Pattern arg: v1 scope-out.
                    return Err(ParseError::new(
                        "pattern args in v1: literal/capture only",
                        next.span,
                    ));
                }
                Token::LParen => {
                    // Nested pattern arg: v1 scope-out.
                    return Err(ParseError::new(
                        "pattern args in v1: literal/capture only",
                        next.span,
                    ));
                }
                _ => {
                    let err_span = next.span;
                    return Err(ParseError::new(
                        "pattern args in v1: literal/capture only",
                        err_span,
                    ));
                }
            }
        }
        Ok(args)
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
    fn parses_operator_and_uppercase_symbols() {
        // murphy-ke0: operator-method and uppercase symbols flow through to
        // `Lit::Sym` unchanged.
        assert_eq!(k(":+"), PatKind::Lit(Lit::Sym("+".into())));
        assert_eq!(k(":[]="), PatKind::Lit(Lit::Sym("[]=".into())));
        assert_eq!(k(":<=>"), PatKind::Lit(Lit::Sym("<=>".into())));
        assert_eq!(k(":Foo"), PatKind::Lit(Lit::Sym("Foo".into())));
    }

    #[test]
    fn parses_variable_symbols_in_node_match() {
        // murphy-afl: `:@x`/`:@@x`/`:$x` flow through the lexer with the
        // sigil preserved, so RuboCop-style patterns matching the first
        // child of `(ivar :@foo)` / `(cvar :@@foo)` / `(gvar :$foo)` parse
        // end-to-end.
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
        // `:Foo` exercises the uppercase symbol grammar (murphy-ke0); the test
        // asserts only that the capture body is a `Node`, so the symbol payload
        // is incidental.
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

    // --- murphy-ycx: postfix quantifier (`*`, `+`, `?`) -------------------

    /// Pull the children out of a top-level `(...)` parse, panicking with the
    /// pattern source if the root is not a `Node`. Only used by quantifier
    /// tests below.
    fn children_of(src: &str) -> Vec<Pat> {
        let p = parse(src).unwrap_or_else(|e| panic!("parse `{src}`: {e:?}"));
        match p.root.kind {
            PatKind::Node { children, .. } => children,
            other => panic!("`{src}` should be a Node, got {other:?}"),
        }
    }

    #[test]
    fn parses_plus_quantifier_on_kind() {
        // `(array int+)` — the lone child is a quantifier wrapping `Kind(int)`.
        let cs = children_of("(array int+)");
        assert_eq!(cs.len(), 1);
        match &cs[0].kind {
            PatKind::Quantifier { body, min, max } => {
                assert_eq!(*min, 1);
                assert_eq!(*max, u8::MAX);
                assert!(matches!(body.kind, PatKind::Kind(_)));
            }
            other => panic!("expected Quantifier, got {other:?}"),
        }
    }

    #[test]
    fn parses_star_quantifier_on_kind() {
        let cs = children_of("(array int*)");
        match &cs[0].kind {
            PatKind::Quantifier { min, max, .. } => {
                assert_eq!(*min, 0);
                assert_eq!(*max, u8::MAX);
            }
            other => panic!("expected Quantifier, got {other:?}"),
        }
    }

    #[test]
    fn parses_question_quantifier_on_kind() {
        let cs = children_of("(send _ :update_columns hash?)");
        match &cs[2].kind {
            PatKind::Quantifier { min, max, .. } => {
                assert_eq!(*min, 0);
                assert_eq!(*max, 1);
            }
            other => panic!("expected Quantifier, got {other:?}"),
        }
    }

    #[test]
    fn quantifier_span_covers_body_through_postfix() {
        // `(array int+)` — `int` at 7..10, `+` at 10..11; the Quantifier
        // span must cover 7..11.
        let cs = children_of("(array int+)");
        assert_eq!((cs[0].span.start, cs[0].span.end), (7, 11));
    }

    #[test]
    fn parses_quantifier_with_rest_in_same_child_list() {
        // `(send _ :foo ... int+)` — `...` and a quantifier coexist in the
        // same child list, in DESIGN's recommended mix-with-rest form.
        let cs = children_of("(send _ :foo ... int+)");
        assert!(matches!(cs[2].kind, PatKind::Rest));
        assert!(matches!(cs[3].kind, PatKind::Quantifier { .. }));
    }

    #[test]
    fn parses_quantifier_on_sym_kind() {
        // `(send _ :pluck sym+)` — a quantifier on `Kind(sym)`.
        let cs = children_of("(send _ :pluck sym+)");
        match &cs[2].kind {
            PatKind::Quantifier { body, min, .. } => {
                assert_eq!(*min, 1);
                assert!(matches!(body.kind, PatKind::Kind(_)));
            }
            other => panic!("expected Quantifier, got {other:?}"),
        }
    }

    #[test]
    fn parses_bare_predicate_in_node_child_slot() {
        let cs = children_of("(int odd?)");
        assert_eq!(cs.len(), 1);
        assert_eq!(
            cs[0].kind,
            PatKind::Predicate {
                name: "odd?".into(),
                args: vec![]
            }
        );
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

        assert_eq!(
            cs[3].kind,
            PatKind::Predicate {
                name: "odd?".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn disallows_unknown_bare_predicate_in_sym_child_slot() {
        let e = parse("(send _ odd? :foo)").expect_err("unknown predicate in sym slot");
        assert!(e.message.contains("unknown node type") && e.message.contains("odd"));
    }

    #[test]
    fn parses_bare_predicate_in_union_in_node_child_slot() {
        // The union sits in a node-child slot, so each alt is at the same
        // effective position — both `odd?` and `even?` parse as predicate
        // shorthand. `allow_bare_predicate` must flow into `union`.
        let cs = children_of("(send _ :puts {odd? even?})");
        assert_eq!(cs.len(), 3);
        let PatKind::Union(alts) = &cs[2].kind else {
            panic!("expected Union, got {:?}", cs[2].kind);
        };
        assert_eq!(alts.len(), 2);
        assert_eq!(
            alts[0].kind,
            PatKind::Predicate {
                name: "odd?".into(),
                args: vec![]
            }
        );
        assert_eq!(
            alts[1].kind,
            PatKind::Predicate {
                name: "even?".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn parses_bare_predicate_in_capture_body_in_node_child_slot() {
        // `$odd?` inside a node-child slot — capture body inherits the
        // allow-bare-predicate flag and the body parses as a `Predicate`.
        let p = parse("(int $odd?)").expect("ok");
        let cs = match &p.root.kind {
            PatKind::Node { children, .. } => children,
            other => panic!("expected Node, got {other:?}"),
        };
        assert_eq!(cs.len(), 1);
        let PatKind::Capture { body, .. } = &cs[0].kind else {
            panic!("expected Capture, got {:?}", cs[0].kind);
        };
        assert_eq!(
            body.kind,
            PatKind::Predicate {
                name: "odd?".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn parses_bang_form_bare_predicate_in_send_arg_slot() {
        // `save!` in a node-child slot: bare predicate shorthand for `#save!`.
        let cs = children_of("(send _ :foo save!)");
        assert_eq!(cs.len(), 3);
        assert_eq!(
            cs[2].kind,
            PatKind::Predicate {
                name: "save!".into(),
                args: vec![]
            }
        );
    }

    #[test]
    fn top_level_bare_predicate_errors_with_hint() {
        // `save!` at top level is not in a node-child slot, so the parser
        // can't accept it as bare predicate shorthand. The error names the
        // explicit `#save!` form so users learn the host-predicate syntax.
        let e = parse("save!").expect_err("must reject bare predicate at top level");
        assert!(
            e.message.contains("unknown node type") && e.message.contains("`#save!`"),
            "expected hint about `#save!`, got: {}",
            e.message,
        );

        let e = parse("save?").expect_err("must reject bare predicate at top level");
        assert!(
            e.message.contains("`#save?`"),
            "expected hint about `#save?`, got: {}",
            e.message,
        );
    }

    #[test]
    fn sym_slot_bare_predicate_error_includes_hint() {
        // Symbol slots disallow bare predicates even in node-child position.
        // The error should still surface the `#name?` hint.
        let e =
            parse("(send _ save?)").expect_err("bare predicate not allowed in Sym slot of send");
        assert!(
            e.message.contains("`#save?`"),
            "expected hint about `#save?`, got: {}",
            e.message,
        );
    }

    // --- $pat+ / $pat* / $pat? capture slot kinds ------------------------

    #[test]
    fn capture_with_plus_body_is_seq_slot() {
        // `(send _ :pluck $sym+)` — anonymous capture with a `Quantifier` body,
        // slot kind upgrades to `Seq` so the matcher returns a slice.
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
        // `(send _ :update_columns $hash?)` — `?` produces `OptNode`.
        let p = parse("(send _ :update_columns $hash?)").expect("ok");
        assert_eq!(p.capture_kinds(), &[CaptureKind::OptNode]);
    }

    #[test]
    fn named_capture_without_postfix_keeps_node_kind() {
        // `(send $receiver _)` — no postfix; the existing named-capture
        // behavior (body = Wildcard, slot = Node) is preserved.
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
        // `(send $name?)` — `$` + ident + postfix becomes an anonymous
        // capture whose body is `Quantifier(Kind(name), ?)`. There is no
        // `name` *named* capture: the slot is anonymous and has no `name`.
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

    // --- error cases (the 5 DESIGN-listed parse failures) -----------------

    #[test]
    fn error_quantifier_at_top_level() {
        // `int+` — quantifier outside a node child list.
        let e = parse("int+").expect_err("top-level quantifier");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_in_union_arm() {
        // `{int+ sym}` — quantifier inside `{}` is not a node child either.
        let e = parse("{int+ sym}").expect_err("quantifier in union arm");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_not_sigil_body() {
        // `!int+` — `!` wraps `int+`, so the quantifier's parent is `Not`,
        // not a node match. DESIGN writes this as `!(int+)`, but parens
        // around `int+` start a node match (not grouping), so the
        // single-pattern form `!int+` is the actual syntactic shape.
        let e = parse("!int+").expect_err("quantifier under `!`");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_parent_sigil_body() {
        // `^int+` — the body of `^` is not a node child.
        let e = parse("^int+").expect_err("quantifier under `^`");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_quantifier_inside_descend_sigil_body() {
        // `` `int+ `` — the body of `` ` `` is not a node child.
        let e = parse("`int+").expect_err("quantifier under backtick");
        assert!(
            e.message.contains("direct child of a node match"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_chained_postfix_plus_plus() {
        // `(array int++)` — two postfixes in a row is a parser-level reject.
        let e = parse("(array int++)").expect_err("chained ++");
        assert!(
            e.message.contains("chained") || e.message.contains("at most one"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_chained_postfix_star_question() {
        let e = parse("(array int*?)").expect_err("chained *?");
        assert!(
            e.message.contains("chained") || e.message.contains("at most one"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_capture_inside_quantifier_body_via_parens() {
        // `(array ($int)+)` — `($int)` parses as a Node with missing head
        // (rejected earlier) — the message can be either "needs a head" or
        // a capture-in-quantifier-body message depending on parse order.
        // The acceptance criterion DESIGN names is `($int)+`; verify it
        // errors *somehow*.
        assert!(parse("(array ($int)+)").is_err());
    }

    #[test]
    fn error_capture_inside_quantifier_body_via_node() {
        // `(array (send _ $_)+)` — a quantifier wrapping a Node whose
        // children include a `$` capture; rejected by
        // `validate_quantifier_body`.
        let e = parse("(array (send _ $_)+)").expect_err("capture in quantifier body");
        assert!(
            e.message.contains("quantifier body"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_rest_with_postfix_quantifier() {
        // `(array ...+)` — chaining `+` on `...` is a parser-level reject.
        let e = parse("(array ...+)").expect_err("rest with postfix");
        assert!(
            e.message.contains("...") && e.message.contains("quantifier"),
            "message was: {}",
            e.message
        );
    }

    #[test]
    fn error_standalone_postfix_token() {
        // A bare `+` with nothing before it must error at primary.
        let e = parse("+").expect_err("bare +");
        assert!(
            e.message.contains("must follow a pattern"),
            "message was: {}",
            e.message
        );
    }

    // --- mixing rule: at most one rest, but quantifier may coexist ----------

    #[test]
    fn allows_rest_and_quantifier_in_same_child_list() {
        // DESIGN mixing rule: at most one rest, but a quantifier may also
        // appear alongside it.
        assert!(parse("(send _ :foo ... int+)").is_ok());
        assert!(parse("(send _ :foo int+ ...)").is_ok());
    }

    #[test]
    fn allows_multiple_quantifiers_in_same_child_list() {
        // No "at most one quantifier" rule: a child list may have several.
        assert!(parse("(send _ :foo int+ sym*)").is_ok());
        assert!(parse("(send _ :foo int? str+ sym*)").is_ok());
    }

    // --- murphy-jyi: predicate args (#name(arg1 arg2 ...)) ----------------

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
        // `(send $val #contains?($val))` — `$val` is slot 0, capture back-ref
        let pat = parse("(send $val #contains?($val))").expect("parse ok");
        let PatKind::Node { children, .. } = &pat.root.kind else {
            panic!("expected Node");
        };
        // children: [Capture(slot=0), Predicate{name, args:[Capture(0)]}]
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
        // Pattern args are v1 scope-out; the parser must reject them.
        let e = parse("#g?({:A :B})").expect_err("must reject pattern arg");
        assert!(
            e.message
                .contains("pattern args in v1: literal/capture only"),
            "expected 'pattern args in v1: literal/capture only', got: {}",
            e.message
        );
    }

    #[test]
    fn parse_predicate_unknown_capture_ref_is_rejected() {
        // Forward/unknown capture refs in predicate args must error.
        let e = parse("#pred?($unknown)").expect_err("must reject unknown capture ref");
        assert!(
            e.message.contains("unknown or forward capture reference"),
            "expected 'unknown or forward capture reference', got: {}",
            e.message
        );
    }
}
