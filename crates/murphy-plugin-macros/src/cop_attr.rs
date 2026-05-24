//! Implementation of the `#[cop]` and `#[on_node]` attribute proc-macros
//! (murphy-9cr.8).
//!
//! This module contains the `proc_macro2::TokenStream`-level logic; the
//! `proc_macro::TokenStream` shims in `lib.rs` call into these functions.
//!
//! The pipeline is:
//!
//! 1. `parse_cop_args` decodes the `#[cop(...)]` arguments into [`CopArgs`].
//! 2. The decorated item is parsed as a [`syn::ItemImpl`]; the impl block is
//!    validated to be inherent, non-generic, non-unsafe, and to target a
//!    named type.
//! 3. `collect_cop_methods` walks the impl's methods, strips `#[on_node]`
//!    attributes in place, resolves each `kind` string through
//!    [`murphy_ast::tag_from_pattern_name`], rejects `#[cfg]`/`#[cfg_attr]`
//!    on dispatched methods, and returns a list of [`CopMethod`]s.
//! 4. `validate_signature` checks each `#[on_node]` method is exactly
//!    `fn(&self, NodeId, &Cx<'_>)`.
//! 5. `validate_no_duplicate_kinds` rejects the same kind appearing on more
//!    than one `#[on_node]`.
//! 6. `lower_cop_impl` emits `impl Cop` (metadata `const`s), `impl NodeCop`
//!    (`KINDS` array + a `check` that matches on `NodeKindTag::of(...).0`
//!    and routes to the dispatched methods), and re-emits the original
//!    impl block with `#[on_node]` removed.
//!
//! `on_node` itself is registered only so that misuse outside a `#[cop]`
//! impl produces a clear compile error — correct uses are consumed by
//! `cop` before the `on_node` proc-macro is ever reached.

use std::collections::BTreeMap;

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::{
    Error, Ident, ImplItem, ItemImpl, LitBool, LitStr, Path, Token,
    parse::{Parse, ParseStream, Parser},
};

/// Parsed `#[cop(...)]` arguments.
struct CopArgs {
    name: LitStr,
    description: Option<LitStr>,
    /// Parsed severity literal and its resolved variant name ("Warning" / "Error").
    default_severity: Option<(LitStr, &'static str)>,
    default_enabled: Option<LitBool>,
    options: Option<Path>,
}

/// `key = value` pair in a macro argument list.
struct KvArg {
    key: Ident,
    _eq: Token![=],
    value: syn::Expr,
}

impl Parse for KvArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(KvArg {
            key: input.parse()?,
            _eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// Accumulate an error into `acc`, creating it if `None`.
fn push_error(acc: &mut Option<Error>, e: Error) {
    match acc.take() {
        Some(mut prev) => {
            prev.combine(e);
            *acc = Some(prev);
        }
        None => *acc = Some(e),
    }
}

/// Require that `value_expr` is a string literal; return it or add an error.
fn require_str_lit(key: &Ident, value: &syn::Expr, errors: &mut Option<Error>) -> Option<LitStr> {
    match value {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.clone()),
        _ => {
            push_error(
                errors,
                Error::new_spanned(value, format!("#[cop]: '{}' must be a string literal", key)),
            );
            None
        }
    }
}

/// Require that `value_expr` is a bool literal; return it or add an error.
fn require_bool_lit(key: &Ident, value: &syn::Expr, errors: &mut Option<Error>) -> Option<LitBool> {
    match value {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(b),
            ..
        }) => Some(b.clone()),
        _ => {
            push_error(
                errors,
                Error::new_spanned(
                    value,
                    format!("#[cop]: '{}' must be a bool literal (true or false)", key),
                ),
            );
            None
        }
    }
}

/// Require that `value_expr` is a path expression; return it or add an error.
fn require_path(key: &Ident, value: &syn::Expr, errors: &mut Option<Error>) -> Option<Path> {
    match value {
        syn::Expr::Path(ep) => Some(ep.path.clone()),
        _ => {
            push_error(
                errors,
                Error::new_spanned(value, format!("#[cop]: '{}' must be a type path", key)),
            );
            None
        }
    }
}

