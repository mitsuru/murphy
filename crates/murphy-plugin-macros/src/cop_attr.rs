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

/// A `#[on_node]` method extracted from an impl block.
struct CopMethod {
    /// Method identifier.
    ident: Ident,
    /// All (kind_lit, kind_tag) from `#[on_node]` on this method, in order.
    kinds: Vec<(LitStr, u8)>,
}

/// Collect all `#[on_node]` methods from an impl block.
///
/// Strips `#[on_node]` from method attributes in place (leaves other attrs).
/// Returns a list of methods that had at least one `#[on_node]`.
fn collect_cop_methods(item_impl: &mut ItemImpl) -> syn::Result<Vec<CopMethod>> {
    let mut cop_methods = Vec::new();
    let mut errors: Option<Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(f) = item else {
            continue;
        };

        let mut kinds: Vec<(LitStr, u8)> = Vec::new();

        // Partition: collect #[on_node] and retain everything else.
        let mut kept_attrs = Vec::new();
        for attr in f.attrs.drain(..) {
            if attr.path().is_ident("on_node") {
                match attr.parse_args_with(|input: ParseStream<'_>| {
                    // Parse as OnNodeArgsRaw then resolve.
                    let key: Ident = input.parse()?;
                    if key != "kind" {
                        return Err(Error::new_spanned(
                            &key,
                            format!("#[on_node]: unknown argument '{key}'; expected 'kind'"),
                        ));
                    }
                    input.parse::<Token![=]>()?;
                    let kind: LitStr = input.parse()?;
                    Ok(kind)
                }) {
                    Ok(kind_lit) => {
                        let kind_str = kind_lit.value();
                        match murphy_ast::tag_from_pattern_name(&kind_str) {
                            Some(tag) => {
                                kinds.push((kind_lit, tag.0));
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
            } else {
                kept_attrs.push(attr);
            }
        }
        f.attrs = kept_attrs;

        if !kinds.is_empty() {
            // `#[cfg]` / `#[cfg_attr]` on a `#[on_node]` method would
            // conditionally drop the method body while the generated
            // `KINDS` array entry and `match` arm remain unconditional,
            // producing "cannot find method" errors when the cfg is off.
            // Generating cfg-gated KINDS entries and match arms would
            // require splitting the const slice and is out of v1 scope —
            // reject the attribute explicitly instead.
            for attr in &f.attrs {
                if attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr") {
                    let e = Error::new_spanned(
                        attr,
                        "#[cfg] / #[cfg_attr] on a #[on_node] method are not supported in v1 \
                         (the generated KINDS array and dispatch arm would still reference the \
                         conditionally-removed method); move the conditional gating outside the \
                         #[cop] impl",
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
                kinds,
            });
        }
    }

    if let Some(e) = errors {
        return Err(e);
    }

    Ok(cop_methods)
}

/// Validate the signature of a `#[on_node]`-tagged method.
///
/// Must be: `fn name(&self, node: <NodeId>, cx: &<Cx<'_>>)`
/// with no generics, no async, no const, no abi, no return type or `()`.
fn validate_signature(method: &syn::ImplItemFn) -> syn::Result<()> {
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

    // Must have exactly 2 more arguments (NodeId and &Cx<'_>).
    if inputs.len() != 3 {
        return Err(Error::new_spanned(
            sig.fn_token,
            "#[on_node] methods must have exactly 2 parameters after &self: node: NodeId, cx: &Cx<'_>",
        ));
    }

    // Second arg: type path with last segment `NodeId`, no generic args.
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

    // Third arg: type reference to Cx with one lifetime generic.
    let cx_arg = inputs[2];
    let cx_ty = match cx_arg {
        syn::FnArg::Typed(pt) => &*pt.ty,
        _ => {
            return Err(Error::new_spanned(
                cx_arg,
                "#[on_node]: third parameter must be `cx: &Cx<'_>`",
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

/// Check for duplicate kind registrations across all methods.
fn validate_no_duplicate_kinds(methods: &[CopMethod]) -> syn::Result<()> {
    // Map from kind name -> first occurrence LitStr.
    let mut seen: BTreeMap<String, LitStr> = BTreeMap::new();
    let mut errors: Option<Error> = None;

    for method in methods {
        for (kind_lit, _tag) in &method.kinds {
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

/// Validate the signature of all `#[on_node]` methods in the impl block.
fn validate_all_signatures(item_impl: &ItemImpl, cop_methods: &[CopMethod]) -> syn::Result<()> {
    let mut errors: Option<Error> = None;

    for cop_method in cop_methods {
        // Find the corresponding ImplItemFn.
        for item in &item_impl.items {
            if let ImplItem::Fn(f) = item
                && f.sig.ident == cop_method.ident
            {
                if let Err(e) = validate_signature(f) {
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

    // Build KINDS array entries and match arms.
    let mut kinds_entries: Vec<TokenStream> = Vec::new();
    let mut match_arms: Vec<TokenStream> = Vec::new();

    for cop_method in cop_methods {
        let method_ident = &cop_method.ident;

        // Build KINDS entries for each kind of this method.
        for (_, tag) in &cop_method.kinds {
            let tag_lit = Literal::u8_suffixed(*tag);
            kinds_entries.push(quote! {
                ::murphy_plugin_api::NodeKindTag(#tag_lit)
            });
        }

        // Build the match arm (possibly with or-patterns for multiple kinds).
        let tag_patterns: Vec<TokenStream> = cop_method
            .kinds
            .iter()
            .map(|(_, tag)| {
                let tag_lit = Literal::u8_suffixed(*tag);
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

    let impl_node_cop = quote! {
        impl ::murphy_plugin_api::NodeCop for #self_ty {
            const KINDS: &'static [::murphy_plugin_api::NodeKindTag] = &[
                #(#kinds_entries,)*
            ];

            fn check(&self, node: ::murphy_ast::NodeId, cx: &::murphy_plugin_api::Cx<'_>) {
                match ::murphy_plugin_api::NodeKindTag::of(cx.kind(node)).0 {
                    #(#match_arms)*
                    _ => {}
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

    // 4. Collect #[on_node] methods (strips them from the impl in place).
    let cop_methods = match collect_cop_methods(&mut item_impl) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };

    // 5. Validate signatures of all on_node methods.
    if let Err(e) = validate_all_signatures(&item_impl, &cop_methods) {
        return e.to_compile_error();
    }

    // 6. Check for duplicate kind registrations.
    if let Err(e) = validate_no_duplicate_kinds(&cop_methods) {
        return e.to_compile_error();
    }

    // 7. Require at least one #[on_node] method.
    if cop_methods.is_empty() {
        return Error::new_spanned(
            item_impl.impl_token,
            "#[cop]: impl block has no #[on_node] methods",
        )
        .to_compile_error();
    }

    // 8. Lower to trait impls + stripped impl.
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
