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

mod cop_attr;
mod cop_options;
mod node_pattern;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, Path, Token, parse_macro_input, punctuated::Punctuated};

/// Register a list of cop types as a Murphy native plugin pack
/// (ADR 0038 single-surface ABI).
///
/// `mode = static|dynamic` is **required** as the first argument and
/// selects the symbol shape of the generated `murphy_plugin_register`
/// entry. Both modes generate the *same* static `PluginCopV1` table —
/// only the export shape differs (design §5 of
/// `docs/plans/2026-05-22-plugin-reboot-design.md`):
///
/// - `mode = dynamic` — emits `#[no_mangle] pub unsafe extern "C" fn
///   murphy_plugin_register` inside an anonymous `const` block. This is
///   the shape an external `.so` plugin pack exports; the murphy-cli
///   loader resolves it through `dlsym`.
/// - `mode = static` — emits a plain Rust `pub fn murphy_plugin_register`
///   at the macro caller's scope, with **no `#[no_mangle]` symbol**. This
///   is the shape used by statically-linked built-in packs (`murphy-std`,
///   future `murphy-rails` siblings); murphy-cli calls the Rust path
///   directly. Avoiding `#[no_mangle]` prevents a C-symbol collision when
///   multiple static packs are linked into the same binary.
///
/// The mode is a macro argument rather than a Cargo feature because
/// features unify across the dependency tree — a `plugin-dynamic`
/// feature flipped on by an unrelated crate would silently force every
/// static pack to emit a `#[no_mangle]` symbol and collide.
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
/// // .so plugin pack (cdylib):
/// murphy_plugin_macros::register_cops!(mode = dynamic, NoTabs);
///
/// // Statically-linked built-in pack:
/// // murphy_plugin_macros::register_cops!(mode = static, NoTabs);
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

    let uniqueness_check = quote! {
        // Compile-time uniqueness check. The duplicate-`NAME` `panic!`
        // is emitted inline here (not inside the helper) so the
        // const-eval error stays a clean `error[E0080]` with no
        // `core::panic` frame, keeping the trybuild snapshot stable
        // across `rust-src` presence (murphy-8np).
        const _: () = if !__api::__internal::cop_names_unique::<#n>(
            [ #(#name_exprs),* ]
        ) {
            ::core::panic!(
                "register_cops!: two registered cops share the same NAME"
            );
        };
    };

    let expanded = match input.mode {
        RegisterMode::Dynamic => quote! {
            const _: () = {
                use ::murphy_plugin_api as __api;

                #uniqueness_check

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
        },
        RegisterMode::Static => quote! {
            #[doc(hidden)]
            pub static __MURPHY_PLUGIN_COPS_V1: [::murphy_plugin_api::PluginCopV1; #n] = {
                use ::murphy_plugin_api as __api;
                [ #(#cop_entries),* ]
            };

            const _: () = {
                use ::murphy_plugin_api as __api;
                #uniqueness_check
            };

            /// Fill the host-provided `PluginRegistration` with this
            /// pack's cops. Static-mode entry point: the host (murphy-cli)
            /// calls this directly through the Rust path — no `dlsym`,
            /// no `#[no_mangle]` symbol that could collide with another
            /// statically-linked pack (design §5).
            ///
            /// # Safety
            ///
            /// The signature mirrors the dynamic-mode `extern "C"` entry:
            /// `out` must be either null (treated as a usage error and
            /// rejected with `1`) or point to a writable
            /// `PluginRegistration` slot valid for the duration of the
            /// call. The dynamic and static entry points share this
            /// contract so the host can route either through a single
            /// registration code path.
            pub unsafe fn murphy_plugin_register(
                out: *mut ::murphy_plugin_api::PluginRegistration,
            ) -> i32 {
                if out.is_null() {
                    return 1;
                }
                // Safety: `out` is non-null per the check above; the
                // caller upholds the pointee-writability part of the
                // contract documented on this function.
                unsafe {
                    *out = ::murphy_plugin_api::PluginRegistration {
                        abi_version: ::murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
                        cops_ptr: __MURPHY_PLUGIN_COPS_V1.as_ptr(),
                        cops_len: __MURPHY_PLUGIN_COPS_V1.len(),
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

/// Define a compile-time AST pattern matcher (B backend, murphy-9cr.18).
///
/// `node_pattern!(name, "pattern")` expands to a module-level
/// `fn name(node, cx)` that tests whether `node` matches the
/// S-expression `pattern`. With zero `$` captures the matcher returns
/// `bool`; with one or more it returns `Option<(captures…)>` in slot
/// order (`$_` → `NodeId`, `$...` → `&[NodeId]`).
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_macros::node_pattern;
///
/// node_pattern!(is_puts_call, "(send nil? :puts $...)");
/// ```
#[proc_macro]
pub fn node_pattern(input: TokenStream) -> TokenStream {
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

/// Parsed form of `register_cops!(mode = static|dynamic, Cop1, Cop2, …);`.
struct RegisterCopsInput {
    mode: RegisterMode,
    cops: Punctuated<Path, Token![,]>,
}

impl syn::parse::Parse for RegisterCopsInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        // `mode = static|dynamic ,` — required as the first argument.
        let mode_kw: Ident = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "register_cops!: first argument must be `mode = static` or \
                 `mode = dynamic` (design §5: macro argument, not Cargo feature, \
                 to avoid feature-unification surprises)",
            )
        })?;
        if mode_kw != "mode" {
            return Err(syn::Error::new(
                mode_kw.span(),
                format!("register_cops!: expected first argument `mode`, found `{mode_kw}`"),
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
        let _comma: Token![,] = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "register_cops!: expected `,` after `mode = …` and at least one cop",
            )
        })?;

        let cops = Punctuated::parse_terminated(input)?;
        Ok(RegisterCopsInput { mode, cops })
    }
}