/// Parse `#[cop(name = "...", ...)]` arguments.
///
/// Accepted keys: `name` (required), `description`, `default_severity`,
/// `default_enabled`, `options`.
fn parse_cop_args(args: TokenStream) -> syn::Result<CopArgs> {
    // Collect all key=value arguments as a comma-separated list.
    let pairs: syn::punctuated::Punctuated<KvArg, Token![,]> =
        syn::punctuated::Punctuated::parse_terminated
            .parse2(args)
            .map_err(|_| Error::new(Span::call_site(), "#[cop]: invalid argument syntax"))?;

    let mut name_lit: Option<LitStr> = None;
    let mut description_lit: Option<LitStr> = None;
    let mut default_severity: Option<(LitStr, &'static str)> = None;
    let mut default_enabled: Option<LitBool> = None;
    let mut options_path: Option<Path> = None;
    let mut errors: Option<Error> = None;

    for pair in pairs {
        let key = &pair.key;
        let key_str = key.to_string();
        match key_str.as_str() {
            "name" => {
                if name_lit.is_some() {
                    push_error(
                        &mut errors,
                        Error::new_spanned(key, "#[cop]: duplicate argument 'name'"),
                    );
                } else if let Some(s) = require_str_lit(key, &pair.value, &mut errors) {
                    name_lit = Some(s);
                }
            }
            "description" => {
                if description_lit.is_some() {
                    push_error(
                        &mut errors,
                        Error::new_spanned(key, "#[cop]: duplicate argument 'description'"),
                    );
                } else if let Some(s) = require_str_lit(key, &pair.value, &mut errors) {
                    description_lit = Some(s);
                }
            }
            "default_severity" => {
                if default_severity.is_some() {
                    push_error(
                        &mut errors,
                        Error::new_spanned(key, "#[cop]: duplicate argument 'default_severity'"),
                    );
                } else if let Some(lit) = require_str_lit(key, &pair.value, &mut errors) {
                    let variant: Option<&'static str> = match lit.value().as_str() {
                        "warning" => Some("Warning"),
                        "error" => Some("Error"),
                        _ => {
                            push_error(
                                &mut errors,
                                Error::new_spanned(
                                    &lit,
                                    "#[cop]: default_severity must be one of \"warning\" / \"error\"",
                                ),
                            );
                            None
                        }
                    };
                    if let Some(v) = variant {
                        default_severity = Some((lit, v));
                    }
                }
            }
            "default_enabled" => {
                if default_enabled.is_some() {
                    push_error(
                        &mut errors,
                        Error::new_spanned(key, "#[cop]: duplicate argument 'default_enabled'"),
                    );
                } else if let Some(b) = require_bool_lit(key, &pair.value, &mut errors) {
                    default_enabled = Some(b);
                }
            }
            "options" => {
                if options_path.is_some() {
                    push_error(
                        &mut errors,
                        Error::new_spanned(key, "#[cop]: duplicate argument 'options'"),
                    );
                } else if let Some(p) = require_path(key, &pair.value, &mut errors) {
                    options_path = Some(p);
                }
            }
            other => {
                push_error(
                    &mut errors,
                    Error::new_spanned(key, format!("#[cop]: unknown argument '{other}'")),
                );
            }
        }
    }

    if let Some(e) = errors {
        return Err(e);
    }

    let name = name_lit.ok_or_else(|| {
        Error::new(
            Span::call_site(),
            "#[cop]: missing required argument 'name'",
        )
    })?;

    Ok(CopArgs {
        name,
        description: description_lit,
        default_severity,
        default_enabled,
        options: options_path,
    })
}

/// Format the list of valid kind names for error messages.
fn valid_kinds_list() -> String {
    murphy_ast::KIND_PATTERN_NAMES
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Dispatch model for a single cop method.
///
/// A method is reached either by per-kind dispatch (one or more
/// `#[on_node(kind = "...")]`) or by the singular per-file investigation
/// (`#[on_new_investigation]`, modelled on RuboCop's hook of the same
/// name); the two are mutually exclusive on the same method and across
/// the same `#[cop]` impl (see [`validate_dispatch_consistency`]).
///
/// `#[on_new_investigation]` is intended for file-scoped passes that
/// iterate `cx.comments()` (or similar file-level structured data). The
/// name is the soft guardrail: anything reaching for `cx.source()` /
/// `cx.raw_source()` is signalling "I really want raw bytes" and
/// should be a hand-rolled `KINDS = &[]` cop, not the macro shorthand.
enum Dispatch {
    /// `#[on_node(kind = "...")]` — one entry per kind on the method.
    Node(Vec<NodeKindEntry>),
    /// `#[on_new_investigation]` — one bare attribute, no payload. The
    /// kept [`syn::Attribute`] carries the span for diagnostics; boxed
    /// to keep [`Dispatch`] cheap relative to the `Node` variant
    /// (clippy::large_enum_variant).
    Investigation(Box<syn::Attribute>),
}

/// One `#[on_node(kind = "...", methods = [...])]` declaration on a
/// dispatch method.
///
/// `methods` is **only** meaningful for `kind = "send"` — it lists the
/// allow-listed `Send.method` symbol names that should reach the
/// user's check method. Empty `methods` means no restriction (the
/// historical behaviour: every `Send` reaches the cop). RuboCop's
/// `restrict_on_send` analogue (murphy-34d).
struct NodeKindEntry {
    kind_lit: LitStr,
    tag: u8,
    /// Allow-listed method names for `kind = "send"`. Empty ⇒ no
    /// filter applied; the macro-generated dispatch arm invokes the
    /// user method for every matching node.
    methods: Vec<LitStr>,
}

/// A dispatched method extracted from an impl block (`#[on_node]` or
/// `#[on_file]`).
struct CopMethod {
    /// Method identifier.
    ident: Ident,
    /// How the host reaches this method.
    dispatch: Dispatch,
}

/// Parse the contents of `#[on_node(kind = "..."[, methods = ["..."]])]`.
///
/// Returns the kind literal plus the (possibly empty) list of method
/// name literals. The methods array is rejected at the call site if
/// the kind is not `"send"` (RuboCop's `restrict_on_send` analogue —
/// `methods` only makes sense on send dispatch).
fn parse_on_node_args(input: ParseStream<'_>) -> syn::Result<(LitStr, Vec<LitStr>)> {
    let key: Ident = input.parse()?;
    if key != "kind" {
        return Err(Error::new_spanned(
            &key,
            format!("#[on_node]: unknown argument '{key}'; expected 'kind'"),
        ));
    }
    input.parse::<Token![=]>()?;
    let kind: LitStr = input.parse()?;

    let mut methods: Vec<LitStr> = Vec::new();
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
        // Optional trailing comma — accept it and stop.
        if input.is_empty() {
            return Ok((kind, methods));
        }
        let second_key: Ident = input.parse()?;
        if second_key != "methods" {
            return Err(Error::new_spanned(
                &second_key,
                format!("#[on_node]: unknown argument '{second_key}'; expected 'methods'",),
            ));
        }
        input.parse::<Token![=]>()?;
        // `methods = [...]` — a bracketed list of string literals.
        let content;
        syn::bracketed!(content in input);
        while !content.is_empty() {
            let m: LitStr = content.parse()?;
            methods.push(m);
            if content.is_empty() {
                break;
            }
            content.parse::<Token![,]>()?;
        }
        if methods.is_empty() {
            return Err(Error::new_spanned(
                &second_key,
                "#[on_node]: `methods = []` is not allowed — list at least one method name or omit the argument",
            ));
        }
    }

    Ok((kind, methods))
}

