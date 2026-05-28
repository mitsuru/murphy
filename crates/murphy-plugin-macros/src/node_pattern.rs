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

    let unify_names = collect_unify_names(&ast.root);
    let mut ctx = Lower {
        fail: fail.clone(),
        next: 0,
        capture_kinds: ast.capture_kinds().to_vec(),
        local_caps: HashMap::new(),
        probe_caps: HashMap::new(),
        unify_names: unify_names.clone(),
    };
    let body = lower_pat(&ast.root, &quote!(node), &mut ctx)?;

    // D4 (murphy-nnr8): declare one `Option<NodeId>` variable per unique `_name`
    // unification atom. These are initialized to `None` at function entry and
    // read/written by the `lower_pat`/`lower_bool` arms for `PatKind::Unify`.
    let unify_decls: Vec<TokenStream> = ctx.unify_names
        .iter()
        .map(|n| {
            let var = unify_var(n);
            quote!(let mut #var: ::core::option::Option<::murphy_plugin_api::NodeId> = ::core::option::Option::None;)
        })
        .collect();

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
            #(#unify_decls)*
            #body
            #success
        }
    })
}

/// The gensym'd variable identifier for the unification variable of `name`.
///
/// `_x` → `__unify_x`, `_foo` → `__unify_foo`. The name is the post-`_`
/// portion stored in [`murphy_pattern::PatKind::Unify`].
fn unify_var(name: &str) -> Ident {
    Ident::new(&format!("__unify_{name}"), Span::call_site())
}

/// Collect all distinct unify names from the pattern tree.
///
/// Returns names in first-occurrence order (depth-first, left-to-right)
/// so the generated declarations are deterministic across compiler runs.
fn collect_unify_names(pat: &murphy_pattern::Pat) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    collect_unify_names_inner(pat, &mut names);
    names
}

fn collect_unify_names_inner(pat: &murphy_pattern::Pat, names: &mut Vec<String>) {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Unify { name } => {
            if !names.iter().any(|n| n == name) {
                names.push(name.clone());
            }
        }
        PatKind::Node { children, .. } => {
            for c in children {
                collect_unify_names_inner(c, names);
            }
        }
        PatKind::Union(alts) => {
            for a in alts {
                collect_unify_names_inner(a, names);
            }
        }
        PatKind::Not(b) | PatKind::Parent(b) | PatKind::Descend(b) => {
            collect_unify_names_inner(b, names);
        }
        PatKind::Capture { body, .. } => collect_unify_names_inner(body, names),
        PatKind::Quantifier { body, .. } => collect_unify_names_inner(body, names),
        PatKind::AnyOrder { children } | PatKind::Intersection { children } => {
            for c in children {
                collect_unify_names_inner(c, names);
            }
        }
        PatKind::Wildcard
        | PatKind::Rest
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_) => {}
    }
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
    /// Probe-scope capture bindings declared inside the AnyOrder phase-1
    /// search loops. Each entry maps a capture slot to a gensym'd
    /// `__pcap{n}: Option<NodeId>` identifier that is written when the
    /// capture pattern's element is probed and read by subsequent
    /// predicate-arg expressions within the same probe path.
    ///
    /// This map is populated just before `lower_bool_anyorder_probe` is
    /// called for the AnyOrder children and cleared on exit. Outside
    /// AnyOrder probe scope it is always empty, so [`predicate_arg_exprs`]
    /// falls back to the function-level `__cap{slot}` binding.
    probe_caps: HashMap<u16, Ident>,
    /// All distinct `_name` unification variable names collected from the
    /// pattern tree. Used by `emit_unify_snapshot` / `emit_unify_restore`
    /// to generate snapshot/restore code at each backtracking site (Union,
    /// Not body, Descend iteration). Populated once in `lower_matcher`.
    unify_names: Vec<String>,
}

