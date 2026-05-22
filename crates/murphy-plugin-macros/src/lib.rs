//! Proc macros for Murphy native plugins, targeting the single-surface
//! plugin ABI (ADR 0038).
//!
//! This crate ships [`register_cops!`], the macro plugin authors invoke
//! to turn their cop implementations into the static
//! `murphy_plugin_api::PluginRegistration` table Murphy's loader expects,
//! and [`derive@CopOptions`], which generates a cop's option schema and
//! JSON decoder.
//!
//! `#[murphy::cop]` / `#[on_node]` (murphy-9cr.8) will land here
//! alongside them.

mod cop_options;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Path, Token, parse_macro_input, punctuated::Punctuated};

/// Register a comma-separated list of cop types as a Murphy native
/// plugin (ADR 0038 single-surface ABI).
///
/// Expands to a `const _: () = { … };` block that defines the static
/// `[PluginCopV1; N]` cop table and exports a `#[no_mangle]`
/// `extern "C" fn murphy_plugin_register` that fills a
/// `murphy_plugin_api::PluginRegistration`.
///
/// # Example
///
/// ```ignore
/// use murphy_ast::NodeId;
/// use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeKindTag};
///
/// #[derive(Default)]
/// struct NoTabs;
///
/// impl Cop for NoTabs {
///     type Options = NoOptions;
///     const NAME: &'static str = "Plugin/NoTabs";
/// }
///
/// impl NodeCop for NoTabs {
///     const KINDS: &'static [NodeKindTag] = &[];
///     fn check(&self, node: NodeId, cx: &Cx<'_>) {}
/// }
///
/// murphy_plugin_macros::register_cops!(NoTabs);
/// ```
///
/// Every listed type must implement `murphy_plugin_api::NodeCop` (hence
/// `Cop`) and [`Default`] — the dispatch thunk constructs a fresh,
/// stateless cop per matched node. `KINDS` / `check` are hand-written
/// here; `#[on_node]` / `#[murphy::cop]` (murphy-9cr.8) will generate
/// them.
///
/// Each cop's `NAME` must be pairwise distinct; `register_cops!`
/// enforces this at compile time with an inline const-eval `panic!`
/// guarded by `murphy_plugin_api::__internal::cop_names_unique`.
#[proc_macro]
pub fn register_cops(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RegisterCopsInput);
    let cops: Vec<&Path> = input.cops.iter().collect();
    let n = cops.len();

    let cop_entries = cops.iter().map(|cop| {
        quote! { __api::__internal::build_cop::<#cop>() }
    });

    let name_exprs = cops.iter().map(|cop| {
        quote! { <#cop as __api::Cop>::NAME }
    });

    let expanded = quote! {
        const _: () = {
            use ::murphy_plugin_api as __api;

            // Compile-time uniqueness check. The duplicate-`NAME`
            // `panic!` is emitted inline here (not inside the helper)
            // so the const-eval error stays a clean `error[E0080]`
            // with no `core::panic` frame, keeping the trybuild
            // snapshot stable across `rust-src` presence (murphy-8np).
            const _: () = if !__api::__internal::cop_names_unique::<#n>(
                [ #(#name_exprs),* ]
            ) {
                ::core::panic!(
                    "register_cops!: two registered cops share the same NAME"
                );
            };

            static COPS: [__api::PluginCopV1; #n] = [
                #(#cop_entries),*
            ];

            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn murphy_plugin_register(
                out: *mut __api::PluginRegistration,
            ) -> i32 {
                if out.is_null() {
                    return 1;
                }
                unsafe {
                    *out = __api::PluginRegistration {
                        abi_version: __api::MURPHY_PLUGIN_ABI_VERSION,
                        cops_ptr: COPS.as_ptr(),
                        cops_len: COPS.len(),
                    };
                }
                0
            }
        };
    };

    expanded.into()
}

/// Derive `murphy_plugin_api::CopOptions` for an options struct.
///
/// Generates `impl Default` (honouring `#[option(default = …)]`) and
/// `impl CopOptions` (the `SCHEMA` const plus a `from_config_json`
/// JSON decoder).
///
/// # Supported field types
///
/// `bool`, `i64`, `String`, `Vec<String>`, and `Option<bool|i64|String>`.
///
/// # `#[option(...)]` keys
///
/// - `default = <literal>` — bool / integer / string / string list.
/// - `description = "..."`.
/// - `enum_values = ["a", "b"]` — `String` fields only.
/// - `deprecated` or `deprecated = "replacement_key"`.
/// - `reason = "..."`.
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_macros::CopOptions;
///
/// #[derive(CopOptions)]
/// struct LineLengthOptions {
///     #[option(default = 80, description = "Maximum line width")]
///     max: i64,
/// }
/// ```
#[proc_macro_derive(CopOptions, attributes(option))]
pub fn derive_cop_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    cop_options::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Parsed form of `register_cops!(Cop1, Cop2, …);`.
struct RegisterCopsInput {
    cops: Punctuated<Path, Token![,]>,
}

impl syn::parse::Parse for RegisterCopsInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        Ok(RegisterCopsInput {
            cops: Punctuated::parse_terminated(input)?,
        })
    }
}