/// Collect all dispatched methods (`#[on_node]` and
/// `#[on_new_investigation]`) from an impl block.
///
/// Strips both attributes from method attributes in place (leaves other
/// attrs). Returns a list of methods that had at least one of either.
fn collect_cop_methods(item_impl: &mut ItemImpl) -> syn::Result<Vec<CopMethod>> {
    let mut cop_methods = Vec::new();
    let mut errors: Option<Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(f) = item else {
            continue;
        };

        let mut kinds: Vec<NodeKindEntry> = Vec::new();
        let mut investigation_attrs: Vec<syn::Attribute> = Vec::new();

        // Partition: collect #[on_node] / #[on_new_investigation] and retain everything else.
        let mut kept_attrs = Vec::new();
        for attr in f.attrs.drain(..) {
            if attr.path().is_ident("on_node") {
                match attr.parse_args_with(parse_on_node_args) {
                    Ok((kind_lit, methods_lits)) => {
                        let kind_str = kind_lit.value();
                        match murphy_ast::tag_from_pattern_name(&kind_str) {
                            Some(tag) => {
                                // `methods` is only meaningful on send (RuboCop's
                                // `restrict_on_send` analogue). For any other kind it's
                                // a category error — the cop is filtering by an axis
                                // that does not exist on that node type.
                                if !methods_lits.is_empty() && kind_str != "send" {
                                    let e = Error::new_spanned(
                                        &kind_lit,
                                        format!(
                                            "#[on_node]: `methods = [...]` is only valid for `kind = \"send\"`; got kind \"{kind_str}\"",
                                        ),
                                    );
                                    match errors.take() {
                                        Some(mut acc) => {
                                            acc.combine(e);
                                            errors = Some(acc);
                                        }
                                        None => errors = Some(e),
                                    }
                                } else {
                                    kinds.push(NodeKindEntry {
                                        kind_lit,
                                        tag: tag.0,
                                        methods: methods_lits,
                                    });
                                }
                            }
                            None => {
                                let valid = valid_kinds_list();
                                let e = Error::new_spanned(
                                    &kind_lit,
                                    format!(
                                        "#[on_node]: unknown node kind \"{kind_str}\". Valid kinds are: {valid}"
                                    ),
                                );
                                match errors.take() {
                                    Some(mut acc) => {
                                        acc.combine(e);
                                        errors = Some(acc);
                                    }
                                    None => errors = Some(e),
                                }
                            }
                        }
                    }
                    Err(e) => match errors.take() {
                        Some(mut acc) => {
                            acc.combine(e);
                            errors = Some(acc);
                        }
                        None => errors = Some(e),
                    },
                }
            } else if attr.path().is_ident("on_new_investigation") {
                // `#[on_new_investigation]` takes no arguments. Reject any
                // payload so typos like `#[on_new_investigation(kind = "send")]`
                // fail loudly instead of silently behaving as a file-scoped
                // pass.
                if !matches!(attr.meta, syn::Meta::Path(_)) {
                    let e = Error::new_spanned(
                        &attr,
                        "#[on_new_investigation]: takes no arguments (per-file investigation is the singular dispatch shape)",
                    );
                    match errors.take() {
                        Some(mut acc) => {
                            acc.combine(e);
                            errors = Some(acc);
                        }
                        None => errors = Some(e),
                    }
                }
                investigation_attrs.push(attr);
            } else {
                kept_attrs.push(attr);
            }
        }
        f.attrs = kept_attrs;

        // Per-method validation: a method can't mix dispatch shapes, and
        // `#[on_new_investigation]` must appear at most once on a single
        // method.
        if !kinds.is_empty() && !investigation_attrs.is_empty() {
            let e = Error::new_spanned(
                &investigation_attrs[0],
                "#[on_new_investigation] and #[on_node] cannot appear on the same method (choose one dispatch shape)",
            );
            match errors.take() {
                Some(mut acc) => {
                    acc.combine(e);
                    errors = Some(acc);
                }
                None => errors = Some(e),
            }
            continue;
        }
        if investigation_attrs.len() > 1 {
            let e = Error::new_spanned(
                &investigation_attrs[1],
                "#[on_new_investigation] may appear at most once per method",
            );
            match errors.take() {
                Some(mut acc) => {
                    acc.combine(e);
                    errors = Some(acc);
                }
                None => errors = Some(e),
            }
            continue;
        }

        let dispatch = if let Some(attr) = investigation_attrs.into_iter().next() {
            Dispatch::Investigation(Box::new(attr))
        } else if !kinds.is_empty() {
            Dispatch::Node(kinds)
        } else {
            // No dispatch attrs — not a cop method.
            continue;
        };

        // `#[cfg]` / `#[cfg_attr]` on a dispatched method would conditionally
        // drop the method body while the generated `KINDS` array and
        // dispatch arm (or direct investigation call) remain unconditional,
        // producing "cannot find method" errors when the cfg is off.
        // Generating cfg-gated dispatch would require splitting the const
        // slice / branching the `check` body and is out of v1 scope —
        // reject explicitly instead.
        let attr_name = match &dispatch {
            Dispatch::Node(_) => "#[on_node]",
            Dispatch::Investigation(_) => "#[on_new_investigation]",
        };
        for attr in &f.attrs {
            if attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr") {
                let e = Error::new_spanned(
                    attr,
                    format!(
                        "#[cfg] / #[cfg_attr] on a {attr_name} method are not supported in v1 \
                         (the generated KINDS array and dispatch arm would still reference the \
                         conditionally-removed method); move the conditional gating outside the \
                         #[cop] impl"
                    ),
                );
                match errors.take() {
                    Some(mut acc) => {
                        acc.combine(e);
                        errors = Some(acc);
                    }
                    None => errors = Some(e),
                }
            }
        }

        cop_methods.push(CopMethod {
            ident: f.sig.ident.clone(),
            dispatch,
        });
    }

    if let Some(e) = errors {
        return Err(e);
    }

    Ok(cop_methods)
}

