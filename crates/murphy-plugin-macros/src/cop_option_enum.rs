//! `#[derive(CopOptionEnum)]` implementation.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

/// Entry point invoked by the `#[proc_macro_derive]` shim in `lib.rs`.
pub fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let enum_name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(CopOptionEnum)] does not support generic enums",
        ));
    }

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[derive(CopOptionEnum)] can only be applied to enums",
            ));
        }
    };

    let mut parsed = Vec::with_capacity(variants.len());
    for variant in variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "#[derive(CopOptionEnum)] only supports unit variants",
            ));
        }

        let mut value = None;
        for attr in &variant.attrs {
            if !attr.path().is_ident("option") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("value") {
                    if value.is_some() {
                        return Err(meta.error("duplicate #[option(value = ...)]"));
                    }
                    let lit: LitStr = meta.value()?.parse()?;
                    value = Some(lit.value());
                    Ok(())
                } else {
                    Err(meta.error("unknown #[option(...)] key"))
                }
            })?;
        }

        let value = value.ok_or_else(|| {
            syn::Error::new(
                variant.ident.span(),
                "#[derive(CopOptionEnum)] variants require #[option(value = \"...\")]",
            )
        })?;
        parsed.push((&variant.ident, value));
    }

    let mut seen = std::collections::BTreeSet::new();
    for (_, value) in &parsed {
        if !seen.insert(value.clone()) {
            return Err(syn::Error::new(
                enum_name.span(),
                format!("duplicate CopOptionEnum value `{value}`"),
            ));
        }
    }

    let variants = parsed.iter().map(|(ident, _)| ident);
    let variants_for_as_str = parsed.iter().map(|(ident, _)| ident);
    let values = parsed.iter().map(|(_, value)| value);
    let values_for_match = parsed.iter().map(|(_, value)| value);
    let values_for_as_str = parsed.iter().map(|(_, value)| value);
    let values_json = serde_json::to_string(
        &parsed
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
    )
    .unwrap();

    Ok(quote! {
        impl ::murphy_plugin_api::CopOptionEnum for #enum_name {
            const VALUES: &'static [&'static str] = &[ #(#values),* ];
            const VALUES_JSON: &'static str = #values_json;

            fn from_str(value: &str) -> ::core::option::Option<Self> {
                match value {
                    #(#values_for_match => ::core::option::Option::Some(Self::#variants),)*
                    _ => ::core::option::Option::None,
                }
            }

            fn as_str(self) -> &'static str {
                match self {
                    #(Self::#variants_for_as_str => #values_for_as_str,)*
                }
            }
        }
    })
}
