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
///
/// `capture_allowed` is genuinely unread until Task 8 (`$` captures), so
/// `#[allow(dead_code)]` keeps the struct clean under `-D warnings` until
/// that arm lands.
#[allow(dead_code)]
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
    #[allow(dead_code)] // First used in Task 5 (tuple-variant NodeKinds).
    Pos(usize, usize),
}

/// The pattern-child schema for one matchable NodeKind variant.
struct KindSchema {
    /// The `NodeKind::` variant identifier (e.g. "Send").
    variant: &'static str,
    slots: &'static [Slot],
}

/// Per-NodeKind structural schema, keyed by `NodeKindTag` `u8`. v1 covers
/// only the four kinds below; Task 5 extends this table to ~25 kinds.
///
/// All four variants are struct variants whose schema covers *every* field,
/// so the generated destructuring pattern lists all fields and never emits
/// a trailing `..`.
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

/// Resolve a `NodeKindTag` `u8` to its structural schema. `None` means the
/// kind has no `node_pattern!` schema in v1.
fn schema_for(tag: u8) -> Option<&'static KindSchema> {
    static SEND: KindSchema = KindSchema {
        variant: "Send",
        slots: SEND_SLOTS,
    };
    static CSEND: KindSchema = KindSchema {
        variant: "Csend",
        slots: CSEND_SLOTS,
    };
    static CONST: KindSchema = KindSchema {
        variant: "Const",
        slots: CONST_SLOTS,
    };
    static IF: KindSchema = KindSchema {
        variant: "If",
        slots: IF_SLOTS,
    };
    match tag {
        17 => Some(&SEND),
        18 => Some(&CSEND),
        13 => Some(&CONST),
        25 => Some(&IF),
        _ => None,
    }
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
        other => Err(syn::Error::new(
            Span::call_site(),
            format!("node_pattern!: pattern feature not yet supported: {other:?}"),
        )),
    }
}

/// Lower a `(head child...)` node match against `subject`.
///
/// Task 4 handles `Head::Exact` only; `Head::Any` / `Head::OneOf` land in
/// Task 5. The child sequence is dispatched onto the kind's [`KindSchema`]:
/// fixed slots (`Node`/`OptNode`/`Sym`) consume children left-to-right, and
/// a trailing `List` slot (if any) consumes the remaining children.
fn lower_node(
    head: &murphy_pattern::Head,
    children: &[murphy_pattern::Pat],
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::{Head, Lit, PatKind};

    // 1. Resolve the head. Only `Exact` is supported in this task.
    let tag = match head {
        Head::Exact(t) => *t,
        Head::Any | Head::OneOf(_) => {
            return Err(syn::Error::new(
                Span::call_site(),
                "node_pattern!: head not yet supported",
            ));
        }
    };

    // 2. Look up the per-NodeKind structural schema.
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

    // 3. Reject `...` rest children (Task 7 territory).
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

    // 4. Child-count checks.
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

    // 5. Allocate a fresh binding ident per slot and build the destructuring
    //    pattern. Every v1 schema covers all fields, so no trailing `..`.
    let bindings: Vec<Ident> = schema.slots.iter().map(|_| gensym(ctx, "__b")).collect();
    let variant = Ident::new(schema.variant, Span::call_site());
    let field_pats: Vec<TokenStream> = schema
        .slots
        .iter()
        .zip(&bindings)
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
    let fail = fail_stmt(ctx);
    // A struct variant lists `field: binding`; a tuple variant lists the
    // positional holes inside `( )`. Task 4 only has struct variants.
    let destructure = match schema.slots.first().map(|s| s.field) {
        Some(FieldRef::Pos(..)) => quote! {
            let ::murphy_ast::NodeKind::#variant(#(#field_pats),*) = *cx.kind(#subject) else {
                #fail
            };
        },
        _ => quote! {
            let ::murphy_ast::NodeKind::#variant { #(#field_pats),* } = *cx.kind(#subject) else {
                #fail
            };
        },
    };

    let mut guards: Vec<TokenStream> = vec![destructure];

    // 6. Match each fixed slot against its pattern child.
    for (slot, (bind, child)) in schema
        .slots
        .iter()
        .take(fixed_count)
        .zip(bindings.iter().zip(children))
    {
        match slot.ty {
            SlotTy::Node => {
                guards.push(lower_pat(child, &quote!(#bind), ctx)?);
            }
            SlotTy::OptNode => {
                if matches!(child.kind, PatKind::NilTest) {
                    let n = gensym(ctx, "__n");
                    guards.push(quote! {
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
                    });
                } else {
                    let n = gensym(ctx, "__n");
                    guards.push(quote! {
                        let ::core::option::Option::Some(#n) = #bind.get() else {
                            #fail
                        };
                    });
                    guards.push(lower_pat(child, &quote!(#n), ctx)?);
                }
            }
            SlotTy::Sym => match &child.kind {
                PatKind::Wildcard => {}
                PatKind::Lit(Lit::Sym(s)) => {
                    let s = s.as_str();
                    guards.push(quote! {
                        if cx.symbol_str(#bind) != #s {
                            #fail
                        }
                    });
                }
                _ => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "node_pattern!: symbol slot only accepts a `:sym` literal or `_`",
                    ));
                }
            },
            SlotTy::List => unreachable!("List slot is excluded from fixed slots"),
        }
    }

    // 7. A trailing `List` slot consumes the remaining (explicit-only)
    //    children. `...` rest is rejected above; Task 7 adds it.
    if has_list {
        let list_bind = &bindings[bindings.len() - 1];
        let list_children = &children[fixed_count..];
        let list_val = gensym(ctx, "__list");
        let len = list_children.len();
        guards.push(quote! {
            let #list_val = cx.list(#list_bind);
            if #list_val.len() != #len {
                #fail
            }
        });
        for (i, child) in list_children.iter().enumerate() {
            guards.push(lower_pat(child, &quote!(#list_val[#i]), ctx)?);
        }
    }

    Ok(quote!({ #(#guards)* }))
}