/// Validate the signature of a dispatched method.
///
/// Two shapes are accepted, depending on `dispatch`:
/// - `Dispatch::Node`: `fn name(&self, node: <NodeId>, cx: &<Cx<'_>>)`
/// - `Dispatch::Investigation`: `fn name(&self, cx: &<Cx<'_>>)` —
///   no `NodeId` parameter, modelled on RuboCop's
///   `on_new_investigation(&self)` hook (the cop reaches file-level
///   data via `cx.comments()` etc.).
///
/// Common to both: no generics, no async, no const, no abi, no return
/// type or `()`. Error messages keep the `#[on_node]` wording to match
/// existing trybuild fixtures; a future cleanup can route the attr
/// name through the messages.
fn validate_signature(method: &syn::ImplItemFn, dispatch: &Dispatch) -> syn::Result<()> {
    let sig = &method.sig;

    // No async.
    if sig.asyncness.is_some() {
        return Err(Error::new_spanned(
            sig.asyncness,
            "#[on_node] methods must not be async",
        ));
    }

    // No const.
    if sig.constness.is_some() {
        return Err(Error::new_spanned(
            sig.constness,
            "#[on_node] methods must not be const",
        ));
    }

    // No abi.
    if sig.abi.is_some() {
        return Err(Error::new_spanned(
            &sig.abi,
            "#[on_node] methods must not specify an ABI",
        ));
    }

    // No generics.
    if !sig.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &sig.generics,
            "#[on_node] methods must not have generic parameters",
        ));
    }

    // Return type must be absent or `()`.
    match &sig.output {
        syn::ReturnType::Default => {}
        syn::ReturnType::Type(_, ty) => {
            // Accept `()` explicitly.
            match ty.as_ref() {
                syn::Type::Tuple(t) if t.elems.is_empty() => {}
                _ => {
                    return Err(Error::new_spanned(
                        ty,
                        "#[on_node] methods must return () or have no return type annotation",
                    ));
                }
            }
        }
    }

    // Inputs: &self, NodeId, &Cx<'_>
    let inputs: Vec<_> = sig.inputs.iter().collect();

    // First must be &self.
    match inputs.first() {
        Some(syn::FnArg::Receiver(r)) => {
            // Must be `&self` (reference, not mutable, no self type override).
            if r.reference.is_none() {
                return Err(Error::new_spanned(
                    r,
                    "#[on_node] methods must take `&self` (not `self` by value)",
                ));
            }
            if r.mutability.is_some() {
                return Err(Error::new_spanned(
                    r,
                    "#[on_node] methods must take `&self` (not `&mut self`)",
                ));
            }
        }
        Some(other) => {
            return Err(Error::new_spanned(
                other,
                "#[on_node] methods must take `&self` as the first parameter",
            ));
        }
        None => {
            return Err(Error::new_spanned(
                sig.fn_token,
                "#[on_node] methods must take `&self` as the first parameter",
            ));
        }
    }

    // Remaining-argument count depends on the dispatch shape:
    // - Node:          (&self, NodeId, &Cx<'_>)         → 3 inputs
    // - Investigation: (&self, &Cx<'_>)                 → 2 inputs
    let cx_arg = match dispatch {
        Dispatch::Node(_) => {
            if inputs.len() != 3 {
                return Err(Error::new_spanned(
                    sig.fn_token,
                    "#[on_node] methods must have exactly 2 parameters after &self: node: NodeId, cx: &Cx<'_>",
                ));
            }
            // Second arg: type path with last segment `NodeId`.
            let node_arg = inputs[1];
            let node_ty = match node_arg {
                syn::FnArg::Typed(pt) => &*pt.ty,
                _ => {
                    return Err(Error::new_spanned(
                        node_arg,
                        "#[on_node]: second parameter must be `node: NodeId`",
                    ));
                }
            };
            validate_node_id_type(node_ty)?;
            inputs[2]
        }
        Dispatch::Investigation(_) => {
            if inputs.len() != 2 {
                return Err(Error::new_spanned(
                    sig.fn_token,
                    "#[on_new_investigation] methods must have exactly 1 parameter after &self: cx: &Cx<'_> \
                     (no NodeId — file-level investigations reach data via cx.comments() etc.)",
                ));
            }
            inputs[1]
        }
    };

    let cx_ty = match cx_arg {
        syn::FnArg::Typed(pt) => &*pt.ty,
        _ => {
            return Err(Error::new_spanned(
                cx_arg,
                "#[on_node]: parameter must be `cx: &Cx<'_>`",
            ));
        }
    };
    validate_cx_type(cx_ty)?;

    Ok(())
}

