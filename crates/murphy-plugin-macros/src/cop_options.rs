//! `#[derive(CopOptions)]` implementation.
//!
//! Turns a plain options struct into `impl Default` + `impl CopOptions`
//! (the `SCHEMA` const and a `from_config_json` decoder). See
//! `docs/plans/2026-05-22-murphy-9cr7-derive-copoptions-design.md` and
//! ADR 0036.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, ExprArray, ExprLit, Fields, GenericArgument, Ident, Lit, Path,
    PathArguments, Type, spanned::Spanned,
};

/// Entry point invoked by the `#[proc_macro_derive]` shim in `lib.rs`.
pub fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let struct_name = &input.ident;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            Fields::Unit => {
                // A unit struct is a valid option-less options type.
                return Ok(generate_unit(struct_name));
            }
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "#[derive(CopOptions)] requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[derive(CopOptions)] can only be applied to structs",
            ));
        }
    };

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(CopOptions)] does not support generic structs",
        ));
    }

    let parsed: Vec<ParsedField> = fields
        .iter()
        .map(ParsedField::parse)
        .collect::<syn::Result<_>>()?;
    validate_unique_external_names(&parsed)?;

    let default_impl = generate_default(struct_name, &parsed);
    let copoptions_impl = generate_copoptions(struct_name, &parsed);

    Ok(quote! {
        #default_impl
        #copoptions_impl
    })
}

fn validate_unique_external_names(fields: &[ParsedField]) -> syn::Result<()> {
    let mut seen = std::collections::BTreeMap::new();
    for field in fields {
        let name = field.external_name_string();
        if let Some(first) = seen.insert(name.clone(), field.ident.clone()) {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "duplicate option name `{name}`; `{}` already uses this name",
                    first
                ),
            ));
        }
    }
    Ok(())
}

/// Options struct with no fields — empty schema, default decoder.
fn generate_unit(name: &Ident) -> TokenStream {
    quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self
            }
        }
        impl ::murphy_plugin_api::CopOptions for #name {}
    }
}

/// Supported option field type.
#[derive(Clone)]
enum FieldType {
    Bool,
    Int,
    Str,
    StrList,
    OptBool,
    OptInt,
    OptStr,
    Enum(Path),
}

impl FieldType {
    /// Schema `ty` wire string.
    fn wire(&self) -> &'static str {
        match self {
            FieldType::Bool | FieldType::OptBool => "bool",
            FieldType::Int | FieldType::OptInt => "int",
            FieldType::Str | FieldType::OptStr | FieldType::Enum(_) => "string",
            FieldType::StrList => "string_list",
        }
    }

    fn is_optional(&self) -> bool {
        matches!(
            self,
            FieldType::OptBool | FieldType::OptInt | FieldType::OptStr
        )
    }

    fn is_string(&self) -> bool {
        matches!(self, FieldType::Str | FieldType::OptStr)
    }

    fn is_enum(&self) -> bool {
        matches!(self, FieldType::Enum(_))
    }
}

/// A parsed `#[option(default = ...)]` literal.
enum DefaultValue {
    Bool(bool),
    Int(i64),
    Str(String),
    StrList(Vec<String>),
}

/// How a field was marked deprecated, if at all.
struct ParsedField {
    ident: Ident,
    external_name: Option<String>,
    ty: FieldType,
    default: Option<DefaultValue>,
    description: Option<String>,
    enum_values: Option<Vec<String>>,
    /// `Some("")` means a bare `#[option(deprecated)]`; `Some(other)`
    /// carries a replacement key; `None` means not deprecated.
    replacement: Option<String>,
    reason: Option<String>,
}

impl ParsedField {
    fn external_name_string(&self) -> String {
        self.external_name
            .clone()
            .unwrap_or_else(|| self.ident.to_string())
    }

    fn parse(field: &syn::Field) -> syn::Result<Self> {
        let ident = field.ident.clone().expect("named field has an identifier");
        let ty = parse_field_type(&field.ty)?;

        let mut parsed = ParsedField {
            ident,
            external_name: None,
            ty: ty.clone(),
            default: None,
            description: None,
            enum_values: None,
            replacement: None,
            reason: None,
        };

        for attr in &field.attrs {
            if !attr.path().is_ident("option") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("default") {
                    parsed.default = Some(parse_default(&meta, &ty)?);
                } else if meta.path.is_ident("name") {
                    parsed.external_name = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("description") {
                    parsed.description = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("enum_values") {
                    parsed.enum_values = Some(parse_str_array(&meta)?);
                } else if meta.path.is_ident("deprecated") {
                    // `deprecated` (bare) or `deprecated = "replacement"`.
                    if meta.input.peek(syn::Token![=]) {
                        parsed.replacement = Some(parse_str(&meta)?);
                    } else {
                        parsed.replacement = Some(String::new());
                    }
                } else if meta.path.is_ident("reason") {
                    parsed.reason = Some(parse_str(&meta)?);
                } else {
                    return Err(meta.error("unknown #[option(...)] key"));
                }
                Ok(())
            })?;
        }

        parsed.validate(field)?;
        Ok(parsed)
    }

