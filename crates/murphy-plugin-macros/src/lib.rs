//! Proc macros for Murphy native plugins, targeting the single-surface
//! plugin ABI (ADR 0038).
//!
//! This crate ships [`register_cops!`], the macro plugin authors invoke
//! to turn their cop implementations into the static
//! `murphy_plugin_api::PluginRegistration` table Murphy's loader expects,
//! and [`derive@CopOptions`], which generates a cop's option schema and
//! JSON decoder. [`derive@CopOptionEnum`] generates typed string enum
//! options for use inside `CopOptions` structs.
//!
//! `#[murphy::cop]` / `#[on_node]` (murphy-9cr.8) will land here
//! alongside them.

mod cop_attr;
mod cop_option_enum;
mod cop_options;
mod node_pattern;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, Token, parse_macro_input};

/// Declare the Murphy plugin registration entry point for a cop pack.
///
/// `mode = static|dynamic` is **required** and selects the symbol shape of
/// the generated `murphy_plugin_register` entry (design §5):
///
/// - `mode = dynamic` — emits `#[no_mangle] pub unsafe extern "C" fn
///   murphy_plugin_register`. Used by external `.so` plugin packs; the
///   murphy-cli loader resolves it through `dlsym`.
/// - `mode = static` — emits a plain Rust `pub fn murphy_plugin_register`
///   at the macro caller's scope, with **no `#[no_mangle]` symbol**. Used
///   by statically-linked built-in packs (`murphy-std`); murphy-cli calls
///   the Rust path directly. Avoids C-symbol collision when multiple static
///   packs are linked into the same binary.
///
/// Both modes also declare `pub static PACK_COPS: [PluginCopV1]` as a
/// `#[linkme::distributed_slice]`. Each cop file calls
/// `murphy_plugin_api::submit_cop!(T)` to register itself; the linker
/// collects all submissions into `PACK_COPS` at link time. No central cop
/// list is needed in `lib.rs` — add a cop by editing only its own file.
///
/// # Example
///
/// ```ignore
/// // lib.rs — just the entry point declaration, no cop list:
/// murphy_plugin_api::register_cops!(mode = dynamic);
///
/// // cops/lint/no_tabs.rs — self-registration in the cop's own file:
/// #[derive(Default)]
/// struct NoTabs;
/// // ... impl Cop + NodeCop ...
/// murphy_plugin_api::submit_cop!(NoTabs);
/// ```
#[proc_macro]
pub fn register_cops(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RegisterCopsInput);

    // Shared debug-mode uniqueness check emitted into murphy_plugin_register.
    let dup_check = quote! {
        #[cfg(debug_assertions)]
        {
            let mut seen = ::std::collections::HashSet::new();
            for cop in PACK_COPS.iter() {
                let name = unsafe { cop.name.as_bytes() };
                if !seen.insert(name) {
                    panic!(
                        "register_cops!: two cops share the same NAME: {}",
                        ::std::str::from_utf8(name).unwrap_or("<invalid utf8>")
                    );
                }
            }
        }
    };

    let expanded = match input.mode {
        RegisterMode::Dynamic => quote! {
            /// This pack's cop distributed slice. Each `submit_cop!(T)` call
            /// contributes an entry; the linker collects them at link time.
            #[::murphy_plugin_api::linkme::distributed_slice]
            pub(crate) static PACK_COPS: [::murphy_plugin_api::PluginCopV1];

            const _: () = {
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn murphy_plugin_register(
                    out: *mut ::murphy_plugin_api::PluginRegistration,
                ) -> i32 {
                    if out.is_null() {
                        return 1;
                    }
                    #dup_check
                    unsafe {
                        *out = ::murphy_plugin_api::PluginRegistration {
                            abi_version: ::murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
                            cops_ptr: PACK_COPS.as_ptr(),
                            cops_len: PACK_COPS.len(),
                        };
                    }
                    0
                }
            };
        },
        RegisterMode::Static => quote! {
            /// This pack's cop distributed slice. Each `submit_cop!(T)` call
            /// contributes an entry; the linker collects them at link time.
            #[::murphy_plugin_api::linkme::distributed_slice]
            pub static PACK_COPS: [::murphy_plugin_api::PluginCopV1];

            /// Fill the host-provided `PluginRegistration` with this pack's
            /// cops. Static-mode entry point: the host (murphy-cli) calls
            /// this directly through the Rust path — no `dlsym`, no
            /// `#[no_mangle]` symbol that could collide with another
            /// statically-linked pack (design §5).
            ///
            /// # Safety
            ///
            /// `out` must be either null (treated as a usage error and
            /// rejected with `1`) or point to a writable `PluginRegistration`
            /// slot valid for the duration of the call.
            pub unsafe fn murphy_plugin_register(
                out: *mut ::murphy_plugin_api::PluginRegistration,
            ) -> i32 {
                if out.is_null() {
                    return 1;
                }
                #dup_check
                // Safety: `out` is non-null per the check above.
                unsafe {
                    *out = ::murphy_plugin_api::PluginRegistration {
                        abi_version: ::murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
                        cops_ptr: PACK_COPS.as_ptr(),
                        cops_len: PACK_COPS.len(),
                    };
                }
                0
            }
        },
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
/// `bool`, `i64`, `String`, `Vec<String>`, `Option<bool|i64|String>`, and
/// enums deriving `CopOptionEnum`.
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

/// Derive `murphy_plugin_api::CopOptionEnum` for a string-backed enum option.
///
/// Each unit variant must specify its wire value with
/// `#[option(value = "...")]`. `#[derive(CopOptions)]` uses the generated
/// metadata to publish `enum_values_json` and decode the option into the
/// typed enum.
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_macros::{CopOptionEnum, CopOptions};
///
/// #[derive(CopOptionEnum, Clone, Copy)]
/// enum Style {
///     #[option(value = "no_space")]
///     NoSpace,
///     #[option(value = "space")]
///     Space,
/// }
///
/// #[derive(CopOptions)]
/// struct Options {
///     #[option(default = "no_space")]
///     style: Style,
/// }
/// ```
#[proc_macro_derive(CopOptionEnum, attributes(option))]
pub fn derive_cop_option_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    cop_option_enum::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Define a compile-time AST pattern matcher (B backend, murphy-9cr.18).
///
/// `def_node_matcher!(name, "pattern")` expands to a module-level
/// `fn name(node, cx)` that tests whether `node` matches the
/// S-expression `pattern`. With zero `$` captures the matcher returns
/// `bool`; with one or more it returns `Option<(captures…)>` in slot
/// order (`$_` → `NodeId`, `$...` → `&[NodeId]`).
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_macros::def_node_matcher;
///
/// def_node_matcher!(is_puts_call, "(send nil? :puts $...)");
/// ```
#[proc_macro]
pub fn def_node_matcher(input: TokenStream) -> TokenStream {
    node_pattern::node_pattern(input.into()).into()
}

/// Attribute macro for defining a Murphy native cop (murphy-9cr.8).
///
/// Apply to an inherent `impl` block. The macro generates
/// `impl murphy_plugin_api::Cop` (metadata) and
/// `impl murphy_plugin_api::NodeCop` (`KINDS` array + a `check` dispatch
/// that routes by `NodeKindTag`), then re-emits the original `impl` block
/// with `#[on_node]` attributes stripped. The decorated type must derive
/// (or otherwise implement) `Default` — `register_cops!` and the dispatch
/// thunk construct a fresh cop value per matched node (ADR 0035).
///
/// # Arguments
///
/// - `name = "..."` (**required**) — the cop identifier.
/// - `description = "..."` — one-line summary; defaults to the trait default.
/// - `default_severity = "warning" | "error"` — severity default; absent =
///   trait default.
/// - `default_enabled = true | false` — enablement default; absent = trait
///   default.
/// - `options = <path>` — type to use for `Cop::Options` (defaults to
///   `::murphy_plugin_api::NoOptions`).
///
/// # Requirements
///
/// - The `impl` must be **inherent** (`impl T { ... }`), non-generic, and
///   not `unsafe`. `#[cop]` on a struct, trait impl, or generic impl is a
///   compile error.
/// - At least one method inside the `impl` must carry `#[on_node]` (for
///   per-kind dispatch) or `#[on_new_investigation]` (modelled on
///   RuboCop's hook of the same name; runs once per file with access
///   to `cx.comments()`).
/// - `#[on_node]` and `#[on_new_investigation]` cannot be mixed in the
///   same impl, and at most one method per impl may use
///   `#[on_new_investigation]`.
/// - `#[on_node]` methods must have the signature
///   `fn(&self, NodeId, &Cx<'_>)`; `#[on_new_investigation]` methods
///   take only `fn(&self, &Cx<'_>)` (no `NodeId` — the file is the
///   subject).
///
/// `#[on_new_investigation]` is the file-scoped entry point. It is
/// intended for cops that iterate `cx.comments()` or similar
/// file-level structured data; reaching for `cx.source()` /
/// `cx.raw_source()` here is the escape-hatch path and should be a
/// hand-rolled `KINDS = &[]` cop instead (the macro's structured
/// dispatch is the guardrail).
///
/// # Example (per-kind dispatch)
///
/// ```ignore
/// use murphy_plugin_api::{Cx, NodeId};
/// use murphy_plugin_macros::cop;
///
/// #[derive(Default)]
/// struct NoTabs;
///
/// #[cop(name = "Plugin/NoTabs", description = "flag literal tabs")]
/// impl NoTabs {
///     #[on_node(kind = "send")]
///     fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) { /* … */ }
/// }
/// ```
///
/// # Example (per-file investigation)
///
/// ```ignore
/// use murphy_plugin_api::Cx;
/// use murphy_plugin_macros::cop;
///
/// #[derive(Default)]
/// struct TodoFormat;
///
/// #[cop(name = "Example/TodoFormat")]
/// impl TodoFormat {
///     #[on_new_investigation]
///     fn investigate(&self, cx: &Cx<'_>) {
///         for c in cx.comments() { /* … */ }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn cop(args: TokenStream, item: TokenStream) -> TokenStream {
    cop_attr::cop(args.into(), item.into()).into()
}