/// Validate that `ty` is a path whose last segment is `NodeId` with no
/// generic arguments.
fn validate_node_id_type(ty: &syn::Type) -> syn::Result<()> {
    let syn::Type::Path(tp) = ty else {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: second parameter type must be `NodeId` (a named type path)",
        ));
    };

    let last = tp
        .path
        .segments
        .last()
        .expect("path must have at least one segment");

    if last.ident != "NodeId" {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: second parameter type must be `NodeId`",
        ));
    }

    if !matches!(last.arguments, syn::PathArguments::None) {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: `NodeId` must not have generic arguments",
        ));
    }

    Ok(())
}

/// Validate that `ty` is `&Cx<'lifetime>` — a reference to a path whose
/// last segment is `Cx` with exactly one lifetime generic argument.
fn validate_cx_type(ty: &syn::Type) -> syn::Result<()> {
    let syn::Type::Reference(tr) = ty else {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: third parameter type must be `&Cx<'_>`",
        ));
    };

    if tr.mutability.is_some() {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: third parameter must be `&Cx<'_>` (not `&mut Cx<'_>`)",
        ));
    }

    let syn::Type::Path(tp) = tr.elem.as_ref() else {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: third parameter type must be `&Cx<'_>`",
        ));
    };

    let last = tp
        .path
        .segments
        .last()
        .expect("path must have at least one segment");

    if last.ident != "Cx" {
        return Err(Error::new_spanned(
            ty,
            "#[on_node]: third parameter type must be `&Cx<'_>` (found different type name)",
        ));
    }

    // Must have exactly one lifetime generic argument.
    match &last.arguments {
        syn::PathArguments::AngleBracketed(ab) => {
            let args: Vec<_> = ab.args.iter().collect();
            if args.len() != 1 {
                return Err(Error::new_spanned(
                    ty,
                    "#[on_node]: `Cx` must have exactly one lifetime argument, e.g. `Cx<'_>`",
                ));
            }
            match args[0] {
                syn::GenericArgument::Lifetime(_) => {}
                _ => {
                    return Err(Error::new_spanned(
                        ty,
                        "#[on_node]: `Cx`'s generic argument must be a lifetime, e.g. `Cx<'_>`",
                    ));
                }
            }
        }
        _ => {
            return Err(Error::new_spanned(
                ty,
                "#[on_node]: `Cx` must have exactly one lifetime argument, e.g. `Cx<'_>`",
            ));
        }
    }

    Ok(())
}

/// Check for duplicate kind registrations across all node-dispatch methods.
///
/// Investigation methods are not checked: they declare no kinds.
fn validate_no_duplicate_kinds(methods: &[CopMethod]) -> syn::Result<()> {
    // Map from kind name -> first occurrence LitStr.
    let mut seen: BTreeMap<String, LitStr> = BTreeMap::new();
    let mut errors: Option<Error> = None;

    for method in methods {
        let Dispatch::Node(kinds) = &method.dispatch else {
            continue;
        };
        for entry in kinds {
            let kind_lit = &entry.kind_lit;
            let name = kind_lit.value();
            if let Some(first) = seen.get(&name) {
                let mut e = Error::new_spanned(
                    kind_lit,
                    format!("#[cop]: kind \"{name}\" is dispatched to multiple methods"),
                );
                // Attach a note pointing at the first occurrence.
                e.combine(Error::new_spanned(
                    first,
                    format!("#[cop]: kind \"{name}\" first declared here"),
                ));
                match errors.take() {
                    Some(mut acc) => {
                        acc.combine(e);
                        errors = Some(acc);
                    }
                    None => errors = Some(e),
                }
            } else {
                seen.insert(name, kind_lit.clone());
            }
        }
    }

    if let Some(e) = errors {
        return Err(e);
    }

    Ok(())
}