    /// Cross-field-attribute consistency checks.
    fn validate(&self, field: &syn::Field) -> syn::Result<()> {
        if self.enum_values.is_some() && !self.ty.is_string() {
            return Err(syn::Error::new_spanned(
                field,
                "#[option(enum_values = ...)] is only valid on `String` fields",
            ));
        }
        if self.ty.is_enum() && !matches!(self.default, Some(DefaultValue::Str(_)) | None) {
            return Err(syn::Error::new_spanned(
                field,
                "#[option(default = ...)] for CopOptionEnum fields must be a string literal",
            ));
        }
        if let (Some(values), Some(DefaultValue::Str(d))) = (&self.enum_values, &self.default)
            && !values.iter().any(|v| v == d)
        {
            return Err(syn::Error::new_spanned(
                field,
                format!("#[option(default = \"{d}\")] is not one of enum_values"),
            ));
        }
        Ok(())
    }
}

/// Recognise a supported field type from its `syn::Type`.
fn parse_field_type(ty: &Type) -> syn::Result<FieldType> {
    let path = match ty {
        Type::Path(p) if p.qself.is_none() => &p.path,
        _ => return Err(unsupported_type(ty)),
    };
    let segment = path.segments.last().ok_or_else(|| unsupported_type(ty))?;
    let name = segment.ident.to_string();

    match name.as_str() {
        "bool" => Ok(FieldType::Bool),
        "i64" => Ok(FieldType::Int),
        "String" => Ok(FieldType::Str),
        "Vec" => {
            let inner = single_generic_arg(segment, ty)?;
            match parse_field_type(inner)? {
                FieldType::Str => Ok(FieldType::StrList),
                _ => Err(syn::Error::new_spanned(
                    ty,
                    "#[derive(CopOptions)] only supports `Vec<String>`",
                )),
            }
        }
        "Option" => {
            let inner = single_generic_arg(segment, ty)?;
            match parse_field_type(inner)? {
                FieldType::Bool => Ok(FieldType::OptBool),
                FieldType::Int => Ok(FieldType::OptInt),
                FieldType::Str => Ok(FieldType::OptStr),
                _ => Err(syn::Error::new_spanned(
                    ty,
                    "#[derive(CopOptions)] only supports `Option<bool>`, \
                     `Option<i64>`, or `Option<String>`",
                )),
            }
        }
        _ if segment.arguments.is_empty() && is_custom_type_name(&segment.ident) => {
            Ok(FieldType::Enum(path.clone()))
        }
        _ => Err(unsupported_type(ty)),
    }
}

fn is_custom_type_name(ident: &Ident) -> bool {
    ident
        .to_string()
        .chars()
        .next()
        .is_some_and(char::is_uppercase)
}

fn unsupported_type(ty: &Type) -> syn::Error {
    syn::Error::new(
        ty.span(),
        "#[derive(CopOptions)] supports only bool, i64, String, \
         Vec<String>, Option<bool|i64|String>, and CopOptionEnum fields",
    )
}

fn single_generic_arg<'a>(segment: &'a syn::PathSegment, ty: &Type) -> syn::Result<&'a Type> {
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(unsupported_type(ty));
    };
    let mut type_args = args.args.iter().filter_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    });
    let inner = type_args.next().ok_or_else(|| unsupported_type(ty))?;
    if type_args.next().is_some() {
        return Err(unsupported_type(ty));
    }
    Ok(inner)
}

