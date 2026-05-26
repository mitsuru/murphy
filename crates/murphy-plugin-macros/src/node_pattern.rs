//! `node_pattern!` — the B backend of Murphy's pattern mechanism
//! (murphy-9cr.18). Lowers an S-expression pattern to a Rust matcher
//! `fn` at compile time. See
//! `docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md`.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

use std::collections::HashMap;

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
            CaptureKind::Node => quote!(::murphy_plugin_api::NodeId),
            CaptureKind::Seq => quote!(&'a [::murphy_plugin_api::NodeId]),
            CaptureKind::OptNode => {
                quote!(::core::option::Option<::murphy_plugin_api::NodeId>)
            }
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
        next: 0,
        capture_kinds: ast.capture_kinds().to_vec(),
        local_caps: HashMap::new(),
    };
    let body = lower_pat(&ast.root, &quote!(node), &mut ctx)?;

    Ok(quote! {
        // A capture matcher's `OptNode`-slot lowering emits `let Some(n) =
        // slot.get() else { return None; }`, which clippy wants rewritten
        // with `?`. The rewrite is not uniformly valid — a zero-capture
        // matcher returns `bool`, not `Option` — so silence the lint here.
        #[allow(clippy::question_mark)]
        fn #name<'a>(
            node: ::murphy_plugin_api::NodeId,
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

/// Emit the assignment statement for capture `slot`. Consults
/// `ctx.local_caps`: if the slot was redirected to a local
/// `Option<T>` binding by a backtracking scope (murphy-ycx
/// [`lower_quantifier_list`]), emit `<local> = Some(value);`; otherwise
/// emit `__cap{slot} = value;` to the function-level binding.
fn capture_assign(slot: u16, value: TokenStream, ctx: &Lower) -> TokenStream {
    if let Some(local) = ctx.local_caps.get(&slot) {
        quote!(#local = ::core::option::Option::Some(#value);)
    } else {
        let cap = cap_ident(slot as usize);
        quote!(#cap = #value;)
    }
}

/// Mutable state threaded through the recursive lowering.
struct Lower {
    /// The expression a failed guard returns (`false` or `None`).
    fail: TokenStream,
    /// Monotonic counter feeding [`gensym`]; guarantees unique binding
    /// identifiers across recursion depth so nested `(send (send ...) ...)`
    /// matches do not shadow each other's `__b*` / `__n*` / `__list*`.
    next: usize,
    /// Per-capture-slot kinds (indexed by slot id). Populated from
    /// `PatternAst::capture_kinds()` at entry to [`lower_matcher`] so the
    /// quantifier driver can declare a typed `__lcap{slot}: Option<T>`
    /// without re-deriving the type from the pattern.
    capture_kinds: Vec<CaptureKind>,
    /// Captures redirected to local `Option<T>` bindings inside a
    /// backtracking scope (murphy-ycx). When a slot is present here,
    /// [`capture_assign_tokens`] writes `<ident> = Some(value);` instead
    /// of `__cap{slot} = value;`. The quantifier driver pops the entry
    /// on exit and commits the local Options to the outer captures on
    /// the successful path.
    local_caps: HashMap<u16, Ident>,
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
    /// `Symbol` field — accepts `_`, a single `:sym` literal, or a
    /// `{:a :b ...}` union of `:sym` literals (murphy-rs7).
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

// `Lvar(Symbol)` / `Ivar(Symbol)` / `Cvar(Symbol)` / `Gvar(Symbol)`: tuple
// variants, arity 1, index 0. The single field is a `Symbol` payload — a
// pattern child supplied at the sym slot filters on the variable name
// (`(gvar :$stdout)` matches a `Gvar(:$stdout)` only, `(gvar _)` accepts
// any name). murphy-o5k promotes these from "atoms with no sub-pattern"
// to a one-slot `(name <sym-pattern>)` form.
static VAR_SYM_SLOTS: &[Slot] = &[Slot {
    field: FieldRef::Pos(1, 0),
    ty: SlotTy::Sym,
}];

/// The full v1 `node_pattern!` schema table, keyed by `NodeKindTag` `u8`.
///
/// The `u8` tags are the `NodeKind` discriminants. The source of truth for
/// the tag ↔ pattern-name mapping is `crates/murphy-ast/src/kinds.rs`
/// `KIND_PATTERN_NAMES` (and the `NodeKind::tag()` `match` in
/// `crates/murphy-ast/src/node.rs`). A future renumber must update those
/// tables and this one together; the `schema_tags_match_pattern_names`
/// unit test below guards the link.
static SCHEMA_TABLE: &[(u8, KindSchema)] = &[
    // Variable-read atoms with a `Symbol` payload — see `VAR_SYM_SLOTS`.
    (
        9,
        KindSchema {
            variant: "Lvar",
            slots: VAR_SYM_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        10,
        KindSchema {
            variant: "Ivar",
            slots: VAR_SYM_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        11,
        KindSchema {
            variant: "Cvar",
            slots: VAR_SYM_SLOTS,
            covers_all_fields: true,
        },
    ),
    (
        12,
        KindSchema {
            variant: "Gvar",
            slots: VAR_SYM_SLOTS,
            covers_all_fields: true,
        },
    ),
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

/// Example literal pattern for an atom kind with a value form
/// (`5` for `int`, `:foo` for `sym`, …). `None` for `self`, the only
/// remaining atom with no literal form, and for any non-atom name.
/// Used after [`is_atom_kind_name`] to choose between the two
/// rejection phrasings in [`unsupported_node_match_error`].
///
/// `lvar`/`ivar`/`cvar`/`gvar` were promoted to one-slot kinds with a
/// `Symbol` sub-pattern (murphy-o5k), so they no longer reach this
/// function — `schema_for` returns `Some` for them.
fn atom_literal_example(name: &str) -> Option<&'static str> {
    Some(match name {
        "int" => "5",
        "float" => "1.0",
        "str" => "\"s\"",
        "sym" => ":foo",
        "true" => "true",
        "false" => "false",
        "nil" => "nil",
        // `self`: atom with no literal form.
        _ => return None,
    })
}

/// `true` iff `name` is one of the 8 remaining atom node kinds. Pair
/// with [`atom_literal_example`] when building the rejection diagnostic
/// for an atom written in the unsupported `(name …)` node-match form.
fn is_atom_kind_name(name: &str) -> bool {
    matches!(
        name,
        "nil" | "true" | "false" | "self" | "int" | "float" | "str" | "sym"
    )
}

/// Build the `compile_error!` for a node-match head whose kind has no
/// `SCHEMA_TABLE` entry. Atoms (`int`, `self`, …) get a kind-specific
/// hint pointing at the literal or bare-kind alternative; other unsupported
/// kinds (e.g. `rescue`) get the generic "follow-up issue" diagnostic.
fn unsupported_node_match_error(tag: murphy_ast::NodeKindTag) -> syn::Error {
    let name = murphy_ast::pattern_name(tag).unwrap_or("?");
    let msg = if is_atom_kind_name(name) {
        match atom_literal_example(name) {
            Some(lit) => format!(
                "node_pattern!: atom kind `{name}` cannot be matched as \
                 `({name} ...)` — use literal `{lit}` or bare kind name `{name}`"
            ),
            None => format!(
                "node_pattern!: atom kind `{name}` cannot be matched as \
                 `({name} ...)` — use bare kind name `{name}`"
            ),
        }
    } else {
        format!(
            "node_pattern!: node kind `{name}` is not supported by \
             node_pattern! in v1 — see follow-up issue"
        )
    };
    syn::Error::new(Span::call_site(), msg)
}

/// Parse a `#predicate` name into a callable Rust identifier.
///
/// A `#name` resolves to a free function in scope at the `node_pattern!`
/// call site. Ruby-style `?` / `!` suffixes are mangled so the call site
/// uses a valid Rust identifier (murphy-bj7):
///
/// - `#odd?` → calls `odd_p` (predicate; `_p` matches the mruby
///   `_p_method` C-binding convention).
/// - `#save!` → calls `save_bang` (Ruby's "bang method" idiom).
/// - `#save` → calls `save` (unchanged).
///
/// `#save?` and `#save` therefore resolve to *different* Rust fns,
/// matching the Rails idiom where `save` and `save?` are distinct
/// methods. Ruby setter names (`foo=`) are deliberately rejected: the
/// `=` has no canonical Rust counterpart, and a `#foo=` predicate has
/// no use case the EPIC needs to cover.
fn predicate_ident(name: &str) -> syn::Result<Ident> {
    let mangled = if let Some(stem) = name.strip_suffix('?') {
        format!("{stem}_p")
    } else if let Some(stem) = name.strip_suffix('!') {
        format!("{stem}_bang")
    } else {
        name.to_string()
    };
    syn::parse_str::<Ident>(&mangled).map_err(|_| {
        // The lexer constrains `#name` to a Ruby-method-shaped identifier
        // optionally suffixed by `?`/`!`/`=`. After stripping `?`/`!`
        // above, only the `=` (setter) case can still fail Rust ident
        // parsing — reported with the original source name for clarity.
        syn::Error::new(
            Span::call_site(),
            format!(
                "node_pattern!: predicate name `{name}` is not a valid Rust \
                 identifier; `?`/`!` are mangled to `_p`/`_bang`, but other \
                 Ruby suffixes (e.g. `=`) have no Rust counterpart"
            ),
        )
    })
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
                        ::murphy_plugin_api::NodeKind::Int(__v) if __v == #v
                    ) {
                        #fail
                    }
                },
                Lit::Float(v) => quote! {
                    if let ::murphy_plugin_api::NodeKind::Float(__v) = *cx.kind(#subject) {
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
                            ::murphy_plugin_api::NodeKind::Str(__id) if cx.string_str(__id) == #s
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
                            ::murphy_plugin_api::NodeKind::Sym(__sym) if cx.symbol_str(__sym) == #s
                        ) {
                            #fail
                        }
                    }
                }
                Lit::True => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_plugin_api::NodeKind::True_
                    ) {
                        #fail
                    }
                },
                Lit::False => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_plugin_api::NodeKind::False_
                    ) {
                        #fail
                    }
                },
                Lit::Nil => quote! {
                    if !::core::matches!(
                        *cx.kind(#subject),
                        ::murphy_plugin_api::NodeKind::Nil
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
                if cx.kind(#subject).tag() != ::murphy_plugin_api::NodeKindTag(#tag_u8) {
                    #fail
                }
            })
        }
        PatKind::NilTest => {
            let fail = fail_stmt(ctx);
            Ok(quote! {
                if !::core::matches!(
                    *cx.kind(#subject),
                    ::murphy_plugin_api::NodeKind::Nil
                ) {
                    #fail
                }
            })
        }
        PatKind::Node { head, children } => lower_node(head, children, subject, ctx),
        PatKind::Union(alts) => {
            // Each alternative lowers to a `return`-free bool expression; the
            // node matches the union iff any arm's expression is true.
            let alt_bools: Vec<TokenStream> = alts
                .iter()
                .map(|alt| lower_bool(alt, subject, ctx))
                .collect::<syn::Result<_>>()?;
            let fail = fail_stmt(ctx);
            let ok = gensym(ctx, "__ok");
            Ok(quote! {
                let #ok: bool = ( #(#alt_bools)||* );
                if !#ok {
                    #fail
                }
            })
        }
        PatKind::Not(inner) => {
            // `!x` matches iff `x` does not — lower `x` to a bool expression
            // and fail when it holds.
            let inner_bool = lower_bool(inner, subject, ctx)?;
            let fail = fail_stmt(ctx);
            Ok(quote! {
                if #inner_bool {
                    #fail
                }
            })
        }
        PatKind::Capture {
            slot,
            name: _,
            body,
        } => {
            // A `$...` seq capture (`Capture` whose `body` is `Rest`) is
            // only valid inside a node's variable-length child list, where
            // `lower_trailing_list` intercepts it. Reaching this arm means
            // `$...` appeared at a fixed slot position — a position error.
            if matches!(body.kind, PatKind::Rest) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "node_pattern!: `$...` seq capture is only allowed in a \
                     node's variable-length child list (e.g. `send`/`csend` \
                     args, `array`/`hash`/`begin` elements)",
                ));
            }
            // Node capture: lower the body's guards first (so a mismatch
            // returns `ctx.fail` before the slot is written), then assign
            // the captured node id into the deferred-init capture variable.
            let body_guards = lower_pat(body, subject, ctx)?;
            let assign = capture_assign(*slot, quote!(#subject), ctx);
            Ok(quote!(#body_guards #assign))
        }
        PatKind::Predicate(name) => {
            // `#name` calls a free fn `name(node, cx) -> bool` in scope at the
            // call site. Fail the guard when the predicate returns `false`.
            let ident = predicate_ident(name)?;
            let fail = fail_stmt(ctx);
            Ok(quote! {
                if !#ident(#subject, cx) {
                    #fail
                }
            })
        }
        PatKind::Parent(inner) => {
            // `^x` — bind the parent (fail if absent), then match `inner`
            // against it. The parent direction is unique, so definite
            // assignment is preserved and `inner` may capture: lower it via
            // the `lower_pat` (guard) route.
            let p = gensym(ctx, "__p");
            let fail = fail_stmt(ctx);
            let inner_guards = lower_pat(inner, &quote!(#p), ctx)?;
            Ok(quote! {
                let ::core::option::Option::Some(#p) = cx.parent(#subject).get() else {
                    #fail
                };
                #inner_guards
            })
        }
        PatKind::Descend(inner) => {
            // `` `x `` — succeed iff some descendant matches `inner`. The
            // descendant scan visits many nodes, so `inner` cannot capture;
            // lower it via `lower_bool` (which structurally rejects captures)
            // into a bool expression over the per-descendant binding.
            let d = gensym(ctx, "__d");
            let inner_bool = lower_bool(inner, &quote!(#d), ctx)?;
            let hit = gensym(ctx, "__hit");
            let fail = fail_stmt(ctx);
            Ok(quote! {
                let #hit = cx.descendants(#subject).into_iter().any(|#d| #inner_bool);
                if !#hit {
                    #fail
                }
            })
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
    // 1. Look up the per-NodeKind structural schema.
    let schema = schema_for(tag.0).ok_or_else(|| unsupported_node_match_error(tag))?;

    // Split the schema into fixed slots and an optional trailing `List`.
    let has_list = schema
        .slots
        .last()
        .is_some_and(|s| matches!(s.ty, SlotTy::List));
    let fixed_count = schema.slots.len() - usize::from(has_list);

    // 2. Child-count checks. A rest-like element among the `List`-slot
    //    children stands for zero-or-more nodes, so when a `List` slot is
    //    present the exact-count check is relaxed (it is re-derived inside
    //    `lower_trailing_list`). Rest-like elements at *fixed* slot positions
    //    are not special-cased here: they flow into `lower_fixed_slot`, which
    //    rejects them. `...` reaching a `List`-less node also reaches a fixed
    //    slot and is rejected the same way.
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

    // 3. Allocate a fresh binding ident per slot and build the destructuring.
    let bindings: Vec<Ident> = schema.slots.iter().map(|_| gensym(ctx, "__b")).collect();
    let mut guards: Vec<TokenStream> = vec![build_destructure(schema, &bindings, subject, ctx)];

    // 4. Match each fixed slot against its pattern child.
    for (slot, (bind, child)) in schema
        .slots
        .iter()
        .take(fixed_count)
        .zip(bindings.iter().zip(children))
    {
        guards.push(lower_fixed_slot(slot.ty, bind, child, ctx)?);
    }

    // 5. A trailing `List` slot consumes the remaining children, including a
    //    `...` / `$...` rest-like element among them.
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
            let ::murphy_plugin_api::NodeKind::#variant(#(#field_pats),*) = *cx.kind(#subject) else {
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
                let ::murphy_plugin_api::NodeKind::#variant { #(#field_pats),* #rest } = *cx.kind(#subject) else {
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
    use murphy_pattern::PatKind;
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
                                ::murphy_plugin_api::NodeKind::Nil
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
        SlotTy::Sym => {
            let syms = sym_slot_alternatives(child)?;
            // One arm: `if cx.symbol_str(b) != "x" { fail }`.
            // Many arms: `if !matches!(cx.symbol_str(b), "a" | "b") { fail }`.
            // Empty: the wildcard child returns an empty list — emit no guard.
            if syms.is_empty() {
                return Ok(quote!());
            }
            if syms.len() == 1 {
                let s = syms[0];
                return Ok(quote! {
                    if cx.symbol_str(#bind) != #s {
                        #fail
                    }
                });
            }
            Ok(quote! {
                if !::core::matches!(cx.symbol_str(#bind), #(#syms)|*) {
                    #fail
                }
            })
        }
        SlotTy::List => unreachable!("List slot is excluded from fixed slots"),
    }
}

/// Classify a sym-slot pattern child into a flat list of accepted name
/// strings. Used by both [`lower_fixed_slot`] and [`lower_bool_fixed_slot`]
/// to share the wildcard / `:sym` literal / `{:a :b ...}` union surface
/// (murphy-rs7). Returns an empty `Vec` for a wildcard (the slot
/// matches any name and emits no guard) and a one-element `Vec` for a
/// single literal. A union must hold only `:sym` literals — any other
/// arm is a span-carrying compile error.
fn sym_slot_alternatives(child: &murphy_pattern::Pat) -> syn::Result<Vec<&str>> {
    use murphy_pattern::{Lit, PatKind};
    match &child.kind {
        PatKind::Wildcard => Ok(Vec::new()),
        PatKind::Lit(Lit::Sym(s)) => Ok(vec![s.as_str()]),
        PatKind::Union(alts) => {
            let mut out = Vec::with_capacity(alts.len());
            for alt in alts {
                match &alt.kind {
                    PatKind::Lit(Lit::Sym(s)) => out.push(s.as_str()),
                    _ => {
                        return Err(syn::Error::new(
                            Span::call_site(),
                            "node_pattern!: symbol slot union `{...}` only \
                             accepts `:sym` literals",
                        ));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: symbol slot only accepts a `:sym` literal, `_`, \
             or a `{:sym :sym ...}` union of `:sym` literals",
        )),
    }
}

/// A rest-like `List`-slot pattern child: a bare `...` ([`PatKind::Rest`]) or
/// a `$...` seq capture (a [`PatKind::Capture`] whose body is `Rest`).
enum RestKind {
    /// Bare `...` — matches zero-or-more nodes, binds nothing.
    Bare,
    /// `$...` — matches zero-or-more nodes, binds the slice to capture `slot`.
    Capture(u16),
}

/// Classify a `List`-slot pattern child as rest-like, if it is. The
/// murphy-pattern parser guarantees at most one rest-like element per node
/// child list, so the caller stops at the first hit.
fn rest_kind(pat: &murphy_pattern::Pat) -> Option<RestKind> {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Rest => Some(RestKind::Bare),
        PatKind::Capture { slot, body, .. } if matches!(body.kind, PatKind::Rest) => {
            Some(RestKind::Capture(*slot))
        }
        _ => None,
    }
}

/// Lower a trailing `List` slot: resolve the bound `NodeList` and match the
/// `List`-slot pattern children against its elements.
///
/// With no rest-like child the list length must match exactly and each
/// pattern child matches the element at its index (Task 5 behaviour). With a
/// rest-like child at index `r` (a `...` or `$...`, at most one — guaranteed
/// by the parser), the `k - 1` non-rest children split into an `r`-element
/// prefix and a `k - 1 - r`-element suffix: the prefix matches the leading
/// elements, the suffix matches the *trailing* elements, and the span between
/// them is the rest. A `$...` binds that span; a bare `...` binds nothing.
///
/// When any child is a postfix `*` / `+` / `?` quantifier (murphy-ycx), the
/// list switches to a backtracking driver that mirrors the C matcher's
/// `match_list_from` (PR #3 / #76). See [`lower_quantifier_list`].
fn lower_trailing_list(
    list_bind: &Ident,
    list_children: &[murphy_pattern::Pat],
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    if has_quantifier_child(list_children) {
        return lower_quantifier_list(list_bind, list_children, ctx);
    }
    let fail = fail_stmt(ctx);
    let list_val = gensym(ctx, "__list");

    // Locate the (at most one) rest-like child.
    let rest_at = list_children.iter().position(|c| rest_kind(c).is_some());

    let Some(r) = rest_at else {
        // No rest: exact length, indexed matches (Task 5 behaviour).
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
        return Ok(quote!(#(#guards)*));
    };

    // A rest-like child at index `r`. `k` is the total child count; `k - 1`
    // are non-rest (`r` prefix + `suffix_count` suffix). `r < k`, so
    // `k - 1 - r` does not underflow.
    let k = list_children.len();
    let non_rest = k - 1;
    let suffix_count = non_rest - r;
    // `RestKind` was already shown to be `Some` by `position` above.
    let rest = rest_kind(&list_children[r]).expect("rest_at points at a rest-like child");

    let mut guards: Vec<TokenStream> = vec![quote! {
        let #list_val = cx.list(#list_bind);
    }];

    // Length guard: there are `non_rest` non-rest children to place. When
    // `non_rest == 0` the guard would be `len < 0` (always false) — skip it.
    if non_rest > 0 {
        guards.push(quote! {
            if #list_val.len() < #non_rest {
                #fail
            }
        });
    }

    // Bind the length once: the suffix index and a suffix-bounded rest span
    // are computed against it. With no suffix the length is never needed —
    // the suffix loop is empty and the rest span runs to the end (`..`) — so
    // the `__len` ident is only gensym'd and bound when `suffix_count > 0`.
    let len_val = if suffix_count > 0 {
        let len_val = gensym(ctx, "__len");
        guards.push(quote! {
            let #len_val = #list_val.len();
        });
        Some(len_val)
    } else {
        None
    };

    // Prefix: `lp[i]` matches `list[i]` for `i in 0..r`.
    for (i, child) in list_children.iter().take(r).enumerate() {
        guards.push(lower_pat(child, &quote!(#list_val[#i]), ctx)?);
    }

    // Suffix: the `suffix_count` children after the rest match the *last*
    // `suffix_count` elements. `lp[r + 1 + j]` matches `list[len - back]`
    // where `back = suffix_count - j` runs from `suffix_count` down to `1`
    // (never `0`, so no `len - 0` identity-op lint). This loop only runs
    // when `suffix_count > 0`, so `len_val` is `Some` here.
    for (j, child) in list_children.iter().skip(r + 1).enumerate() {
        let back = suffix_count - j;
        guards.push(lower_pat(child, &quote!(#list_val[#len_val - #back]), ctx)?);
    }

    // Middle: the rest span `list[r .. len - suffix_count]`. Only a `$...`
    // capture needs it bound; a bare `...` matches nothing here.
    if let RestKind::Capture(slot) = rest {
        // Shape the slice expression to avoid `len - 0` / `[0..]` lints.
        // The `_, _` arms only fire when `suffix_count > 0`, so `len_val`
        // is `Some` whenever it is interpolated.
        let span = match (r, suffix_count) {
            (0, 0) => quote!(&#list_val[..]),
            (0, _) => quote!(&#list_val[..#len_val - #suffix_count]),
            (_, 0) => quote!(&#list_val[#r..]),
            (_, _) => quote!(&#list_val[#r..#len_val - #suffix_count]),
        };
        guards.push(capture_assign(slot, span, ctx));
    }

    Ok(quote!(#(#guards)*))
}

/// True iff any child carries a postfix `*` / `+` / `?` quantifier — directly
/// or wrapped by a single `$` capture (`$pat+` etc.). The parser forbids any
/// other nesting (`($pat)+`, double-`Capture`, `Quantifier` inside
/// `Quantifier` body), so the classification is unambiguous.
fn has_quantifier_child(children: &[murphy_pattern::Pat]) -> bool {
    use murphy_pattern::PatKind;
    children.iter().any(|c| {
        let inner = match &c.kind {
            PatKind::Capture { body, .. } => &body.kind,
            other => other,
        };
        matches!(inner, PatKind::Quantifier { .. })
    })
}

/// A pattern child classified into the three list-element shapes the
/// backtracker drives: a Fixed element (one-elem-one-pat), a Rest span
/// (`...` or `$...`, zero-or-more), or a Quantifier (`pat*` / `pat+` /
/// `pat?` with optional `$` capture).
enum ListKid<'a> {
    Fixed(&'a murphy_pattern::Pat),
    Rest(RestKind),
    Quantifier(QuantifierPat<'a>),
}

/// A classified quantifier child: its body, arity bounds, and optional
/// capture slot. `is_optional` mirrors the C matcher's interpretation —
/// `min == 0 && max == Some(1)` selects the `OptNode` capture shape over
/// `Seq`.
struct QuantifierPat<'a> {
    body: &'a murphy_pattern::Pat,
    min: u8,
    /// `None` for the unbounded `*` / `+` arity (`u8::MAX` in the AST).
    max: Option<u8>,
    capture_slot: Option<u16>,
    is_optional: bool,
}

fn classify_list_kid(pat: &murphy_pattern::Pat) -> ListKid<'_> {
    use murphy_pattern::PatKind;
    if let Some(r) = rest_kind(pat) {
        return ListKid::Rest(r);
    }
    let (cap_slot, q_pat) = match &pat.kind {
        PatKind::Quantifier { .. } => (None, pat),
        PatKind::Capture { slot, body, .. } if matches!(body.kind, PatKind::Quantifier { .. }) => {
            (Some(*slot), body.as_ref())
        }
        _ => return ListKid::Fixed(pat),
    };
    let PatKind::Quantifier { body, min, max } = &q_pat.kind else {
        unreachable!("classified as Quantifier above")
    };
    let max_opt = (*max != u8::MAX).then_some(*max);
    let is_optional = *min == 0 && max_opt == Some(1);
    ListKid::Quantifier(QuantifierPat {
        body: body.as_ref(),
        min: *min,
        max: max_opt,
        capture_slot: cap_slot,
        is_optional,
    })
}

/// Lower a trailing `List` slot whose children include at least one
/// quantifier. Emits a closure-wrapped backtracking driver that mirrors
/// `murphy_pattern::matcher::match_list_from` (PR #3 / #76).
///
/// - The driver returns `Some(())` on a full match, `None` on exhaustion.
/// - On failure the outer guard returns the caller's `ctx.fail` (so `bool` /
///   `Option<(..)>` matchers both bail correctly).
/// - Every capture bound *within* the list is redirected to a local
///   `Option<T>` binding (re-assignable across backtrack attempts) via
///   `ctx.local_caps`. On the successful path the locals are unwrapped into
///   the function-level `__cap{slot}` variables.
fn lower_quantifier_list(
    list_bind: &Ident,
    list_children: &[murphy_pattern::Pat],
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let outer_fail = fail_stmt(ctx);
    let list_val = gensym(ctx, "__list");
    let len_val = gensym(ctx, "__len");
    let attempt = gensym(ctx, "__attempt");

    // 1. Collect every capture slot reachable from this list's children
    //    and redirect each to a local `Option<T>` binding. The locals are
    //    declared outside the closure so the closure can re-assign them
    //    across backtrack attempts. Slots register in insertion order so
    //    the commit step is deterministic.
    let mut slots: Vec<u16> = Vec::new();
    for child in list_children {
        collect_capture_slots(child, &mut slots);
    }
    // Each slot ID may legitimately repeat across positions in the parser
    // surface (the parser deduplicates them later); keep only the first
    // occurrence so we declare one local per slot.
    let mut seen = std::collections::BTreeSet::new();
    slots.retain(|s| seen.insert(*s));

    // Slot id -> (local ident, capture kind, original `__cap{id}` ident).
    let mut local_specs: Vec<(u16, Ident, CaptureKind, Ident)> = Vec::with_capacity(slots.len());
    let mut local_decls: Vec<TokenStream> = Vec::with_capacity(slots.len());
    for slot in &slots {
        let kind = ctx
            .capture_kinds
            .get(*slot as usize)
            .copied()
            .expect("slot id within capture_kinds range");
        let ty = capture_kind_ty(kind);
        let local = Ident::new(&format!("__lcap{slot}"), proc_macro2::Span::call_site());
        local_decls.push(
            quote!(let mut #local: ::core::option::Option<#ty> = ::core::option::Option::None;),
        );
        local_specs.push((*slot, local, kind, cap_ident(*slot as usize)));
    }

    // 2. Register the redirects, swap `ctx.fail` for the closure scope,
    //    emit the recursive driver, then restore.
    for (slot, local, _, _) in &local_specs {
        ctx.local_caps.insert(*slot, local.clone());
    }
    let saved_fail = std::mem::replace(&mut ctx.fail, quote!(::core::option::Option::None));
    let body = emit_list_step(list_children, &quote!(0usize), &list_val, &len_val, ctx);
    ctx.fail = saved_fail;
    for slot in &slots {
        ctx.local_caps.remove(slot);
    }
    let body = body?;

    // 3. On the successful path, unwrap each local into the function-level
    //    capture. The locals are `Some` because every successful path
    //    inside the driver wrote them before returning `Some(())`.
    let commits: Vec<TokenStream> = local_specs
        .iter()
        .map(|(_, local, _, outer)| {
            quote!(#outer = #local.expect("capture written on successful match path");)
        })
        .collect();

    Ok(quote! {
        let #list_val = cx.list(#list_bind);
        let #len_val: usize = #list_val.len();
        #(#local_decls)*
        let #attempt: ::core::option::Option<()> = (|| -> ::core::option::Option<()> {
            #body
        })();
        if #attempt.is_none() {
            #outer_fail
        }
        #(#commits)*
    })
}

/// Map a [`CaptureKind`] to the Rust type the matcher binds the capture
/// variable to (matches the type emitted by [`lower_matcher`]). Used by
/// [`lower_quantifier_list`] to declare each local `Option<T>` shadow.
fn capture_kind_ty(kind: CaptureKind) -> TokenStream {
    match kind {
        CaptureKind::Node => quote!(::murphy_plugin_api::NodeId),
        CaptureKind::Seq => quote!(&'a [::murphy_plugin_api::NodeId]),
        CaptureKind::OptNode => {
            quote!(::core::option::Option<::murphy_plugin_api::NodeId>)
        }
    }
}

/// Walk `pat` and push every capture slot id reached (including captures
/// nested inside `Capture` bodies — the slot of an outer `$pat` and any
/// inner `$inner` are both written on a successful path).
fn collect_capture_slots(pat: &murphy_pattern::Pat, out: &mut Vec<u16>) {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Capture { slot, body, .. } => {
            out.push(*slot);
            collect_capture_slots(body, out);
        }
        PatKind::Node { children, .. } => {
            for c in children {
                collect_capture_slots(c, out);
            }
        }
        PatKind::Union(alts) => {
            for a in alts {
                collect_capture_slots(a, out);
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            collect_capture_slots(b, out);
        }
        PatKind::Quantifier { body, .. } => collect_capture_slots(body, out),
        PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate(_)
        | PatKind::Kind(_)
        | PatKind::Rest => {}
    }
}

/// Recursively emit the backtracker for the remaining list children at
/// `cursor_expr` (an expression of type `usize` naming the current
/// position in `list_val`). Returns a token stream that, inside an
/// `Option<()>`-returning closure, either falls through to the next step
/// or `return`s `Some(())` / `None`.
fn emit_list_step(
    kids: &[murphy_pattern::Pat],
    cursor_expr: &TokenStream,
    list_val: &Ident,
    len_val: &Ident,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let Some((first, rest_kids)) = kids.split_first() else {
        // Base case: every kid placed. The list must be fully consumed.
        return Ok(quote! {
            if (#cursor_expr) == #len_val {
                return ::core::option::Option::Some(());
            }
            return ::core::option::Option::None;
        });
    };

    match classify_list_kid(first) {
        ListKid::Fixed(pat) => emit_fixed_step(pat, rest_kids, cursor_expr, list_val, len_val, ctx),
        ListKid::Rest(rk) => emit_rest_step(rk, rest_kids, cursor_expr, list_val, len_val, ctx),
        ListKid::Quantifier(q) => {
            emit_quantifier_step(q, rest_kids, cursor_expr, list_val, len_val, ctx)
        }
    }
}

/// Emit a fixed-element step: bind `list[cursor]`, run the per-element
/// guards via [`lower_pat`] (whose failures route to `ctx.fail = None`),
/// then recurse with `cursor + 1`.
fn emit_fixed_step(
    pat: &murphy_pattern::Pat,
    rest_kids: &[murphy_pattern::Pat],
    cursor_expr: &TokenStream,
    list_val: &Ident,
    len_val: &Ident,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let cur = gensym(ctx, "__cur");
    let elem = gensym(ctx, "__elem");
    let guards = lower_pat(pat, &quote!(#elem), ctx)?;
    let next = emit_list_step(rest_kids, &quote!(#cur + 1), list_val, len_val, ctx)?;
    Ok(quote! {
        let #cur: usize = #cursor_expr;
        if #cur >= #len_val {
            return ::core::option::Option::None;
        }
        let #elem = #list_val[#cur];
        #guards
        #next
    })
}

/// Emit a rest step (`...` / `$...`): greedily try every span length from
/// the remaining tail down to `0`, attempting the suffix per length. A
/// `$...` commits its captured slice on the first successful attempt.
fn emit_rest_step(
    rest: RestKind,
    rest_kids: &[murphy_pattern::Pat],
    cursor_expr: &TokenStream,
    list_val: &Ident,
    len_val: &Ident,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let cur = gensym(ctx, "__cur");
    let cnt = gensym(ctx, "__cnt");
    let sub = gensym(ctx, "__sub");
    let suffix_cursor = quote!(#cur + #cnt);
    let suffix = emit_list_step(rest_kids, &suffix_cursor, list_val, len_val, ctx)?;
    let commit = match rest {
        RestKind::Bare => quote!(),
        RestKind::Capture(slot) => capture_assign(slot, quote!(&#list_val[#cur..#cur + #cnt]), ctx),
    };
    Ok(quote! {
        let #cur: usize = #cursor_expr;
        let __remaining: usize = #len_val - #cur;
        for #cnt in (0..=__remaining).rev() {
            let #sub: ::core::option::Option<()> = (|| -> ::core::option::Option<()> {
                #suffix
            })();
            if #sub.is_some() {
                #commit
                return ::core::option::Option::Some(());
            }
        }
        return ::core::option::Option::None;
    })
}

/// Emit a quantifier step: greedily count how many leading elements
/// satisfy the body (as a `lower_bool` expression — captures and rest are
/// parser-forbidden inside a quantifier body), then iterate counts from
/// the greedy max down to `min`, attempting the suffix per count. On the
/// first successful attempt, commit the optional capture (`Seq` for
/// `*`/`+`, `OptNode` for `?`).
fn emit_quantifier_step(
    q: QuantifierPat<'_>,
    rest_kids: &[murphy_pattern::Pat],
    cursor_expr: &TokenStream,
    list_val: &Ident,
    len_val: &Ident,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let cur = gensym(ctx, "__cur");
    let greedy = gensym(ctx, "__greedy");
    let count = gensym(ctx, "__count");
    let sub = gensym(ctx, "__sub");
    let elem = gensym(ctx, "__elem");
    let body_bool = lower_bool(q.body, &quote!(#elem), ctx)?;
    let suffix_cursor = quote!(#cur + #count);
    let suffix = emit_list_step(rest_kids, &suffix_cursor, list_val, len_val, ctx)?;
    let min = q.min as usize;
    let max_break = match q.max {
        Some(m) => {
            let m = m as usize;
            quote!(if #greedy >= #m { break; })
        }
        None => quote!(),
    };
    let commit = match q.capture_slot {
        Some(slot) => {
            if q.is_optional {
                let value = quote! {
                    if #count == 1 {
                        ::core::option::Option::Some(#list_val[#cur])
                    } else {
                        ::core::option::Option::None
                    }
                };
                capture_assign(slot, value, ctx)
            } else {
                capture_assign(slot, quote!(&#list_val[#cur..#cur + #count]), ctx)
            }
        }
        None => quote!(),
    };
    Ok(quote! {
        let #cur: usize = #cursor_expr;
        let mut #greedy: usize = 0;
        while #cur + #greedy < #len_val {
            let #elem = #list_val[#cur + #greedy];
            if !(#body_bool) { break; }
            #greedy += 1;
            #max_break
        }
        if #greedy < #min {
            return ::core::option::Option::None;
        }
        for #count in (#min..=#greedy).rev() {
            let #sub: ::core::option::Option<()> = (|| -> ::core::option::Option<()> {
                #suffix
            })();
            if #sub.is_some() {
                #commit
                return ::core::option::Option::Some(());
            }
        }
        return ::core::option::Option::None;
    })
}

/// Lower one `Pat` against `subject` (a `NodeId`-typed expression) into a
/// single `return`-free **bool expression**.
///
/// This is the lowering route for the inside of `{}` union, `!` negation and
/// `` ` `` descend: those positions need a value, not a guard sequence that
/// `return`s on mismatch (the `lower_pat` route). The produced expression is
/// built only from `matches!`, `&&`, `||`, `if let`/`map_or` and method
/// calls — it never contains a `return` and never touches `ctx.fail`.
///
/// v1 restriction: a `Node` pattern reachable here may use only fixed slots
/// (`Node`/`OptNode`/`Sym`). A node whose pattern carries variable-length
/// `List`-slot children is rejected with a `compile_error` — fully mirroring
/// `lower_pat`'s `List` handling in bool form would near-duplicate the Node
/// machinery and is out of v1 scope. Captures and `...` are never legal here.
fn lower_bool(
    pat: &murphy_pattern::Pat,
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Wildcard => Ok(quote!(true)),
        PatKind::Lit(lit) => Ok(lower_bool_lit(lit, subject)),
        PatKind::Kind(tag) => {
            let tag_u8 = tag.0;
            Ok(quote! {
                ( cx.kind(#subject).tag() == ::murphy_plugin_api::NodeKindTag(#tag_u8) )
            })
        }
        PatKind::NilTest => Ok(quote! {
            ( ::core::matches!(*cx.kind(#subject), ::murphy_plugin_api::NodeKind::Nil) )
        }),
        PatKind::Node { head, children } => lower_bool_node(head, children, subject, ctx),
        PatKind::Union(alts) => {
            let alt_bools: Vec<TokenStream> = alts
                .iter()
                .map(|alt| lower_bool(alt, subject, ctx))
                .collect::<syn::Result<_>>()?;
            Ok(quote!( ( #(#alt_bools)||* ) ))
        }
        PatKind::Not(inner) => {
            let inner_bool = lower_bool(inner, subject, ctx)?;
            Ok(quote!( ( !#inner_bool ) ))
        }
        PatKind::Predicate(name) => {
            // `#name` calls a free fn `name(node, cx) -> bool` in scope at the
            // call site; in value form it is simply that bool expression.
            let ident = predicate_ident(name)?;
            Ok(quote!( ( #ident(#subject, cx) ) ))
        }
        PatKind::Parent(inner) => {
            let p = gensym(ctx, "__p");
            let inner_bool = lower_bool(inner, &quote!(#p), ctx)?;
            Ok(quote! {
                ( cx.parent(#subject).get().map_or(false, |#p| #inner_bool) )
            })
        }
        PatKind::Descend(inner) => {
            let d = gensym(ctx, "__d");
            let inner_bool = lower_bool(inner, &quote!(#d), ctx)?;
            Ok(quote! {
                ( cx.descendants(#subject).into_iter().any(|#d| #inner_bool) )
            })
        }
        PatKind::Capture { .. } => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: `$` capture is not allowed inside `{}` / `!` / `` ` ``",
        )),
        PatKind::Rest => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: `...` is not valid here",
        )),
        PatKind::Quantifier { .. } => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: postfix `*` / `+` / `?` quantifier is only legal \
             as a direct child of a node match (parser-enforced; reaching \
             this arm is an internal invariant violation)",
        )),
    }
}

/// Lower a `Lit` into a `return`-free bool expression — the value-form
/// counterpart of the `Lit` arm in [`lower_pat`].
fn lower_bool_lit(lit: &murphy_pattern::Lit, subject: &TokenStream) -> TokenStream {
    use murphy_pattern::Lit;
    match lit {
        Lit::Int(v) => quote! {
            ( ::core::matches!(
                *cx.kind(#subject),
                ::murphy_plugin_api::NodeKind::Int(__v) if __v == #v
            ) )
        },
        Lit::Float(v) => quote! {
            ( if let ::murphy_plugin_api::NodeKind::Float(__v) = *cx.kind(#subject) {
                // Exact equality is intentional: the pattern author wrote a specific float literal.
                #[allow(clippy::float_cmp)]
                { __v == #v }
            } else {
                false
            } )
        },
        Lit::Str(s) => {
            let s = s.as_str();
            quote! {
                ( ::core::matches!(
                    *cx.kind(#subject),
                    ::murphy_plugin_api::NodeKind::Str(__id) if cx.string_str(__id) == #s
                ) )
            }
        }
        Lit::Sym(s) => {
            let s = s.as_str();
            quote! {
                ( ::core::matches!(
                    *cx.kind(#subject),
                    ::murphy_plugin_api::NodeKind::Sym(__sym) if cx.symbol_str(__sym) == #s
                ) )
            }
        }
        Lit::True => quote! {
            ( ::core::matches!(*cx.kind(#subject), ::murphy_plugin_api::NodeKind::True_) )
        },
        Lit::False => quote! {
            ( ::core::matches!(*cx.kind(#subject), ::murphy_plugin_api::NodeKind::False_) )
        },
        Lit::Nil => quote! {
            ( ::core::matches!(*cx.kind(#subject), ::murphy_plugin_api::NodeKind::Nil) )
        },
    }
}

/// Lower a `(head child...)` node match into a `return`-free bool expression.
///
/// `Head::Any` / `Head::OneOf` are kind-only matches (children must be empty
/// or a single `...`, reusing [`check_kind_only_children`]). `Head::Exact`
/// destructures the kind and `&&`-chains a per-fixed-slot bool sub-expression.
/// A node whose pattern carries `List`-slot children is rejected — see the
/// v1 restriction documented on [`lower_bool`].
fn lower_bool_node(
    head: &murphy_pattern::Head,
    children: &[murphy_pattern::Pat],
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::Head;
    match head {
        Head::Any => {
            check_kind_only_children(children)?;
            Ok(quote!(true))
        }
        Head::OneOf(tags) => {
            check_kind_only_children(children)?;
            let tag_u8s: Vec<u8> = tags.iter().map(|t| t.0).collect();
            Ok(quote! {
                ( ::core::matches!(cx.kind(#subject).tag().0, #(#tag_u8s)|*) )
            })
        }
        Head::Exact(t) => lower_bool_exact_node(*t, children, subject, ctx),
    }
}

/// Lower a `Head::Exact` node match into a `return`-free bool expression.
fn lower_bool_exact_node(
    tag: murphy_ast::NodeKindTag,
    children: &[murphy_pattern::Pat],
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    let schema = schema_for(tag.0).ok_or_else(|| unsupported_node_match_error(tag))?;

    let has_list = schema
        .slots
        .last()
        .is_some_and(|s| matches!(s.ty, SlotTy::List));
    let fixed_count = schema.slots.len() - usize::from(has_list);

    if children.len() < fixed_count {
        return Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: too few children",
        ));
    }
    // v1 restriction: a node whose pattern supplies `List`-slot children is
    // not supported inside `{}` / `!` / `` ` ``. An unconstrained `List` slot
    // (children fill exactly the fixed slots) is fine — the list is simply
    // left unmatched. `children.len() > fixed_count` means list children are
    // present; for a `List`-less kind it also means a plain count mismatch.
    if children.len() > fixed_count {
        if has_list {
            return Err(syn::Error::new(
                Span::call_site(),
                "node_pattern!: a node pattern with a variable-length child \
                 list is not supported inside `{}` / `!` / `` ` `` in v1",
            ));
        }
        return Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: wrong number of children",
        ));
    }

    // Bind every schema slot's field; fixed slots get a gensym binding, a
    // trailing `List` slot is bound as `_` (the list is left unconstrained).
    let variant = Ident::new(schema.variant, Span::call_site());
    let fixed_binds: Vec<Ident> = (0..fixed_count).map(|_| gensym(ctx, "__b")).collect();

    let field_pats: Vec<TokenStream> = schema
        .slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let bind: TokenStream = if i < fixed_count {
                let b = &fixed_binds[i];
                quote!(#b)
            } else {
                quote!(_)
            };
            match slot.field {
                FieldRef::Named(name) => {
                    let f = Ident::new(name, Span::call_site());
                    quote!(#f: #bind)
                }
                FieldRef::Pos(arity, index) => {
                    let holes =
                        (0..arity).map(|j| if j == index { bind.clone() } else { quote!(_) });
                    quote!(#(#holes),*)
                }
            }
        })
        .collect();

    // Per-fixed-slot bool sub-expressions, `&&`-chained.
    let mut slot_checks: Vec<TokenStream> = Vec::new();
    for (slot, (bind, child)) in schema
        .slots
        .iter()
        .take(fixed_count)
        .zip(fixed_binds.iter().zip(children))
    {
        slot_checks.push(lower_bool_fixed_slot(slot.ty, bind, child, ctx)?);
    }
    let body = if slot_checks.is_empty() {
        quote!(true)
    } else {
        quote!( #(#slot_checks)&&* )
    };

    let is_tuple = matches!(
        schema.slots.first().map(|s| s.field),
        Some(FieldRef::Pos(..))
    );
    let destructure = if is_tuple {
        quote!(::murphy_plugin_api::NodeKind::#variant(#(#field_pats),*))
    } else {
        let rest = if schema.covers_all_fields {
            quote!()
        } else {
            quote!(, ..)
        };
        quote!(::murphy_plugin_api::NodeKind::#variant { #(#field_pats),* #rest })
    };

    Ok(quote! {
        ( if let #destructure = *cx.kind(#subject) {
            #body
        } else {
            false
        } )
    })
}

/// Lower one fixed (non-`List`) slot into a `return`-free bool sub-expression
/// — the value-form counterpart of [`lower_fixed_slot`].
fn lower_bool_fixed_slot(
    ty: SlotTy,
    bind: &Ident,
    child: &murphy_pattern::Pat,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;
    match ty {
        SlotTy::Node => lower_bool(child, &quote!(#bind), ctx),
        SlotTy::OptNode => {
            if matches!(child.kind, PatKind::NilTest) {
                // Bare `nil?` at an `OptNode` slot: an absent slot matches,
                // a present slot must be a `nil` node.
                let n = gensym(ctx, "__n");
                Ok(quote! {
                    ( #bind.get().map_or(true, |#n| ::core::matches!(
                        *cx.kind(#n),
                        ::murphy_plugin_api::NodeKind::Nil
                    )) )
                })
            } else {
                // Any other child: the slot must be present and the child
                // pattern must hold against it.
                let n = gensym(ctx, "__n");
                let inner = lower_bool(child, &quote!(#n), ctx)?;
                Ok(quote! {
                    ( #bind.get().map_or(false, |#n| #inner) )
                })
            }
        }
        SlotTy::Sym => {
            let syms = sym_slot_alternatives(child)?;
            // The bool form mirrors `lower_fixed_slot`'s sym branch with
            // an inverted polarity: a wildcard slot is `true`; a single
            // literal compares; a union routes through `matches!`.
            if syms.is_empty() {
                return Ok(quote!(true));
            }
            if syms.len() == 1 {
                let s = syms[0];
                return Ok(quote!( ( cx.symbol_str(#bind) == #s ) ));
            }
            Ok(quote!((::core::matches!(cx.symbol_str(#bind), #(#syms)|*))))
        }
        SlotTy::List => unreachable!("List slot is excluded from fixed slots"),
    }
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