/// Enforce per-impl dispatch consistency:
///
/// - At most one `#[on_new_investigation]` method per impl
///   (per-file investigation is singular).
/// - `#[on_new_investigation]` and `#[on_node]` cannot coexist in the
///   same impl — mixing would mean the host both dispatches per-kind
///   and calls once-per-file, which the `NodeCop` trait does not
///   express.
fn validate_dispatch_consistency(methods: &[CopMethod]) -> syn::Result<()> {
    let mut investigation_attrs: Vec<&syn::Attribute> = Vec::new();
    let mut has_node = false;
    for m in methods {
        match &m.dispatch {
            Dispatch::Node(_) => has_node = true,
            Dispatch::Investigation(attr) => investigation_attrs.push(attr.as_ref()),
        }
    }

    if investigation_attrs.len() > 1 {
        let mut e = Error::new_spanned(
            investigation_attrs[1],
            "#[cop]: at most one #[on_new_investigation] method per impl (per-file investigation is the singular dispatch)",
        );
        e.combine(Error::new_spanned(
            investigation_attrs[0],
            "#[cop]: first #[on_new_investigation] declared here",
        ));
        return Err(e);
    }

    if has_node && !investigation_attrs.is_empty() {
        return Err(Error::new_spanned(
            investigation_attrs[0],
            "#[cop]: cannot mix #[on_new_investigation] with #[on_node] in the same impl \
             (choose either per-kind dispatch or per-file investigation)",
        ));
    }

    Ok(())
}

/// Validate the signature of all dispatched methods in the impl block.
fn validate_all_signatures(item_impl: &ItemImpl, cop_methods: &[CopMethod]) -> syn::Result<()> {
    let mut errors: Option<Error> = None;

    for cop_method in cop_methods {
        // Find the corresponding ImplItemFn.
        for item in &item_impl.items {
            if let ImplItem::Fn(f) = item
                && f.sig.ident == cop_method.ident
            {
                if let Err(e) = validate_signature(f, &cop_method.dispatch) {
                    match errors.take() {
                        Some(mut acc) => {
                            acc.combine(e);
                            errors = Some(acc);
                        }
                        None => errors = Some(e),
                    }
                }
                break;
            }
        }
    }

    if let Some(e) = errors {
        return Err(e);
    }

    Ok(())
}