fn parse_default(
    meta: &syn::meta::ParseNestedMeta<'_>,
    ty: &FieldType,
) -> syn::Result<DefaultValue> {
    let value = meta.value()?;
    match ty {
        FieldType::Bool | FieldType::OptBool => {
            let lit: syn::LitBool = value.parse()?;
            Ok(DefaultValue::Bool(lit.value))
        }
        FieldType::Int | FieldType::OptInt => {
            let lit: syn::LitInt = value.parse()?;
            Ok(DefaultValue::Int(lit.base10_parse()?))
        }
        FieldType::Str | FieldType::OptStr | FieldType::Enum(_) => {
            let lit: syn::LitStr = value.parse()?;
            Ok(DefaultValue::Str(lit.value()))
        }
        FieldType::StrList => {
            let array: ExprArray = value.parse()?;
            let mut out = Vec::with_capacity(array.elems.len());
            for elem in array.elems {
                out.push(expr_to_string(&elem)?);
            }
            Ok(DefaultValue::StrList(out))
        }
    }
}

fn parse_str(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<String> {
    let lit: syn::LitStr = meta.value()?.parse()?;
    Ok(lit.value())
}

fn parse_str_array(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<String>> {
    let array: ExprArray = meta.value()?.parse()?;
    array.elems.iter().map(expr_to_string).collect()
}

fn expr_to_string(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.value()),
        _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
    }
}

