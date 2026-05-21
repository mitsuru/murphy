//! Proc macros for Murphy native plugins.
//!
//! This crate ships [`register_cops!`], the macro plugin authors invoke to
//! turn their `Cop` implementations into the static `MurphyPluginV1` table
//! Murphy's loader expects.
//!
//! `#[derive(CopOptions)]` (murphy-9cr.7) and `#[murphy::cop]` /
//! `#[on_node]` (murphy-9cr.8) will land here alongside it.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Path, Token, parse_macro_input, punctuated::Punctuated};

/// Register a comma-separated list of [`Cop`](murphy_plugin_api::Cop)
/// implementations as a Murphy native plugin.
///
/// Expands to a `const _: () = { … };` block that defines the static cop
/// table and exports an `extern "C" fn murphy_plugin_register` matching
/// Murphy's ABI (see ADR 0031, ADR 0033).
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_api::{Cop, NoOptions};
///
/// struct NoTabs;
/// impl Cop for NoTabs {
///     type Options = NoOptions;
///     const NAME: &'static str = "Plugin/NoTabs";
/// }
///
/// murphy_plugin_macros::register_cops!(NoTabs);
/// ```
///
/// All listed types must implement
/// [`Cop`](murphy_plugin_api::Cop); their `NAME` constants must be
/// pairwise distinct. Both invariants are enforced at compile time —
/// the first by a trait bound on the generated table, the second by a
/// const panic in
/// [`__internal::assert_unique_cop_names`](murphy_plugin_api::__internal::assert_unique_cop_names).
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

            // Compile-time uniqueness check; surfaces duplicates as
            // const-eval panics pointing at this block.
            const _: () = __api::__internal::assert_unique_cop_names::<#n>(
                [ #(#name_exprs),* ]
            );

            static COPS: [__api::MurphyPluginCopV1; #n] = [
                #(#cop_entries),*
            ];

            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn murphy_plugin_register(
                out: *mut __api::MurphyPluginV1,
            ) -> i32 {
                if out.is_null() {
                    return 1;
                }
                unsafe {
                    *out = __api::MurphyPluginV1 {
                        size: ::core::mem::size_of::<__api::MurphyPluginV1>(),
                        cops_ptr: COPS.as_ptr(),
                        cops_len: COPS.len(),
                        call_dispatch_ptr: ::core::ptr::null(),
                        call_dispatch_len: 0,
                        run_call_dispatch: ::core::option::Option::None,
                        node_dispatch_ptr: ::core::ptr::null(),
                        node_dispatch_len: 0,
                        run_node_dispatch: ::core::option::Option::None,
                    };
                }
                0
            }
        };
    };

    expanded.into()
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