/// Generate the lowered output: `impl Cop + impl NodeCop + stripped impl`.
fn lower_cop_impl(args: CopArgs, cop_methods: &[CopMethod], item_impl: ItemImpl) -> TokenStream {
    let self_ty = &item_impl.self_ty;
    let name_lit = &args.name;

    // Build KINDS array entries and the `check` body. An investigation
    // cop (single `#[on_new_investigation]` method, no node dispatch)
    // lowers to an empty `KINDS` slice and a `check` that delegates
    // straight to the user method — the same shape
    // `Layout/TrailingWhitespace` writes by hand. Per-impl dispatch
    // consistency (no mixing) is guaranteed by
    // [`validate_dispatch_consistency`].
    let investigation_method = cop_methods.iter().find_map(|m| match &m.dispatch {
        Dispatch::Investigation(_) => Some(&m.ident),
        Dispatch::Node(_) => None,
    });

    let mut kinds_entries: Vec<TokenStream> = Vec::new();
    let mut match_arms: Vec<TokenStream> = Vec::new();

    if investigation_method.is_none() {
        for cop_method in cop_methods {
            let method_ident = &cop_method.ident;
            let Dispatch::Node(entries) = &cop_method.dispatch else {
                continue;
            };

            // Build KINDS entries for each kind of this method.
            for entry in entries {
                let tag_lit = Literal::u8_suffixed(entry.tag);
                kinds_entries.push(quote! {
                    ::murphy_plugin_api::NodeKindTag(#tag_lit)
                });
            }

            // Partition entries into unfiltered (no `methods`) and
            // filtered (send-only with `methods = [...]`). Unfiltered
            // entries share one match arm with an or-pattern over their
            // tags; each filtered entry needs its own arm with the
            // symbol_str gate (one send entry per cop today, but the
            // shape generalises naturally).
            let unfiltered_tags: Vec<u8> = entries
                .iter()
                .filter(|e| e.methods.is_empty())
                .map(|e| e.tag)
                .collect();
            if !unfiltered_tags.is_empty() {
                let tag_patterns: Vec<TokenStream> = unfiltered_tags
                    .iter()
                    .map(|t| {
                        let tag_lit = Literal::u8_suffixed(*t);
                        quote! { #tag_lit }
                    })
                    .collect();
                let arm_pattern = if tag_patterns.len() == 1 {
                    quote! { #(#tag_patterns)* }
                } else {
                    quote! { #(#tag_patterns)|* }
                };
                match_arms.push(quote! {
                    #arm_pattern => Self::#method_ident(self, node, cx),
                });
            }

            for entry in entries.iter().filter(|e| !e.methods.is_empty()) {
                let tag_lit = Literal::u8_suffixed(entry.tag);
                // The methods filter is guaranteed (by parser-time validation)
                // to apply only to kind = "send". The dispatch arm
                // destructures `NodeKind::Send` to read the method symbol
                // and compares it against the allow-list before reaching
                // the user method. `cx.symbol_str` is a pure arena read.
                let method_lits = &entry.methods;
                match_arms.push(quote! {
                    #tag_lit => {
                        if let ::murphy_plugin_api::NodeKind::Send { method, .. } =
                            *cx.kind(node)
                        {
                            let m = cx.symbol_str(method);
                            if matches!(m, #(#method_lits)|*) {
                                Self::#method_ident(self, node, cx);
                            }
                        }
                    },
                });
            }
        }
    }

    // Build optional metadata consts for `impl Cop`.
    // Only emit a const when the caller explicitly provided the value;
    // the `Cop` trait provides defaults for all of them.

    let description_const: TokenStream = if let Some(lit) = &args.description {
        quote! { const DESCRIPTION: &'static str = #lit; }
    } else {
        quote! {}
    };

    let default_severity_const: TokenStream = if let Some((_lit, variant)) = &args.default_severity
    {
        let variant_ident = syn::Ident::new(variant, proc_macro2::Span::call_site());
        quote! {
            const DEFAULT_SEVERITY: ::core::option::Option<::murphy_plugin_api::Severity> =
                ::core::option::Option::Some(::murphy_plugin_api::Severity::#variant_ident);
        }
    } else {
        quote! {}
    };

    let default_enabled_const: TokenStream = if let Some(lit) = &args.default_enabled {
        quote! {
            const DEFAULT_ENABLED: ::core::option::Option<bool> =
                ::core::option::Option::Some(#lit);
        }
    } else {
        quote! {}
    };

    // `type Options` — use the caller-specified path, or fall back to NoOptions.
    let options_type: TokenStream = if let Some(path) = &args.options {
        quote! { type Options = #path; }
    } else {
        quote! { type Options = ::murphy_plugin_api::NoOptions; }
    };

    let impl_cop = quote! {
        impl ::murphy_plugin_api::Cop for #self_ty {
            #options_type
            const NAME: &'static str = #name_lit;
            #description_const
            #default_severity_const
            #default_enabled_const
        }
    };

    let impl_node_cop = if let Some(method_ident) = investigation_method {
        // Investigation form: empty KINDS, single direct call. The host
        // contract (see `NodeCop` doc + `dispatch::run_cops`) calls
        // `check` exactly once per file with `node == cx.root()`; the
        // user method takes only `cx` (modelled on RuboCop's
        // `on_new_investigation(&self)`).
        quote! {
            impl ::murphy_plugin_api::NodeCop for #self_ty {
                const KINDS: &'static [::murphy_plugin_api::NodeKindTag] = &[];

                fn check(&self, _node: ::murphy_plugin_api::NodeId, cx: &::murphy_plugin_api::Cx<'_>) {
                    Self::#method_ident(self, cx)
                }
            }
        }
    } else {
        quote! {
            impl ::murphy_plugin_api::NodeCop for #self_ty {
                const KINDS: &'static [::murphy_plugin_api::NodeKindTag] = &[
                    #(#kinds_entries,)*
                ];

                fn check(&self, node: ::murphy_plugin_api::NodeId, cx: &::murphy_plugin_api::Cx<'_>) {
                    match ::murphy_plugin_api::NodeKindTag::of(cx.kind(node)).0 {
                        #(#match_arms)*
                        _ => {}
                    }
                }
            }
        }
    };

    // Re-emit the original impl block with #[on_node] attrs already stripped.
    let stripped_impl = quote! { #item_impl };

    quote! {
        #impl_cop
        #impl_node_cop
        #stripped_impl
    }
}

// ─── Public entry points ──────────────────────────────────────────────────────

/// `#[cop(...)]` — layer-2 core implementation.
pub fn cop(args: TokenStream, item: TokenStream) -> TokenStream {
    // 1. Parse the item as an impl block.
    let mut item_impl: ItemImpl = match syn::parse2(item) {
        Ok(v) => v,
        Err(_) => {
            return Error::new(Span::call_site(), "#[cop] must be on an impl block")
                .to_compile_error();
        }
    };

    // 2. Validate impl block form.
    if let Err(e) = validate_impl_form(&item_impl) {
        return e.to_compile_error();
    }

    // 3. Parse #[cop(...)] args.
    let cop_args = match parse_cop_args(args) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    // 4. Collect dispatched methods (`#[on_node]` and `#[on_file]`).
    //    Both attributes are stripped from the impl in place.
    let cop_methods = match collect_cop_methods(&mut item_impl) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    // 5. Validate signatures of all dispatched methods (both shapes
    //    require `fn(&self, NodeId, &Cx<'_>)`).
    if let Err(e) = validate_all_signatures(&item_impl, &cop_methods) {
        return e.to_compile_error();
    }

    // 6. Check for duplicate kind registrations among `#[on_node]` methods.
    if let Err(e) = validate_no_duplicate_kinds(&cop_methods) {
        return e.to_compile_error();
    }

    // 7. Enforce per-impl dispatch consistency: at most one `#[on_file]`,
    //    and no mixing with `#[on_node]`.
    if let Err(e) = validate_dispatch_consistency(&cop_methods) {
        return e.to_compile_error();
    }

    // 8. Require at least one dispatched method.
    if cop_methods.is_empty() {
        return Error::new_spanned(
            item_impl.impl_token,
            "#[cop]: impl block has no #[on_node] or #[on_new_investigation] methods",
        )
        .to_compile_error();
    }

    // 9. Lower to trait impls + stripped impl.
    lower_cop_impl(cop_args, &cop_methods, item_impl)
}

/// Validate that an impl block is inherent (no trait), non-generic,
/// non-unsafe, and targets a named type.
fn validate_impl_form(item_impl: &ItemImpl) -> syn::Result<()> {
    // No trait impl.
    if let Some((_, path, _)) = &item_impl.trait_ {
        return Err(Error::new_spanned(
            path,
            "#[cop] must be on an inherent impl block (impl T { } not impl Trait for T)",
        ));
    }

    // No unsafe.
    if item_impl.unsafety.is_some() {
        return Err(Error::new_spanned(
            item_impl.unsafety,
            "#[cop] does not support unsafe impl blocks",
        ));
    }

    // No generic parameters.
    if !item_impl.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &item_impl.generics,
            "#[cop] does not support generic impl blocks",
        ));
    }

    // Self type must be a named type (Type::Path).
    if !matches!(*item_impl.self_ty, syn::Type::Path(_)) {
        return Err(Error::new_spanned(
            &item_impl.self_ty,
            "#[cop]: impl block must target a named type",
        ));
    }

    Ok(())
}

/// `#[on_node(...)]` — stub that fires only on misuse.
///
/// When `#[cop]` processes an impl block it will consume all `#[on_node]`
/// attributes directly, so this proc-macro entry point is only reached when
/// `#[on_node]` appears outside a `#[cop]` impl.  We always emit a
/// `compile_error!` to surface that misuse clearly.
pub fn on_node(_args: TokenStream, _item: TokenStream) -> TokenStream {
    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[on_node] must be used inside a #[cop] impl block",
    )
    .to_compile_error()
}