/// Emit a wrapped bool expression with snapshot/restore for unify variables.
///
/// For each `_name` in the pattern, wraps `expr` as:
/// ```ignore
/// {
///     let __snap_unify_x = __unify_x; // per name
///     let __uokN = <expr>;
///     if !__uokN { __unify_x = __snap_unify_x; } // per name, on failure
///     __uokN
/// }
/// ```
/// When there are no unify names, returns `expr` unchanged (zero overhead).
fn wrap_with_unify_rollback(expr: TokenStream, ctx: &mut Lower) -> TokenStream {
    if ctx.unify_names.is_empty() {
        return expr;
    }
    let names = ctx.unify_names.clone();
    let snaps: Vec<Ident> = names
        .iter()
        .map(|n| gensym(ctx, &format!("__snap_unify_{n}_")))
        .collect();
    let vars: Vec<Ident> = names.iter().map(|n| unify_var(n)).collect();
    let save_stmts: Vec<TokenStream> = snaps
        .iter()
        .zip(vars.iter())
        .map(|(snap, var)| quote!(let #snap = #var;))
        .collect();
    let restore_stmts: Vec<TokenStream> = snaps
        .iter()
        .zip(vars.iter())
        .map(|(snap, var)| quote!(#var = #snap;))
        .collect();
    let ok = gensym(ctx, "__uok");
    quote! {
        ({
            #(#save_stmts)*
            let #ok = #expr;
            if !#ok {
                #(#restore_stmts)*
            }
            #ok
        })
    }
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

/// Turn `PredArg` elements into token-stream expressions to be passed as
/// extra arguments in the generated predicate call.
///
/// - `Lit::Int(v)` → `v_i64` literal
/// - `Lit::Float(v)` → `v_f64` literal
/// - `Lit::Str(s)` → `"s"` string literal
/// - `Lit::Sym(s)` → `"s"` string literal (cop methods receive `&str`)
/// - `Lit::True` / `Lit::False` → `true` / `false`
/// - `Lit::Nil` → compile error (nil args are not meaningful in Rust)
/// - `PredArg::Capture(slot)` → the capture slot's variable. Inside an
///   AnyOrder phase-1 probe (when `ctx.probe_caps` is populated) the
///   probe-scope binding `__pcap{n}.unwrap()` is used so that the
///   just-tried element is visible to the predicate; outside that scope
///   the function-level deferred-init variable `__cap{slot}` is used.
fn predicate_arg_exprs(
    args: &[murphy_pattern::PredArg],
    ctx: &Lower,
) -> syn::Result<Vec<TokenStream>> {
    use murphy_pattern::{Lit, PredArg};
    args.iter()
        .map(|arg| match arg {
            PredArg::Lit(lit) => Ok(match lit {
                Lit::Int(v) => quote!(#v),
                Lit::Float(v) => quote!(#v),
                Lit::Str(s) => {
                    let s = s.as_str();
                    quote!(#s)
                }
                Lit::Sym(s) => {
                    let s = s.as_str();
                    quote!(#s)
                }
                Lit::True => quote!(true),
                Lit::False => quote!(false),
                Lit::Nil => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "node_pattern!: `nil` predicate arg has no Rust counterpart",
                    ));
                }
            }),
            PredArg::Capture(slot) => {
                // If we are inside an AnyOrder phase-1 probe and a probe-scope
                // binding was declared for this slot, forward the argument
                // through that binding (`.unwrap()` is safe because
                // `lower_bool_anyorder_probe` wrote `Some(elem)` before the
                // predicate expression is evaluated in the same search path).
                if let Some(pcap) = ctx.probe_caps.get(slot) {
                    let expr = quote!(#pcap.unwrap());
                    return Ok(match ctx.capture_kinds.get(*slot as usize) {
                        Some(CaptureKind::OptNode) => {
                            quote!(::core::option::Option::Some(#expr))
                        }
                        _ => expr,
                    });
                }
                // If we are inside a backtracking commit scope (AnyOrder
                // phase-2 or quantifier), captures are written to a local
                // `Option<T>` via `local_caps`. A predicate arg that references
                // the same slot must read `<local>.unwrap()` rather than the
                // function-level `__cap{slot}` which has not been committed yet.
                if let Some(lcap) = ctx.local_caps.get(slot) {
                    return Ok(quote!(#lcap.unwrap()));
                }
                // Outside any backtracking scope: capture slots are bound as
                // `__cap{slot}` by the generated match function.
                // Forward-references are rejected at parse time.
                let var = cap_ident(*slot as usize);
                Ok(quote!(#var))
            }
        })
        .collect()
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
            // Detect uniform-capture sugar `${alt1 alt2 ...}` — every arm is
            // `Capture{slot:S, body:b}` with the same S. The parser guarantees
            // this is the normalised form for the `${ }` sugar. Lower each
            // arm's body via `lower_bool` (no capture write per-arm), then
            // emit the capture assignment once, unconditionally, after the OR
            // succeeds. This is safe because every arm fires the same slot S
            // on the winning path.
            if let Some(shared_slot) = alts.first().and_then(|a| {
                if let PatKind::Capture { slot, .. } = &a.kind {
                    Some(*slot)
                } else {
                    None
                }
            }) {
                let all_same_capture = alts.iter().all(
                    |a| matches!(&a.kind, PatKind::Capture { slot, .. } if *slot == shared_slot),
                );
                if all_same_capture {
                    // Lower each arm's body to a bool expression, then OR them.
                    let arm_bools: Vec<TokenStream> = alts
                        .iter()
                        .map(|alt| {
                            let PatKind::Capture { body, .. } = &alt.kind else {
                                unreachable!("all_same_capture guarantees Capture");
                            };
                            lower_bool(body, subject, ctx)
                        })
                        .collect::<syn::Result<_>>()?;
                    let fail = fail_stmt(ctx);
                    let ok = gensym(ctx, "__ok");
                    let assign = capture_assign(shared_slot, quote!(#subject), ctx);
                    return Ok(quote! {
                        let #ok: bool = ( #(#arm_bools)||* );
                        if !#ok {
                            #fail
                        }
                        #assign
                    });
                }
            }
            // Normal union (no uniform captures): each alternative lowers to a
            // `return`-free bool expression with unify rollback on failure.
            // The rollback ensures that a failed arm's partial `_name` bindings
            // do not leak into the next arm (D4, murphy-nnr8).
            let alt_bools: Vec<TokenStream> = alts
                .iter()
                .map(|alt| {
                    let b = lower_bool(alt, subject, ctx)?;
                    Ok(wrap_with_unify_rollback(b, ctx))
                })
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
            // and fail when it holds. Unify bindings from the inner body are
            // always rolled back (Not never commits inner state).
            let inner_bool = lower_bool(inner, subject, ctx)?;
            let inner_with_rollback = wrap_with_unify_rollback(inner_bool, ctx);
            let fail = fail_stmt(ctx);
            Ok(quote! {
                if #inner_with_rollback {
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
        PatKind::Predicate { name, args } => {
            // `#name` / `#name(args...)` calls a free fn in scope at the
            // call site. Fail the guard when the predicate returns `false`.
            let ident = predicate_ident(name)?;
            let fail = fail_stmt(ctx);
            let arg_exprs = predicate_arg_exprs(args, ctx)?;
            Ok(quote! {
                if !#ident(#subject, cx #(, #arg_exprs)*) {
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
            // into a bool expression over the per-descendant binding. Each
            // iteration's unify bindings are rolled back so a match attempt on
            // one descendant does not pollute a subsequent attempt.
            let d = gensym(ctx, "__d");
            let inner_bool = lower_bool(inner, &quote!(#d), ctx)?;
            let inner_with_rollback = wrap_with_unify_rollback(inner_bool, ctx);
            let hit = gensym(ctx, "__hit");
            let fail = fail_stmt(ctx);
            Ok(quote! {
                let #hit = cx.descendants(#subject).into_iter().any(|#d| #inner_with_rollback);
                if !#hit {
                    #fail
                }
            })
        }
        PatKind::AnyOrder { .. } => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: `<...>` any-order is only valid as a direct child \
             of a node match's List slot; reaching it in `lower_pat` is an \
             internal invariant violation",
        )),
        PatKind::Intersection { children } => {
            // `[a b c]` — all children match the same subject. Lower each
            // child as a sequential guard (captures flow into function-level
            // bindings in source order). Semantically equivalent to running
            // every child body in sequence on the same node.
            let guards: Vec<TokenStream> = children
                .iter()
                .map(|child| lower_pat(child, subject, ctx))
                .collect::<syn::Result<_>>()?;
            Ok(quote!(#(#guards)*))
        }
        PatKind::Unify { name } => {
            // D4 (murphy-nnr8): `_name` unification.
            //
            // Each unique name gets one `let mut __unify_{name}: Option<NodeId> = None;`
            // variable at the top of the generated function (see `lower_matcher`,
            // which collects them via `collect_unify_names`). Here we emit the
            // runtime binding/check logic:
            //
            //   match __unify_{name} {
            //     None => { __unify_{name} = Some(#subject); }   // first occurrence: bind
            //     Some(__u) => { if __u != #subject { return fail; } }  // check
            //   }
            let var = unify_var(name);
            let fail = fail_stmt(ctx);
            Ok(quote! {
                match #var {
                    ::core::option::Option::None => {
                        #var = ::core::option::Option::Some(#subject);
                    }
                    ::core::option::Option::Some(__u) => {
                        if __u != #subject {
                            #fail
                        }
                    }
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
/// or wrapped by a single `$` capture (`$pat+` etc.) — OR if any child is an
/// `AnyOrder` (`<...>`) block. Both shapes require the backtracking closure
/// scaffolding from [`lower_quantifier_list`] to safely re-assign captures
/// across backtrack attempts.
///
/// The parser forbids any other nesting (`($pat)+`, double-`Capture`,
/// `Quantifier` inside `Quantifier` body), so the classification is
/// unambiguous.
fn has_quantifier_child(children: &[murphy_pattern::Pat]) -> bool {
    use murphy_pattern::PatKind;
    children.iter().any(|c| {
        let inner = match &c.kind {
            PatKind::Capture { body, .. } => &body.kind,
            other => other,
        };
        matches!(inner, PatKind::Quantifier { .. } | PatKind::AnyOrder { .. })
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
    AnyOrder(&'a [murphy_pattern::Pat]),
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
    if let PatKind::AnyOrder { children } = &pat.kind {
        return ListKid::AnyOrder(children);
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
    //    that isn't already redirected by an enclosing quantifier list.
    //    Locals are declared outside the closure so the closure can
    //    re-assign them across backtrack attempts. Slots register in
    //    insertion order so the commit step is deterministic.
    //
    //    A slot owned by an outer driver must keep writing into *that*
    //    driver's local — otherwise the outer's commit would race a
    //    second write to the function-level `__cap{slot}` and rustc
    //    would reject the second assignment (the binding is single-
    //    assign). Inner `capture_assign(slot)` looks up
    //    `ctx.local_caps[slot]` and naturally hits the outer ident, so
    //    the outer's `__lcap{slot}` is the one that sees `Some(_)`.
    let mut slots: Vec<u16> = Vec::new();
    for child in list_children {
        collect_capture_slots(child, &mut slots);
    }
    // Each slot ID may legitimately repeat across positions in the parser
    // surface (the parser deduplicates them later); keep only the first
    // occurrence so we declare one local per slot.
    let mut seen = std::collections::BTreeSet::new();
    slots.retain(|s| seen.insert(*s));
    // Drop slots an outer driver already owns — see the comment above.
    slots.retain(|s| !ctx.local_caps.contains_key(s));

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
        PatKind::AnyOrder { children } => {
            for c in children {
                collect_capture_slots(c, out);
            }
        }
        PatKind::Intersection { children } => {
            for c in children {
                collect_capture_slots(c, out);
            }
        }
        PatKind::Wildcard
        | PatKind::NilTest
        | PatKind::Lit(_)
        | PatKind::Predicate { .. }
        | PatKind::Kind(_)
        | PatKind::Rest
        | PatKind::Unify { .. } => {}
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
        ListKid::AnyOrder(children) => {
            emit_anyorder_step(children, rest_kids, cursor_expr, list_val, len_val, ctx)
        }
    }
}

/// Emit an any-order step: try all permutations of `children` against a
/// prefix of the remaining list elements, then recurse on the suffix.
///
/// Mirrors the C-backend's two-phase backtracking algorithm:
///
/// * **Phase 1 (probe)**: compile-time-unrolled nested loops find a valid
///   element assignment using [`lower_bool_anyorder_probe`] (no captures
///   written).
/// * **Phase 2 (commit)**: replay in declaration order via [`lower_pat`],
///   writing captures into the `ctx.local_caps`-redirected slots.
///
/// Requires that the caller has already set up backtracking closure
/// scaffolding (i.e., the list was routed through [`lower_quantifier_list`]).
/// This is guaranteed because [`has_quantifier_child`] returns `true` for any
/// list containing an `AnyOrder` child.
///
/// v1 limit: at most 10 non-rest patterns in `children` (parser-enforced).
fn emit_anyorder_step(
    children: &[murphy_pattern::Pat],
    rest_kids: &[murphy_pattern::Pat],
    cursor_expr: &TokenStream,
    list_val: &Ident,
    len_val: &Ident,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;

    // Separate rest from non-rest children.
    let has_rest = children.iter().any(|c| matches!(&c.kind, PatKind::Rest));
    let non_rest: Vec<&murphy_pattern::Pat> = children
        .iter()
        .filter(|c| !matches!(&c.kind, PatKind::Rest))
        .collect();
    let n = non_rest.len(); // compile-time known, ≤ 10

    // --- shared idents -----------------------------------------------
    let cur = gensym(ctx, "__cur");
    let consume = gensym(ctx, "__consume");
    let assign_ident = gensym(ctx, "__assign");
    let found_ident = gensym(ctx, "__found");
    // Gensym the label so that multiple `<...>` siblings in the same node
    // list generate distinct label names inside the same outer closure.
    let label_ident = gensym(ctx, "__aos");
    let search_label = syn::Lifetime::new(&format!("'{}", label_ident), Span::call_site());

    // --- phase-1 probe booleans --------------------------------------
    // Collect all capture slots referenced in the non-rest children so that
    // we can declare probe-scope `Option<NodeId>` bindings (`__pcap{n}`) that
    // hold the just-tried element value.  A predicate whose args include a
    // capture (`#pred?($x)`) may then read the probe-scope binding instead of
    // the function-level deferred-init `__cap{slot}` (which would be
    // uninitialized at this point, causing E0381).
    let mut probe_cap_slots: Vec<u16> = Vec::new();
    for pat in non_rest.iter() {
        collect_capture_slots(pat, &mut probe_cap_slots);
    }
    probe_cap_slots.dedup();
    // Gensym a probe-cap ident for each slot and register in `ctx.probe_caps`.
    let probe_cap_idents: Vec<(u16, Ident)> = probe_cap_slots
        .iter()
        .map(|&slot| (slot, gensym(ctx, "__pcap")))
        .collect();
    for (slot, ident) in &probe_cap_idents {
        ctx.probe_caps.insert(*slot, ident.clone());
    }
    // Pre-allocate a unique element ident for each non-rest pattern, then
    // build the probe bool expression (no capture writes, but probe-scope
    // bindings are written via `lower_bool_anyorder_probe`).
    let probe_elems: Vec<Ident> = (0..n).map(|_| gensym(ctx, "__pe")).collect();
    let probe_bools: Vec<TokenStream> = non_rest
        .iter()
        .zip(probe_elems.iter())
        .map(|(pat, elem)| lower_bool_anyorder_probe(pat, &quote!(#elem), ctx))
        .collect::<syn::Result<_>>()?;
    // Clear the probe-scope bindings — they must not be visible outside this
    // AnyOrder block's phase-1 code.
    for (slot, _) in &probe_cap_idents {
        ctx.probe_caps.remove(slot);
    }

    // --- phase-2 commit guards ---------------------------------------
    // Pre-allocate commit element idents, then lower each non-rest pattern
    // via lower_pat (writes captures via ctx.local_caps).
    let commit_elems: Vec<Ident> = (0..n).map(|_| gensym(ctx, "__ce")).collect();
    let commit_guards: Vec<TokenStream> = non_rest
        .iter()
        .zip(commit_elems.iter())
        .enumerate()
        .map(|(i, (pat, commit_elem))| {
            let guard = lower_pat(pat, &quote!(#commit_elem), ctx)?;
            Ok(quote! {
                let #commit_elem = #list_val[#cur + #assign_ident[#i]];
                #guard
            })
        })
        .collect::<syn::Result<_>>()?;

    // --- suffix continuation -----------------------------------------
    let suffix_cursor = quote!(#cur + #consume);
    let suffix = emit_list_step(rest_kids, &suffix_cursor, list_val, len_val, ctx)?;

    // --- build the phase-1 nested search loops -----------------------
    let search_body = build_anyorder_search(
        n,
        0,
        &probe_elems,
        &probe_bools,
        &assign_ident,
        &consume,
        &cur,
        list_val,
        &found_ident,
        &search_label,
    );

    // --- assemble the per-consume-value body -------------------------
    // Reset search state, run phase-1, then on success run phase-2 +
    // suffix in a nested probe closure.  The nested closure means a
    // commit-guard failure returns `None` from the *inner* closure only,
    // not from the outer backtracking closure — allowing the outer loop
    // to try the next consume value.
    let sub = gensym(ctx, "__sub");
    // Probe-scope capture declarations: one `Option<NodeId>` per capture slot
    // referenced inside this AnyOrder block.  Written by
    // `lower_bool_anyorder_probe` when it encounters a `Capture` pattern, and
    // read by `predicate_arg_exprs` via `ctx.probe_caps` when a predicate arg
    // names the same slot.  Re-initialised to `None` at the top of each
    // attempt so that a failed probe path never leaks a stale value into the
    // next attempt.
    let probe_cap_decls: Vec<TokenStream> = probe_cap_idents
        .iter()
        .map(|(_, ident)| {
            quote!(let mut #ident: ::core::option::Option<::murphy_plugin_api::NodeId> = ::core::option::Option::None;)
        })
        .collect();
    let one_attempt = quote! {
        #(#probe_cap_decls)*
        let mut #assign_ident: [usize; 10] = [usize::MAX; 10];
        let mut #found_ident: bool = false;
        #search_label: {
            #search_body
        }
        if #found_ident {
            let #sub: ::core::option::Option<()> = (|| -> ::core::option::Option<()> {
                #(#commit_guards)*
                #suffix
            })();
            if #sub.is_some() {
                return ::core::option::Option::Some(());
            }
        }
    };

    // --- outer structure: iterate consume values ---------------------
    let code = if has_rest {
        // With rest: try each consume value from n to remaining, return
        // on first success.
        quote! {
            let #cur: usize = #cursor_expr;
            if #cur + #n > #len_val {
                return ::core::option::Option::None;
            }
            for #consume in #n..=(#len_val - #cur) {
                #one_attempt
            }
            return ::core::option::Option::None;
        }
    } else {
        // Without rest: consume is fixed at n.
        quote! {
            let #cur: usize = #cursor_expr;
            if #cur + #n > #len_val {
                return ::core::option::Option::None;
            }
            let #consume: usize = #n;
            #one_attempt
            return ::core::option::Option::None;
        }
    };

    Ok(code)
}

/// Build the phase-1 backtracking search tree for [`emit_anyorder_step`].
///
/// Recursively unrolls N loops (one per non-rest pattern) at compile time.
/// At each depth `i`, we try every candidate element index `j` in `0..consume`,
/// probe pattern `i` against `list[cur + j]` (using the pre-built bool
/// expression), and if it passes, record `j` in `assign[i]` and recurse to
/// depth `i+1`.  Duplicate detection uses compile-time index comparisons
/// `assign[0] == j || assign[1] == j || ...` against the already-assigned
/// slots, so any list length is supported without bitmask size limits.
/// When `depth == n` (all patterns placed), set `found` to `true` and break
/// the labeled block.
///
/// `found` and `search_label` are gensym'd idents that the caller wires up;
/// this function only emits expressions that reference them.
#[allow(clippy::too_many_arguments)]
fn build_anyorder_search(
    n: usize,
    depth: usize,
    probe_elems: &[Ident],
    probe_bools: &[TokenStream],
    assign: &Ident,
    consume: &Ident,
    cur: &Ident,
    list_val: &Ident,
    found: &Ident,
    search_label: &syn::Lifetime,
) -> TokenStream {
    if depth == n {
        // All patterns placed: record success and break out of the search block.
        return quote! {
            #found = true;
            break #search_label;
        };
    }
    let i = depth;
    // Collision-free loop variable: index by depth only (one loop per depth).
    let loop_var = Ident::new(&format!("__lv{i}"), Span::call_site());
    let elem = &probe_elems[i];
    let probe = &probe_bools[i];
    let inner = build_anyorder_search(
        n,
        depth + 1,
        probe_elems,
        probe_bools,
        assign,
        consume,
        cur,
        list_val,
        found,
        search_label,
    );
    let checks = (0..depth).map(|idx| quote!(#assign[#idx] == #loop_var));
    let skip_check = if depth > 0 {
        quote! {
            if #(#checks)||* {
                continue;
            }
        }
    } else {
        quote!()
    };
    quote! {
        for #loop_var in 0usize..#consume {
            #skip_check
            let #elem = #list_val[#cur + #loop_var];
            if #probe {
                #assign[#i] = #loop_var;
                #inner
            }
        }
    }
}

/// Probe helper for [`emit_anyorder_step`]'s phase-1 search.
///
/// Behaves like [`lower_bool`] but also handles `PatKind::Capture` *at any
/// depth* inside the pattern tree by:
/// 1. Writing `__pcap{n} = Some(subject)` into the probe-scope binding
///    (so that a subsequent predicate in the same AnyOrder block can read the
///    just-tried element as a predicate arg), then
/// 2. Probing only the capture body as a bool expression (no function-level
///    capture write — that happens in phase 2 only).
///
/// The probe-scope binding `__pcap{n}` is the gensym'd `Ident` registered in
/// `ctx.probe_caps[slot]` by [`emit_anyorder_step`] before calling this fn.
///
/// Compound patterns that can nest a `Capture` — `Node` and `Parent` — are
/// handled recursively (via [`lower_bool_anyorder_probe_node`] and a direct
/// recursive call, respectively) so that an inner `$x` is written to
/// `__pcap{x}` during probe, not just top-level captures.
///
/// All other arms delegate to [`lower_bool`], which already rejects
/// `Rest`, `Quantifier`, and nested `AnyOrder` (parser-enforced).
fn lower_bool_anyorder_probe(
    pat: &murphy_pattern::Pat,
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Capture { slot, body, .. } => {
            // Write the probe-scope binding so that later predicates in the
            // same AnyOrder block can read the just-tried element via its
            // capture slot.
            let assign = if let Some(pcap) = ctx.probe_caps.get(slot) {
                quote!(#pcap = ::core::option::Option::Some(#subject);)
            } else {
                // No probe-cap registered for this slot (can only happen if
                // the slot was not collected, which would be a bug). Emit
                // nothing and let phase-2 handle it.
                quote!()
            };
            let body_probe = lower_bool_anyorder_probe(body, subject, ctx)?;
            Ok(quote!({ #assign #body_probe }))
        }
        PatKind::Node { head, children } => {
            // Recurse through node children so that captures nested inside
            // `(node $x ...)` are written to their probe-scope bindings.
            lower_bool_anyorder_probe_node(head, children, subject, ctx)
        }
        PatKind::Parent(inner) => {
            // `^x` inside an AnyOrder arm: bind the parent (bool-expr form
            // returns false if absent), then recurse with the probe path.
            let p = gensym(ctx, "__p");
            let inner_probe = lower_bool_anyorder_probe(inner, &quote!(#p), ctx)?;
            Ok(quote! {
                ( cx.parent(#subject).get().map_or(false, |#p| #inner_probe) )
            })
        }
        PatKind::Union(alts) => {
            // Uniform-capture sugar `${a b ...}` parses as a Union whose every
            // arm is a Capture with the same slot. Delegating to `lower_bool`
            // would (a) emit a `__cap{slot} = subject;` statement-block of type
            // `()` — which `if #probe { ... }` rejects — and (b) skip writing
            // the probe-scope binding `__pcap{slot}`, so a later predicate in
            // the same AnyOrder block referencing that slot would read None.
            // Recurse into each arm with the probe lowering and OR-chain the
            // resulting bool expressions: the arm that matches writes its
            // probe binding and yields `true`; the others short-circuit.
            let alt_bools: Vec<TokenStream> = alts
                .iter()
                .map(|alt| lower_bool_anyorder_probe(alt, subject, ctx))
                .collect::<syn::Result<_>>()?;
            Ok(quote!( ( #(#alt_bools)||* ) ))
        }
        PatKind::Intersection { children } => {
            // `[a b c]` inside an AnyOrder arm: AND of children's bool
            // expressions, each routed through the probe path so any captures
            // nested inside write their `__pcap{slot}` bindings.
            //
            // The C matcher snapshots its trial buffer and discards it if any
            // child fails (`matcher.rs::IrNode::Intersection`); the macro must
            // match that atomicity. Without a rollback, child 1 could write a
            // probe binding before child 2 fails, leaving a stale
            // `__pcap{slot}` that a subsequent AnyOrder permutation would
            // observe. Snapshot every probe-capture slot reachable from this
            // intersection before the AND chain, and restore on failure.
            let child_bools: Vec<TokenStream> = children
                .iter()
                .map(|child| lower_bool_anyorder_probe(child, subject, ctx))
                .collect::<syn::Result<_>>()?;
            let mut slots: Vec<u16> = Vec::new();
            for child in children {
                collect_capture_slots(child, &mut slots);
            }
            slots.sort_unstable();
            slots.dedup();
            // Resolve each slot to its current probe-scope ident; skip slots
            // that are not registered (defensive — a capture inside an
            // intersection that isn't itself inside the surrounding AnyOrder
            // shouldn't be possible, but if it ever is we just don't snapshot
            // and the existing match behaviour is preserved).
            let pcap_idents: Vec<Ident> = slots
                .iter()
                .filter_map(|s| ctx.probe_caps.get(s).cloned())
                .collect();
            if pcap_idents.is_empty() {
                return Ok(quote!( ( #(#child_bools)&&* ) ));
            }
            let snap_idents: Vec<Ident> = (0..pcap_idents.len())
                .map(|_| gensym(ctx, "__isnap"))
                .collect();
            Ok(quote!({
                #( let #snap_idents = #pcap_idents; )*
                let __intersection_ok = #(#child_bools)&&*;
                if !__intersection_ok {
                    #( #pcap_idents = #snap_idents; )*
                }
                __intersection_ok
            }))
        }
        // All other arms (Wildcard, Lit, Kind, NilTest, Not, Predicate,
        // Descend) cannot legally contain a Capture (parser-enforced), so
        // lower_bool is a correct and complete delegate.
        _ => lower_bool(pat, subject, ctx),
    }
}

/// Probe-mode counterpart of [`lower_bool_node`].
///
/// Mirrors [`lower_bool_node`]'s structure but routes fixed-slot children
/// through [`lower_bool_anyorder_probe_fixed_slot`] instead of
/// [`lower_bool_fixed_slot`], so that a `Capture` nested inside a slot child
/// writes its probe-scope binding (`__pcap{slot}`) during phase-1.
fn lower_bool_anyorder_probe_node(
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
        Head::Exact(t) => lower_bool_anyorder_probe_exact_node(*t, children, subject, ctx),
    }
}

/// Probe-mode counterpart of [`lower_bool_exact_node`].
///
/// Identical to [`lower_bool_exact_node`] except that per-slot children are
/// lowered via [`lower_bool_anyorder_probe_fixed_slot`] so that captures
/// inside slots write their probe bindings.
fn lower_bool_anyorder_probe_exact_node(
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

    // Per-fixed-slot bool sub-expressions, using the probe variant.
    let mut slot_checks: Vec<TokenStream> = Vec::new();
    for (slot, (bind, child)) in schema
        .slots
        .iter()
        .take(fixed_count)
        .zip(fixed_binds.iter().zip(children))
    {
        slot_checks.push(lower_bool_anyorder_probe_fixed_slot(
            slot.ty, bind, child, ctx,
        )?);
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

/// Probe-mode counterpart of [`lower_bool_fixed_slot`].
///
/// Routes `Node` and `OptNode` children through [`lower_bool_anyorder_probe`]
/// instead of [`lower_bool`], so that captures nested inside slot children
/// write their probe-scope bindings during phase-1.  `Sym` slots cannot
/// contain captures (parser-enforced) and are delegated to `lower_bool_fixed_slot`.
fn lower_bool_anyorder_probe_fixed_slot(
    ty: SlotTy,
    bind: &Ident,
    child: &murphy_pattern::Pat,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;
    match ty {
        SlotTy::Node => lower_bool_anyorder_probe(child, &quote!(#bind), ctx),
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
                // Any other child: slot must be present; recurse probe.
                let n = gensym(ctx, "__n");
                let inner = lower_bool_anyorder_probe(child, &quote!(#n), ctx)?;
                Ok(quote! {
                    ( #bind.get().map_or(false, |#n| #inner) )
                })
            }
        }
        // Sym slots only accept wildcard / literal / literal-union — no
        // captures possible, delegate to the non-probe variant.
        SlotTy::Sym | SlotTy::List => lower_bool_fixed_slot(ty, bind, child, ctx),
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
            // Each arm gets unify rollback so a failed arm's partial bindings
            // don't leak into the next (D4, murphy-nnr8).
            let alt_bools: Vec<TokenStream> = alts
                .iter()
                .map(|alt| {
                    let b = lower_bool(alt, subject, ctx)?;
                    Ok(wrap_with_unify_rollback(b, ctx))
                })
                .collect::<syn::Result<_>>()?;
            Ok(quote!( ( #(#alt_bools)||* ) ))
        }
        PatKind::Not(inner) => {
            let inner_bool = lower_bool(inner, subject, ctx)?;
            // Always roll back unify bindings from Not's inner body.
            let inner_with_rollback = wrap_with_unify_rollback(inner_bool, ctx);
            Ok(quote!( ( !#inner_with_rollback ) ))
        }
        PatKind::Predicate { name, args } => {
            // `#name` / `#name(args...)` calls a free fn in scope at the
            // call site; in value form it is simply that bool expression.
            let ident = predicate_ident(name)?;
            let arg_exprs = predicate_arg_exprs(args, ctx)?;
            Ok(quote!( ( #ident(#subject, cx #(, #arg_exprs)*) ) ))
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
            let inner_with_rollback = wrap_with_unify_rollback(inner_bool, ctx);
            Ok(quote! {
                ( cx.descendants(#subject).into_iter().any(|#d| #inner_with_rollback) )
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
        PatKind::AnyOrder { .. } => Err(syn::Error::new(
            Span::call_site(),
            "node_pattern!: `<...>` any-order is not valid inside `{}` / `!` / `` ` ``",
        )),
        PatKind::Intersection { children } => {
            // `[a b c]` inside `{}` / `!` / `` ` ``: AND of children's bool
            // expressions. Captures are structurally forbidden in the `bool`
            // context (`lower_bool` rejects them), so each child lowers cleanly.
            let child_bools: Vec<TokenStream> = children
                .iter()
                .map(|child| lower_bool(child, subject, ctx))
                .collect::<syn::Result<_>>()?;
            Ok(quote!( ( #(#child_bools)&&* ) ))
        }
        PatKind::Unify { name } => {
            // D4 (murphy-nnr8): `_name` unification in bool context.
            // Uses the same `__unify_{name}` variable declared at function scope.
            let var = unify_var(name);
            Ok(quote! {
                ({
                    match #var {
                        ::core::option::Option::None => {
                            #var = ::core::option::Option::Some(#subject);
                            true
                        }
                        ::core::option::Option::Some(__u) => __u == #subject,
                    }
                })
            })
        }
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
