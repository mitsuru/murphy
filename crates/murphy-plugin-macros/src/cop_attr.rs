//! Implementation of the `#[cop]` and `#[on_node]` attribute proc-macros
//! (murphy-9cr.8).
//!
//! This module contains the `proc_macro2::TokenStream`-level logic; the
//! `proc_macro::TokenStream` shims in `lib.rs` call into these functions.
//!
//! ## Layer 2 (murphy-9cr.8.2) — core implementation
//!
//! Implements:
//! - `CopArgs` parsing (`name` required; other named args are rejected)
//! - impl block form validation (inherent, non-generic, non-unsafe)
//! - `#[on_node]` attribute collection and `kind` resolution
//! - Method signature validation
//! - Duplicate kind detection
//! - Lowering to `impl Cop + impl NodeCop + stripped impl`

use std::collections::BTreeMap;

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::{
    Error, Ident, ImplItem, ItemImpl, LitStr, Token,
    parse::{Parse, ParseStream, Parser},
};

/// Parsed `#[cop(...)]` arguments.
struct CopArgs {
    name: LitStr,
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

/// Parse `#[cop(name = "...")]` arguments.
fn parse_cop_args(args: TokenStream) -> syn::Result<CopArgs> {
    // Collect all key=value arguments as a comma-separated list.
    let pairs: syn::punctuated::Punctuated<KvArg, Token![,]> =
        syn::punctuated::Punctuated::parse_terminated
            .parse2(args)
            .map_err(|_| Error::new(Span::call_site(), "#[cop]: invalid argument syntax"))?;

    let mut name_lit: Option<LitStr> = None;
    let mut errors: Option<Error> = None;

    for pair in pairs {
        let key = &pair.key;
        let key_str = key.to_string();
        match key_str.as_str() {
            "name" => {
                if name_lit.is_some() {
                    let e = Error::new_spanned(key, "#[cop]: duplicate argument 'name'");
                    match errors.take() {
                        Some(mut acc) => {
                            acc.combine(e);
                            errors = Some(acc);
                        }
                        None => errors = Some(e),
                    }
                } else {
                    match &pair.value {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(s),
                            ..
                        }) => {
                            name_lit = Some(s.clone());
                        }
                        _ => {
                            let e = Error::new_spanned(
                                &pair.value,
                                "#[cop]: 'name' must be a string literal",
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
            }
            other => {
                let e = Error::new_spanned(
                    key,
                    format!(
                        "#[cop]: unknown argument '{other}' (will be supported in a later layer)"
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

    if let Some(e) = errors {
        return Err(e);
    }

    let name = name_lit.ok_or_else(|| {
        Error::new(
            Span::call_site(),
            "#[cop]: missing required argument 'name'",
        )
    })?;

    Ok(CopArgs { name })
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

    let impl_cop = quote! {
        impl ::murphy_plugin_api::Cop for #self_ty {
            type Options = ::murphy_plugin_api::NoOptions;
            const NAME: &'static str = #name_lit;
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
}