/// `#[on_new_investigation]` — layer-2 core implementation.
///
/// When `#[cop]` processes an impl block it consumes every
/// `#[on_new_investigation]` attribute directly, so this proc-macro
/// entry point is only reached when the attribute appears outside a
/// `#[cop]` impl. We always emit a `compile_error!` to surface the
/// misuse clearly.
pub fn on_new_investigation(_args: TokenStream, _item: TokenStream) -> TokenStream {
    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[on_new_investigation] must be used inside a #[cop] impl block",
    )
    .to_compile_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::TokenStream;

    #[test]
    fn parse_cop_args_name_only() {
        let args = quote! { name = "Plugin/Test" };
        match parse_cop_args(args) {
            Ok(a) => assert_eq!(a.name.value(), "Plugin/Test"),
            Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }

    #[test]
    fn parse_cop_args_missing_name() {
        let args = TokenStream::new();
        match parse_cop_args(args) {
            Ok(_) => panic!("expected Err for missing name"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("missing required argument 'name'"),
                    "got: {msg}"
                );
            }
        }
    }

    #[test]
    fn parse_cop_args_unknown_key() {
        let args = quote! { name = "X", foo = "bar" };
        match parse_cop_args(args) {
            Ok(_) => panic!("expected Err for unknown key"),
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("unknown argument 'foo'"), "got: {msg}");
            }
        }
    }

    #[test]
    fn valid_kind_resolves_correctly() {
        // Directly test the kind name resolution used by collect_cop_methods.
        let tag = murphy_ast::tag_from_pattern_name("send");
        assert_eq!(tag.map(|t| t.0), Some(17));
    }

    #[test]
    fn unknown_kind_returns_none() {
        let tag = murphy_ast::tag_from_pattern_name("carrot");
        assert!(tag.is_none(), "expected None for unknown kind 'carrot'");
    }

    #[test]
    fn parse_cop_args_all_optional_fields() {
        let args = quote! { name = "X", description = "a desc", default_severity = "warning", default_enabled = true };
        match parse_cop_args(args) {
            Ok(a) => {
                assert_eq!(a.name.value(), "X");
                assert_eq!(a.description.unwrap().value(), "a desc");
                let (lit, variant) = a.default_severity.unwrap();
                assert_eq!(lit.value(), "warning");
                assert_eq!(variant, "Warning");
                assert!(a.default_enabled.unwrap().value);
            }
            Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }

    #[test]
    fn parse_cop_args_severity_error_variant() {
        let args = quote! { name = "X", default_severity = "error" };
        match parse_cop_args(args) {
            Ok(a) => {
                let (_lit, variant) = a.default_severity.unwrap();
                assert_eq!(variant, "Error");
            }
            Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }

    #[test]
    fn parse_cop_args_invalid_severity() {
        let args = quote! { name = "X", default_severity = "info" };
        match parse_cop_args(args) {
            Ok(_) => panic!("expected Err for invalid severity"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("default_severity must be one of"),
                    "got: {msg}"
                );
            }
        }
    }

    #[test]
    fn parse_cop_args_options_path() {
        let args = quote! { name = "X", options = MyOptions };
        match parse_cop_args(args) {
            Ok(a) => {
                let path = a.options.unwrap();
                assert_eq!(path.segments.last().unwrap().ident.to_string(), "MyOptions");
            }
            Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }
}