fn generate_default(name: &Ident, fields: &[ParsedField]) -> TokenStream {
    let inits = fields.iter().map(|f| {
        let ident = &f.ident;
        let value = match (&f.default, &f.ty) {
            (Some(DefaultValue::Bool(b)), _) => quote! { #b },
            (Some(DefaultValue::Int(i)), _) => quote! { #i },
            (Some(DefaultValue::Str(s)), FieldType::Enum(path)) => {
                quote! {
                    <#path as ::murphy_plugin_api::CopOptionEnum>::from_str(#s)
                        .expect("CopOptionEnum default must be one of its values")
                }
            }
            (Some(DefaultValue::Str(s)), _) => {
                quote! { ::std::string::String::from(#s) }
            }
            (Some(DefaultValue::StrList(items)), _) => {
                quote! { ::std::vec![ #(::std::string::String::from(#items)),* ] }
            }
            (None, _) => quote! { ::core::default::Default::default() },
        };
        quote! { #ident: #value }
    });
    quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self { #(#inits),* }
            }
        }
    }
}

fn generate_copoptions(name: &Ident, fields: &[ParsedField]) -> TokenStream {
    let schema_entries = fields.iter().map(schema_entry);
    let decoders = fields.iter().map(field_decoder);

    quote! {
        impl ::murphy_plugin_api::CopOptions for #name {
            const SCHEMA: &'static [::murphy_plugin_api::OptionSpec] = &[
                #(#schema_entries),*
            ];

            fn from_config_json(
                __bytes: &[u8],
            ) -> ::core::result::Result<Self, ::murphy_plugin_api::ConfigError> {
                let __value: ::serde_json::Value = ::serde_json::from_slice(__bytes)
                    .map_err(::murphy_plugin_api::ConfigError::parse)?;
                let __obj = __value
                    .as_object()
                    .ok_or_else(::murphy_plugin_api::ConfigError::not_an_object)?;
                ::core::result::Result::Ok(Self {
                    #(#decoders),*
                })
            }
        }
    }
}

fn schema_entry(field: &ParsedField) -> TokenStream {
    let name = field.external_name_string();
    // `ty` is always the base wire type. Enum-ness is carried by a
    // non-empty `enum_values_json`, not by a distinct `ty` string — the
    // single-surface `OptionSpec.ty` is `bool|int|string|string_list`
    // (the pre-reboot ABI used `ty = "enum"`).
    let ty = field.ty.wire();
    let default_json = match &field.default {
        Some(DefaultValue::Bool(b)) => serde_json::to_string(b).unwrap(),
        Some(DefaultValue::Int(i)) => serde_json::to_string(i).unwrap(),
        Some(DefaultValue::Str(s)) => serde_json::to_string(s).unwrap(),
        Some(DefaultValue::StrList(items)) => serde_json::to_string(items).unwrap(),
        None => String::new(),
    };
    let description = field.description.clone().unwrap_or_default();
    let enum_values_json = match (&field.ty, &field.enum_values) {
        (FieldType::Enum(path), _) => {
            quote! { <#path as ::murphy_plugin_api::CopOptionEnum>::VALUES_JSON }
        }
        (_, Some(values)) => {
            let values = serde_json::to_string(values).unwrap();
            quote! { #values }
        }
        (_, None) => quote! { "" },
    };
    let replacement = field.replacement.clone().unwrap_or_default();
    let reason = field.reason.clone().unwrap_or_default();

    quote! {
        ::murphy_plugin_api::OptionSpec {
            name: ::murphy_plugin_api::RawSlice::from_str(#name),
            ty: ::murphy_plugin_api::RawSlice::from_str(#ty),
            default_json: ::murphy_plugin_api::RawSlice::from_str(#default_json),
            description: ::murphy_plugin_api::RawSlice::from_str(#description),
            enum_values_json: ::murphy_plugin_api::RawSlice::from_str(#enum_values_json),
            replacement: ::murphy_plugin_api::RawSlice::from_str(#replacement),
            reason: ::murphy_plugin_api::RawSlice::from_str(#reason),
        }
    }
}

fn field_decoder(field: &ParsedField) -> TokenStream {
    let ident = &field.ident;
    let key = field.external_name_string();
    let wire = field.ty.wire();

    // The expression used when the key is absent from the config object.
    let on_absent: TokenStream = match (&field.default, field.ty.is_optional()) {
        (Some(DefaultValue::Bool(b)), _) => quote! { #b },
        (Some(DefaultValue::Int(i)), _) => quote! { #i },
        (Some(DefaultValue::Str(s)), _) => match &field.ty {
            FieldType::Enum(path) => quote! {
                <#path as ::murphy_plugin_api::CopOptionEnum>::from_str(#s)
                    .expect("CopOptionEnum default must be one of its values")
            },
            _ => quote! { ::std::string::String::from(#s) },
        },
        (Some(DefaultValue::StrList(items)), _) => {
            quote! { ::std::vec![ #(::std::string::String::from(#items)),* ] }
        }
        (None, true) => quote! { ::core::option::Option::None },
        (None, false) => quote! {
            return ::core::result::Result::Err(
                ::murphy_plugin_api::ConfigError::missing_required(#key)
            )
        },
    };

    let present = present_decoder(field, &key, wire);

    quote! {
        #ident: match __obj.get(#key) {
            ::core::option::Option::Some(__v) => { #present }
            ::core::option::Option::None => { #on_absent }
        }
    }
}

/// Decoder body for a present-and-non-null value.
fn present_decoder(field: &ParsedField, key: &str, wire: &str) -> TokenStream {
    let mismatch = quote! {
        ::murphy_plugin_api::ConfigError::type_mismatch(#key, #wire)
    };

    match &field.ty {
        FieldType::Bool => quote! {
            __v.as_bool().ok_or_else(|| #mismatch)?
        },
        FieldType::Int => quote! {
            __v.as_i64().ok_or_else(|| #mismatch)?
        },
        FieldType::Str => string_decoder(field, key, &mismatch, false),
        FieldType::StrList => quote! {
            {
                let __arr = __v.as_array().ok_or_else(|| #mismatch)?;
                let mut __out = ::std::vec::Vec::with_capacity(__arr.len());
                for __item in __arr {
                    __out.push(::std::string::String::from(
                        __item.as_str().ok_or_else(|| #mismatch)?,
                    ));
                }
                __out
            }
        },
        FieldType::OptBool => quote! {
            if __v.is_null() {
                ::core::option::Option::None
            } else {
                ::core::option::Option::Some(__v.as_bool().ok_or_else(|| #mismatch)?)
            }
        },
        FieldType::OptInt => quote! {
            if __v.is_null() {
                ::core::option::Option::None
            } else {
                ::core::option::Option::Some(__v.as_i64().ok_or_else(|| #mismatch)?)
            }
        },
        FieldType::OptStr => {
            let inner = string_decoder(field, key, &mismatch, true);
            quote! {
                if __v.is_null() {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(#inner)
                }
            }
        }
        FieldType::Enum(path) => quote! {
            {
                let __s = __v.as_str().ok_or_else(|| #mismatch)?;
                <#path as ::murphy_plugin_api::CopOptionEnum>::from_str(__s)
                    .ok_or_else(|| ::murphy_plugin_api::ConfigError::enum_violation(#key, __s))?
            }
        },
    }
}

/// Decoder for a `String` value, applying `enum_values` if present.
fn string_decoder(
    field: &ParsedField,
    key: &str,
    mismatch: &TokenStream,
    _optional: bool,
) -> TokenStream {
    let enum_check = match &field.enum_values {
        Some(values) => {
            let allowed = values.iter().map(|v| quote! { #v });
            quote! {
                if ![ #(#allowed),* ].contains(&__s) {
                    return ::core::result::Result::Err(
                        ::murphy_plugin_api::ConfigError::enum_violation(#key, __s),
                    );
                }
            }
        }
        None => quote! {},
    };
    quote! {
        {
            let __s = __v.as_str().ok_or_else(|| #mismatch)?;
            #enum_check
            ::std::string::String::from(__s)
        }
    }
}
