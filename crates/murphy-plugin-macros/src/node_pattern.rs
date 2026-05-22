//! `node_pattern!` — the B backend of Murphy's pattern mechanism
//! (murphy-9cr.18). Lowers an S-expression pattern to a Rust matcher
//! `fn` at compile time. See
//! `docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md`.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

use murphy_pattern::{CaptureKind, PatternAst};

/// Parsed `node_pattern!(name, "pattern")` invocation.
struct NodePatternInput {
    name: Ident,
    pattern: LitStr,
}

impl Parse for NodePatternInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let pattern = input.parse()?;
        Ok(NodePatternInput { name, pattern })
    }
}

/// Entry point for the `#[proc_macro] node_pattern`.
pub fn node_pattern(input: TokenStream) -> TokenStream {
    let input: NodePatternInput = match syn::parse2(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };
    let ast = match murphy_pattern::parse(&input.pattern.value()) {
        Ok(a) => a,
        Err(e) => {
            return syn::Error::new(
                input.pattern.span(),
                format!("node_pattern!: pattern parse error: {e}"),
            )
            .to_compile_error();
        }
    };
    match lower_matcher(&input.name, &ast) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

/// Build the whole matcher `fn` from a parsed pattern.
fn lower_matcher(name: &Ident, ast: &PatternAst) -> syn::Result<TokenStream> {
    let n_caps = ast.n_captures();
    // Return type: `bool` for zero captures, `Option<(..)>` otherwise.
    let cap_tys: Vec<TokenStream> = ast
        .capture_kinds()
        .iter()
        .map(|k| match k {
            CaptureKind::Node => quote!(::murphy_ast::NodeId),
            CaptureKind::Seq => quote!(&'a [::murphy_ast::NodeId]),
        })
        .collect();
    let cap_decls: Vec<TokenStream> = (0..n_caps)
        .map(|i| {
            let id = cap_ident(i);
            let ty = &cap_tys[i];
            quote!(let #id: #ty;)
        })
        .collect();
    let cap_idents: Vec<Ident> = (0..n_caps).map(cap_ident).collect();

    let (ret_ty, fail, success) = if n_caps == 0 {
        (quote!(bool), quote!(false), quote!(true))
    } else {
        (
            quote!(::core::option::Option<(#(#cap_tys,)*)>),
            quote!(::core::option::Option::None),
            quote!(::core::option::Option::Some((#(#cap_idents,)*))),
        )
    };

    let mut ctx = Lower {
        fail: fail.clone(),
        capture_allowed: true,
        next: 0,
    };
    let body = lower_pat(&ast.root, &quote!(node), &mut ctx)?;

    Ok(quote! {
        // A capture matcher's `OptNode`-slot lowering emits `let Some(n) =
        // slot.get() else { return None; }`, which clippy wants rewritten
        // with `?`. The rewrite is not uniformly valid — a zero-capture
        // matcher returns `bool`, not `Option` — so silence the lint here.
        #[allow(clippy::question_mark)]
        fn #name<'a>(
            node: ::murphy_ast::NodeId,
            cx: &::murphy_plugin_api::Cx<'a>,
        ) -> #ret_ty {
            #(#cap_decls)*
            #body
            #success
        }
    })
}

/// The capture binding identifier for slot `i`.
fn cap_ident(i: usize) -> Ident {
    Ident::new(&format!("__cap{i}"), proc_macro2::Span::call_site())
}

/// Mutable state threaded through the recursive lowering.
struct Lower {
    /// The expression a failed guard returns (`false` or `None`).
    fail: TokenStream,
    /// Whether a `$` capture is legal at the current position. Set false
    /// inside `{}` union, `!` negation and `` ` `` descend.
    capture_allowed: bool,
    /// Monotonic counter feeding [`gensym`]; guarantees unique binding
    /// identifiers across recursion depth so nested `(send (send ...) ...)`
    /// matches do not shadow each other's `__b*` / `__n*` / `__list*`.
    next: usize,
}

/// Allocate a fresh, collision-free binding identifier `#{prefix}#{n}`.
fn gensym(ctx: &mut Lower, prefix: &str) -> Ident {
    let n = ctx.next;
    ctx.next += 1;
    Ident::new(&format!("{prefix}{n}"), Span::call_site())
}

/// The `return <fail>;` statement for a mismatched guard.
fn fail_stmt(ctx: &Lower) -> TokenStream {
    let f = &ctx.fail;
    quote!(return #f;)
}

/// One pattern-child slot of a NodeKind: how a pattern child maps onto an
/// arena field.
#[derive(Clone, Copy)]
enum SlotTy {
    /// `NodeId` field — recurse into the child node (always present).
    Node,
    /// `OptNodeId` field — `nil?` matches absence, else the child must be
    /// present and recurse.
    OptNode,
    /// `Symbol` field — only a `:sym` literal or `_` pattern child.
    Sym,
    /// `NodeList` field — the remaining pattern children, `cx.list()`-resolved.
    List,
}

/// A pattern-child slot: the arena field to bind plus its type.
struct Slot {
    /// Field reference for the destructuring pattern. `Named` for struct
    /// variants, `Pos(arity, index)` for tuple variants.
    field: FieldRef,
    ty: SlotTy,
}

#[derive(Clone, Copy)]
enum FieldRef {
    Named(&'static str),
    /// (tuple variant arity, this field's index)
    Pos(usize, usize),
}

/// The pattern-child schema for one matchable NodeKind variant.
struct KindSchema {
    /// The `NodeKind::` variant identifier (e.g. "Send").
    variant: &'static str,
    slots: &'static [Slot],
    /// `true` iff `slots` references *every* field of the variant. When
    /// `false` the variant has fields the schema deliberately omits (e.g.
    /// `Case::else_`), and the generated struct-variant destructuring
    /// pattern must end with a trailing `..` to stay exhaustive. Tuple
    /// variants always cover all fields, so this is `true` for them.
    covers_all_fields: bool,
}

// --- Per-NodeKind slot tables -------------------------------------------
//
// Each table mirrors a `NodeKind` variant from `crates/murphy-ast/src/node.rs`
// (the canon). Fixed slots appear in parser-gem child order; a `List` slot,
// if present, must be last (v1 slot convention: at most one trailing `List`).
// Tuple variants use `FieldRef::Pos(arity, index)`.

// Assignment variants — all `{ name: Symbol, value: OptNodeId }`.
static LVASGN_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Sym,
    },
    Slot {
        field: FieldRef::Named("value"),
        ty: SlotTy::OptNode,
    },
];
static IVASGN_SLOTS: &[Slot] = LVASGN_SLOTS; // identical shape
static GVASGN_SLOTS: &[Slot] = LVASGN_SLOTS; // identical shape
static CVASGN_SLOTS: &[Slot] = LVASGN_SLOTS; // identical shape
static CASGN_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("scope"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Sym,
    },
    Slot {
        field: FieldRef::Named("value"),
        ty: SlotTy::OptNode,
    },
];
static SEND_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("receiver"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("method"),
        ty: SlotTy::Sym,
    },
    Slot {
        field: FieldRef::Named("args"),
        ty: SlotTy::List,
    },
];
static CSEND_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("receiver"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("method"),
        ty: SlotTy::Sym,
    },
    Slot {
        field: FieldRef::Named("args"),
        ty: SlotTy::List,
    },
];
static BLOCK_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("call"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("args"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("body"),
        ty: SlotTy::OptNode,
    },
];
static CONST_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("scope"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Sym,
    },
];
// `Array(NodeList)` / `Hash(NodeList)` / `Begin(NodeList)`: single tuple
// field, arity 1, index 0.
static LIST_TUPLE_SLOTS: &[Slot] = &[Slot {
    field: FieldRef::Pos(1, 0),
    ty: SlotTy::List,
}];
static PAIR_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("key"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("value"),
        ty: SlotTy::Node,
    },
];
static IF_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("cond"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("then_"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("else_"),
        ty: SlotTy::OptNode,
    },
];
// `Case { subject, whens: NodeList, else_: OptNodeId }`: `else_` follows the
// `NodeList`, but the v1 slot convention allows at most one trailing `List`.
// `else_` is therefore omitted from the schema (covers_all_fields = false)
// and cannot be referenced from a pattern.
static CASE_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("subject"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("whens"),
        ty: SlotTy::List,
    },
];
// `When { conds: NodeList, body: OptNodeId }`: `body` follows the `NodeList`,
// so it is omitted (covers_all_fields = false) for the same reason as
// `Case::else_`.
static WHEN_SLOTS: &[Slot] = &[Slot {
    field: FieldRef::Named("conds"),
    ty: SlotTy::List,
}];
// `Return(OptNodeId)`: single tuple field, arity 1, index 0.
static RETURN_SLOTS: &[Slot] = &[Slot {
    field: FieldRef::Pos(1, 0),
    ty: SlotTy::OptNode,
}];
static AND_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("lhs"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("rhs"),
        ty: SlotTy::Node,
    },
];
static OR_SLOTS: &[Slot] = AND_SLOTS; // identical shape
// `Def { receiver, name, args, body }`: `receiver` is omitted from the
// schema (covers_all_fields = false). It precedes `name` in struct
// declaration order, but for `def` patterns the meaningful children are
// `name`/`args`/`body`; singleton-method discrimination is out of v1 scope.
static DEF_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Sym,
    },
    Slot {
        field: FieldRef::Named("args"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("body"),
        ty: SlotTy::OptNode,
    },
];
static CLASS_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("superclass"),
        ty: SlotTy::OptNode,
    },
    Slot {
        field: FieldRef::Named("body"),
        ty: SlotTy::OptNode,
    },
];
static MODULE_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("name"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("body"),
        ty: SlotTy::OptNode,
    },
];
// `While { cond, body, post: bool }` / `Until { ... }`: `post` is a `bool`
// flag, not a child node — it has no `SlotTy` and is omitted from the schema
// (covers_all_fields = false).
static WHILE_SLOTS: &[Slot] = &[
    Slot {
        field: FieldRef::Named("cond"),
        ty: SlotTy::Node,
    },
    Slot {
        field: FieldRef::Named("body"),
        ty: SlotTy::OptNode,
    },
];
static UNTIL_SLOTS: &[Slot] = WHILE_SLOTS; // identical shape