/// Declare that a method inside a `#[cop]` impl handles a particular AST
/// node kind (murphy-9cr.8).
///
/// This macro **must** be used inside a `#[cop]` impl block.  When used
/// correctly the `#[cop]` macro consumes `#[on_node]` before it reaches
/// this entry point, so this proc-macro is only reachable on misuse and
/// always emits a compile error.
///
/// # Example
///
/// ```ignore
/// #[cop(name = "Plugin/NoTabs")]
/// impl NoTabs {
///     #[on_node(kind = "send")]
///     fn check_send(&self, node: NodeId, cx: &Cx<'_>) { /* … */ }
/// }
/// ```
#[proc_macro_attribute]
pub fn on_node(args: TokenStream, item: TokenStream) -> TokenStream {
    cop_attr::on_node(args.into(), item.into()).into()
}

/// Declare that a method inside a `#[cop]` impl is the per-file
/// investigation callback (modelled on RuboCop's hook of the same
/// name). Lowers to `KINDS = &[]`; the host calls the trait `check`
/// exactly once per file (with `node == cx.root()`, ignored here),
/// which then delegates to the user method passing only `cx`.
///
/// Takes no arguments. Must be the only dispatched method in the impl
/// block (cannot coexist with `#[on_node]`).
///
/// The signature must be `fn(&self, &Cx<'_>)` — no `NodeId` parameter.
/// Use `cx.comments()` to walk the file's comments; reaching for
/// `cx.source()` / `cx.raw_source()` is the escape-hatch path and
/// should be a hand-rolled `KINDS = &[]` cop instead.
///
/// Like `#[on_node]`, this entry point is consumed by `#[cop]` before
/// the proc-macro runs; reaching it directly is always a misuse and
/// produces a compile error.
///
/// # Example
///
/// ```ignore
/// #[cop(name = "Plugin/Comments")]
/// impl Comments {
///     #[on_new_investigation]
///     fn investigate(&self, cx: &Cx<'_>) {
///         for comment in cx.comments() { /* … */ }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn on_new_investigation(args: TokenStream, item: TokenStream) -> TokenStream {
    cop_attr::on_new_investigation(args.into(), item.into()).into()
}