/// The full v1 `node_pattern!` schema table, keyed by `NodeKindTag` `u8`.
///
/// The `u8` tags are the `NodeKind` discriminants. The source of truth for
/// the tag ↔ pattern-name mapping is `crates/murphy-ast/src/kinds.rs`
/// `KIND_PATTERN_NAMES` (and the `NodeKind::tag()` `match` in
/// `crates/murphy-ast/src/node.rs`). A future renumber must update those
/// tables and this one together; the `schema_tags_match_pattern_names`
/// unit test below guards the link.
static SCHEMA_TABLE: &[(u8, KindSchema)] = &[
    (
        13,
        KindSchema {
            variant: "Const",
            slots: CONST_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        14,
        KindSchema {
            variant: "Lvasgn",
            slots: LVASGN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        15,
        KindSchema {
            variant: "Ivasgn",
            slots: IVASGN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        16,
        KindSchema {
            variant: "Casgn",
            slots: CASGN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        17,
        KindSchema {
            variant: "Send",
            slots: SEND_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        18,
        KindSchema {
            variant: "Csend",
            slots: CSEND_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        19,
        KindSchema {
            variant: "Block",
            slots: BLOCK_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        22,
        KindSchema {
            variant: "Array",
            slots: LIST_TUPLE_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        23,
        KindSchema {
            variant: "Hash",
            slots: LIST_TUPLE_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        24,
        KindSchema {
            variant: "Pair",
            slots: PAIR_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        25,
        KindSchema {
            variant: "If",
            slots: IF_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        // `Case` omits `else_`: it follows the `whens` NodeList, and v1
        // allows at most one trailing `List` slot.
        26,
        KindSchema {
            variant: "Case",
            slots: CASE_SLOTS,
            covers_all_fields: false,
        },
    ),
    (
        // `When` omits `body`: it follows the `conds` NodeList.
        27,
        KindSchema {
            variant: "When",
            slots: WHEN_SLOTS,
            covers_all_fields: false,
        },
    ),
    (
        28,
        KindSchema {
            variant: "Begin",
            slots: LIST_TUPLE_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        29,
        KindSchema {
            variant: "Return",
            slots: RETURN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        30,
        KindSchema {
            variant: "And",
            slots: AND_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        31,
        KindSchema {
            variant: "Or",
            slots: OR_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        // `Def` omits `receiver`: singleton-method discrimination is out
        // of v1 pattern scope.
        32,
        KindSchema {
            variant: "Def",
            slots: DEF_SLOTS,
            covers_all_fields: false,
        },
    ),
    (
        33,
        KindSchema {
            variant: "Class",
            slots: CLASS_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        34,
        KindSchema {
            variant: "Module",
            slots: MODULE_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        38,
        KindSchema {
            variant: "Gvasgn",
            slots: GVASGN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        39,
        KindSchema {
            variant: "Cvasgn",
            slots: CVASGN_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        // `While` omits `post`: a `bool` flag, not a child node.
        47,
        KindSchema {
            variant: "While",
            slots: WHILE_SLOTS,
            covers_all_fields: false,
        },
    ),
    (
        // `Until` omits `post`: a `bool` flag, not a child node.
        48,
        KindSchema {
            variant: "Until",
            slots: UNTIL_SLOTS,
            covers_all_fields: false,
        },
    ),
];

/// Resolve a `NodeKindTag` `u8` to its structural schema. `None` means the
/// kind has no `node_pattern!` schema in v1.
fn schema_for(tag: u8) -> Option<&'static KindSchema> {
    SCHEMA_TABLE.iter().find(|(t, _)| *t == tag).map(|(_, s)| s)
}

/// Lower one `Pat` against `subject` (a `NodeId`-typed expression) into a
/// block of guard statements that `return ctx.fail` on mismatch.
fn lower_pat(
    pat: &murphy_pattern::Pat,
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::{Lit, PatKind};
    match &pat.kind {
        PatKind::Wildcard => Ok(quote!()),
        PatKind::Lit(lit) => {
            let fail = fail_stmt(ctx);
            let guard = match lit {
                Lit::Int(v) => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_ast::NodeKind::Int(__v) if __v == #v
                    ) {
                        #fail
                    }
                },
                Lit::Float(v) => quote! {
                    if let ::murphy_ast::NodeKind::Float(__v) = *cx.kind(#subject) {
                        // Exact equality is intentional: the pattern author wrote a specific float literal.
                        #[allow(clippy::float_cmp)]
                        if __v != #v {
                            #fail
                        }
                    } else {
                        #fail
                    }
                },
                Lit::Str(s) => {
                    let s = s.as_str();
                    quote! {
                        if !::core::matches!(
                            *cx.kind(#subject),
                            ::murphy_ast::NodeKind::Str(__id) if cx.string_str(__id) == #s
                        ) {
                            #fail
                        }
                    }
                }
                Lit::Sym(s) => {
                    let s = s.as_str();
                    quote! {
                        if !::core::matches!(
                            *cx.kind(#subject),
                            ::murphy_ast::NodeKind::Sym(__sym) if cx.symbol_str(__sym) == #s
                        ) {
                            #fail
                        }
                    }
                }
                Lit::True => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_ast::NodeKind::True_
                    ) {
                        #fail
                    }
                },
                Lit::False => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_ast::NodeKind::False_
                    ) {
                        #fail
                    }
                },
                Lit::Nil => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_ast::NodeKind::Nil
                    ) {
                        #fail
                    }
                },
            };
            Ok(guard)
        }
        PatKind::Kind(tag) => {
            let fail = fail_stmt(ctx);
            let tag_u8 = tag.0;
            Ok(quote! {
                if cx.kind(#subject).tag() != ::murphy_ast::NodeKindTag(#tag_u8) {
                    #fail
                }
            })
        }
        PatKind::NilTest => {
            let fail = fail_stmt(ctx);
            Ok(quote! {
                if !::core::matches!(
                    *cx.kind(#subject),
                    ::murphy_ast::NodeKind::Nil
                ) {
                    #fail
                }
            })
        }
        PatKind::Node { head, children } => lower_node(head, children, subject, ctx),
        PatKind::Capture {
            slot,
            name: _,
            body,
        } => {
            if !ctx.capture_allowed {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "node_pattern!: `$` capture is not allowed inside `{}` / `!` / `` ` ``",
                ));
            }
            // A `$...` seq capture (`Capture` whose `body` is `Rest`) is
            // handled by the `List` slot, not here — see Task 7.
            if matches!(body.kind, PatKind::Rest) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "node_pattern!: seq capture (`$...`) not yet supported",
                ));
            }
            // Node capture: lower the body's guards first (so a mismatch
            // returns `ctx.fail` before the slot is written), then assign
            // the captured node id into the deferred-init capture variable.
            let body_guards = lower_pat(body, subject, ctx)?;
            let cap = cap_ident(*slot as usize);
            Ok(quote!(#body_guards #cap = #subject;))
        }
        other => Err(syn::Error::new(
            Span::call_site(),
            format!("node_pattern!: pattern feature not yet supported: {other:?}"),
        )),
    }
}

/// Enforce the v1 child-list rule for `Head::Any` / `Head::OneOf`: the child
/// list must be either empty or exactly one bare `...` ([`PatKind::Rest`]).
/// Concrete children — anything else — are not supported in v1. A `$...` seq
/// capture is a `PatKind::Capture` (not a bare `Rest`), so it falls into the
/// `else` arm here and is rejected, as v1 requires.
fn check_kind_only_children(children: &[murphy_pattern::Pat]) -> syn::Result<()> {
    use murphy_pattern::PatKind;
    let ok =
        children.is_empty() || (children.len() == 1 && matches!(children[0].kind, PatKind::Rest));
    if ok {
        Ok(())
    } else {
        Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: (_ ...) / ({…} ...) with concrete children \
             is not supported in v1",
        ))
    }
}

/// Lower a `(head child...)` node match against `subject`.
///
/// `Head::Any` (`(_ ...)`) and `Head::OneOf` (`({a b} ...)`) are kind-only
/// matches: they accept an empty child list or a single `...`. `Head::Exact`
/// dispatches the child sequence onto the kind's [`KindSchema`]: fixed slots
/// (`Node`/`OptNode`/`Sym`) consume children left-to-right, and a trailing
/// `List` slot (if any) consumes the remaining children.
fn lower_node(
    head: &murphy_pattern::Head,
    children: &[murphy_pattern::Pat],
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::Head;

    match head {
        // `(_ ...)` — any kind. Validate the child list, emit no kind check.
        Head::Any => {
            check_kind_only_children(children)?;
            Ok(quote!())
        }
        // `({a b} ...)` — kind must be one of `tags`. Validate the child
        // list, emit a single `matches!` guard on the tag.
        Head::OneOf(tags) => {
            check_kind_only_children(children)?;
            let fail = fail_stmt(ctx);
            let tag_u8s: Vec<u8> = tags.iter().map(|t| t.0).collect();
            let t = gensym(ctx, "__t");
            Ok(quote! {
                let #t: u8 = cx.kind(#subject).tag().0;
                if !::core::matches!(#t, #(#tag_u8s)|*) {
                    #fail
                }
            })
        }
        // `(send ...)` — exact kind; dispatch children onto the schema.
        Head::Exact(t) => lower_exact_node(*t, children, subject, ctx),
    }
}

/// Lower a `Head::Exact` node match: look up the structural schema for `tag`
/// and dispatch the child sequence onto its slots.
fn lower_exact_node(
    tag: murphy_ast::NodeKindTag,
    children: &[murphy_pattern::Pat],
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;

    // 1. Look up the per-NodeKind structural schema.
    let schema = schema_for(tag.0).ok_or_else(|| {
        let name = murphy_ast::pattern_name(tag).unwrap_or("?");
        syn::Error::new(
            Span::call_site(),
            format!(
                "node_pattern!: node kind `{name}` is not supported by \
                 node_pattern! in v1 — see follow-up issue"
            ),
        )
    })?;

    // 2. Reject `...` rest children (Task 7 territory).
    if children.iter().any(|c| matches!(c.kind, PatKind::Rest)) {
        return Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: `...` not yet supported",
        ));
    }

    // Split the schema into fixed slots and an optional trailing `List`.
    let has_list = schema
        .slots
        .last()
        .is_some_and(|s| matches!(s.ty, SlotTy::List));
    let fixed_count = schema.slots.len() - usize::from(has_list);

    // 3. Child-count checks.
    if children.len() < fixed_count {
        return Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: too few children",
        ));
    }
    if !has_list && children.len() != fixed_count {
        return Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: wrong number of children",
        ));
    }

    // 4. Allocate a fresh binding ident per slot and build the destructuring.
    let bindings: Vec<Ident> = schema.slots.iter().map(|_| gensym(ctx, "__b")).collect();
    let mut guards: Vec<TokenStream> = vec![build_destructure(schema, &bindings, subject, ctx)];

    // 5. Match each fixed slot against its pattern child.
    for (slot, (bind, child)) in schema
        .slots
        .iter()
        .take(fixed_count)
        .zip(bindings.iter().zip(children))
    {
        guards.push(lower_fixed_slot(slot.ty, bind, child, ctx)?);
    }

    // 6. A trailing `List` slot consumes the remaining (explicit-only)
    //    children. `...` rest is rejected above; Task 7 adds it.
    if has_list {
        let list_bind = &bindings[bindings.len() - 1];
        let list_children = &children[fixed_count..];
        guards.push(lower_trailing_list(list_bind, list_children, ctx)?);
    }

    Ok(quote!({ #(#guards)* }))
}

/// Build the `let NodeKind::Variant { .. } = *cx.kind(subject) else { fail };`
/// destructuring statement for an exact-kind match.
///
/// Struct variants list `field: binding`; tuple variants list positional
/// holes inside `( )`. When the schema does not cover every field of the
/// variant ([`KindSchema::covers_all_fields`] is `false`) a trailing `..`
/// keeps the struct pattern exhaustive. Tuple variants always cover all
/// fields, so they never need `..`.
fn build_destructure(
    schema: &KindSchema,
    bindings: &[Ident],
    subject: &TokenStream,
    ctx: &Lower,
) -> TokenStream {
    let variant = Ident::new(schema.variant, Span::call_site());
    let fail = fail_stmt(ctx);
    let field_pats: Vec<TokenStream> = schema
        .slots
        .iter()
        .zip(bindings)
        .map(|(slot, bind)| match slot.field {
            FieldRef::Named(name) => {
                let f = Ident::new(name, Span::call_site());
                quote!(#f: #bind)
            }
            FieldRef::Pos(arity, index) => {
                let holes = (0..arity).map(|i| if i == index { quote!(#bind) } else { quote!(_) });
                quote!(#(#holes),*)
            }
        })
        .collect();
    match schema.slots.first().map(|s| s.field) {
        Some(FieldRef::Pos(..)) => quote! {
            let ::murphy_ast::NodeKind::#variant(#(#field_pats),*) = *cx.kind(#subject) else {
                #fail
            };
        },
        _ => {
            // Struct variant: append `..` when the schema omits fields.
            let rest = if schema.covers_all_fields {
                quote!()
            } else {
                quote!(, ..)
            };
            quote! {
                let ::murphy_ast::NodeKind::#variant { #(#field_pats),* #rest } = *cx.kind(#subject) else {
                    #fail
                };
            }
        }
    }
}

/// Lower one fixed (non-`List`) slot: match the bound field against its
/// pattern child.
fn lower_fixed_slot(
    ty: SlotTy,
    bind: &Ident,
    child: &murphy_pattern::Pat,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::{Lit, PatKind};
    let fail = fail_stmt(ctx);
    match ty {
        SlotTy::Node => lower_pat(child, &quote!(#bind), ctx),
        SlotTy::OptNode => {
            if matches!(child.kind, PatKind::NilTest) {
                let n = gensym(ctx, "__n");
                Ok(quote! {
                    match #bind.get() {
                        ::core::option::Option::None => {}
                        ::core::option::Option::Some(#n) => {
                            if !::core::matches!(
                                *cx.kind(#n),
                                ::murphy_ast::NodeKind::Nil
                            ) {
                                #fail
                            }
                        }
                    }
                })
            } else {
                let n = gensym(ctx, "__n");
                let inner = lower_pat(child, &quote!(#n), ctx)?;
                Ok(quote! {
                    let ::core::option::Option::Some(#n) = #bind.get() else {
                        #fail
                    };
                    #inner
                })
            }
        }
        SlotTy::Sym => match &child.kind {
            PatKind::Wildcard => Ok(quote!()),
            PatKind::Lit(Lit::Sym(s)) => {
                let s = s.as_str();
                Ok(quote! {
                    if cx.symbol_str(#bind) != #s {
                        #fail
                    }
                })
            }
            _ => Err(syn::Error::new(
                Span::call_site(),
                "node_pattern!: symbol slot only accepts a `:sym` literal or `_`",
            )),
        },
        SlotTy::List => unreachable!("List slot is excluded from fixed slots"),
    }
}

/// Lower a trailing `List` slot: resolve the bound `NodeList` and match each
/// explicit pattern child against the corresponding list element. `...` rest
/// is rejected by the caller; Task 7 adds it.
fn lower_trailing_list(
    list_bind: &Ident,
    list_children: &[murphy_pattern::Pat],
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let fail = fail_stmt(ctx);
    let list_val = gensym(ctx, "__list");
    let len = list_children.len();
    let mut guards: Vec<TokenStream> = vec![quote! {
        let #list_val = cx.list(#list_bind);
        if #list_val.len() != #len {
            #fail
        }
    }];
    for (i, child) in list_children.iter().enumerate() {
        guards.push(lower_pat(child, &quote!(#list_val[#i]), ctx)?);
    }
    Ok(quote!(#(#guards)*))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `SCHEMA_TABLE` tag must resolve to a real pattern name via the
    /// canonical `KIND_PATTERN_NAMES` table — this catches a stale tag
    /// literal after a `NodeKind` renumber. (`murphy_ast::pattern_name`
    /// reads that table.)
    #[test]
    fn schema_tags_match_pattern_names() {
        for (tag, schema) in SCHEMA_TABLE {
            assert!(
                murphy_ast::pattern_name(murphy_ast::NodeKindTag(*tag)).is_some(),
                "SCHEMA_TABLE tag {tag} (variant {}) has no KIND_PATTERN_NAMES entry",
                schema.variant,
            );
        }
    }

    /// Schema tags must be unique — a duplicate would make `schema_for`'s
    /// first-match lookup silently shadow an entry.
    #[test]
    fn schema_tags_are_unique() {
        let mut tags: Vec<u8> = SCHEMA_TABLE.iter().map(|(t, _)| *t).collect();
        tags.sort_unstable();
        let len = tags.len();
        tags.dedup();
        assert_eq!(len, tags.len(), "duplicate tag in SCHEMA_TABLE");
    }
}