/// Whether `register_cops!` emits a `#[no_mangle]` C symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegisterMode {
    /// `.so` plugin pack — emit `#[no_mangle] extern "C" fn
    /// murphy_plugin_register`.
    Dynamic,
    /// Statically-linked built-in pack — emit a plain Rust
    /// `pub fn murphy_plugin_register`, no C symbol.
    Static,
}

/// Parsed form of `register_cops!(mode = static|dynamic)`.
/// Cop list removed — each cop file calls `submit_cop!(T)` instead.
struct RegisterCopsInput {
    mode: RegisterMode,
}

impl syn::parse::Parse for RegisterCopsInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        // `mode = static|dynamic` — required as the only argument.
        let mode_kw: Ident = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "register_cops!: first argument must be `mode = static` or `mode = dynamic`",
            )
        })?;
        if mode_kw != "mode" {
            return Err(syn::Error::new(
                mode_kw.span(),
                format!("register_cops!: expected `mode`, found `{mode_kw}`"),
            ));
        }
        let _eq: Token![=] = input.parse()?;
        // `static` is a Rust keyword and parses as `Token![static]`, not
        // `Ident`; accept it explicitly. `dynamic` is a plain identifier.
        let mode = if input.peek(Token![static]) {
            let _: Token![static] = input.parse()?;
            RegisterMode::Static
        } else {
            let mode_ident: Ident = input.parse()?;
            if mode_ident == "dynamic" {
                RegisterMode::Dynamic
            } else {
                return Err(syn::Error::new(
                    mode_ident.span(),
                    format!(
                        "register_cops!: mode must be `static` or `dynamic`, found `{mode_ident}`"
                    ),
                ));
            }
        };
        // Reject the old cop-list form with a helpful migration message.
        if !input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "register_cops!: cop list is no longer accepted — \
                 call submit_cop!(T) in each cop file instead",
            ));
        }
        Ok(RegisterCopsInput { mode })
    }
}
