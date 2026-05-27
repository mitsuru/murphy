//! `Lint/Debugger` — flag calls that drop into a debugger / REPL or
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Debugger
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-rjwo
//! notes: >
//!   Known gaps remain around RuboCop configuration shape and false-positive guards.
//! ```
//!
//! `require`s that load one. Defaults mirror RuboCop's `DebuggerMethods`
//! and `DebuggerRequires` so a `.rubocop.yml`-aware Ruby developer
//! sees the same set of offenses.
//!
//! ## Matched shapes
//!
//! - **Bare** debugger entrypoints: `debugger`, `byebug`, `remote_byebug`,
//!   `pry`, `save_and_open_page`, `save_and_open_screenshot`, `jard`, …
//! - **Chained** entrypoints: `binding.irb`, `binding.pry`,
//!   `binding.remote_pry`, `binding.pry_remote`, `binding.b`,
//!   `binding.break`, `binding.console`.
//! - **Const-receiver** entrypoints: `Kernel.debugger`, `Kernel.binding`,
//!   `Pry.rescue` (and any nested `Foo::Bar.method` path).
//! - **Debugger requires**: `require 'debug'`, `require 'debug/open'`,
//!   `require 'debug/open_nonstop'`, `require 'byebug'`,
//!   `require 'pry'`, `require 'pry-byebug'`, `require 'capybara/dsl'`.
//!
//! ## Message
//!
//! Matches RuboCop's wording: `Remove debugger entry point `<source>`.`,
//! where `<source>` is the raw source text of the offending call.
//!
//! ## Options
//!
//! - **`DebuggerMethods`** — replaces the default set of debugger
//!   entrypoints to match. Each entry is either a bare method name
//!   (`"debugger"`) or a `<receiver>.<method>` signature
//!   (`"binding.pry"`, `"Kernel.debugger"`). Constant receivers can be
//!   nested (`"Foo::Bar.method"`).
//! - **`DebuggerRequires`** — replaces the default set of required
//!   libraries that trigger an offense.
//!
//! ## Known v1 limitation: option overrides not wired through `Cx`
//!
//! `debugger_methods` / `debugger_requires` are exported via
//! `#[derive(CopOptions)]` so the host validates `murphy.toml` entries,
//! but runtime reads still come from `Options::default()`.
//! `murphy-9cr.9` will route overrides through `Cx`; until then
//! `[cops.rules."Lint/Debugger"]` overrides have no effect at dispatch
//! time. This matches the v1 contract on every other cop with options.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct Debugger;

/// Cop options for [`Debugger`]. v1: read from `Default` at dispatch
/// time (`murphy-9cr.9` will wire live overrides through `Cx`).
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = [
            // bare entrypoints
            "debugger",
            "byebug",
            "remote_byebug",
            "pry",
            "save_and_open_page",
            "save_and_open_screenshot",
            "jard",
            // chained on `binding`
            "binding.irb",
            "binding.pry",
            "binding.remote_pry",
            "binding.pry_remote",
            "binding.b",
            "binding.break",
            "binding.console",
            // const-receiver
            "Kernel.debugger",
            "Kernel.binding",
            "Pry.rescue",
        ],
        description = "Method calls that should be flagged as debugger entry points."
    )]
    pub debugger_methods: Vec<String>,
    #[option(
        default = [
            "byebug",
            "capybara/dsl",
            "debug",
            "debug/open",
            "debug/open_nonstop",
            "pry",
            "pry-byebug",
        ],
        description = "Libraries whose `require` should be flagged as a debugger require."
    )]
    pub debugger_requires: Vec<String>,
}

#[cop(
    name = "Lint/Debugger",
    description = "Flag debugger calls and debugger requires.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl Debugger {
    // The method list covers every right-of-`.` name across the default
    // `debugger_methods` set plus `require` for the requires gate. Custom
    // `DebuggerMethods` entries that need an unlisted method symbol will
    // not dispatch until murphy-9cr.9 wires options into `Cx`; this is
    // documented as a v1 limitation alongside the option override one.
    #[on_node(
        kind = "send",
        methods = [
            "debugger",
            "byebug",
            "remote_byebug",
            "pry",
            "irb",
            "b",
            "break",
            "console",
            "remote_pry",
            "pry_remote",
            "binding",
            "rescue",
            "save_and_open_page",
            "save_and_open_screenshot",
            "jard",
            "require"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        let opts = Options::default();
        let method_str = cx.symbol_str(method);

        // `require '<lib>'` with a Str literal argument.
        if method_str == "require" && receiver.get().is_none() {
            if let Some(arg_id) = cx.list(args).first()
                && let NodeKind::Str(s) = *cx.kind(*arg_id)
            {
                let lib = cx.string_str(s);
                if opts.debugger_requires.iter().any(|e| e == lib) {
                    let src = cx.raw_source(cx.range(node));
                    cx.emit_offense(
                        cx.range(node),
                        &format!("Remove debugger entry point `{src}`."),
                        None,
                    );
                }
            }
            return;
        }

        // Build the call's canonical signature and look it up in the
        // configured `debugger_methods` list.
        let Some(sig) = call_signature(cx, receiver, method_str) else {
            return;
        };
        if opts.debugger_methods.iter().any(|e| e == &sig) {
            let src = cx.raw_source(cx.range(node));
            cx.emit_offense(
                cx.range(node),
                &format!("Remove debugger entry point `{src}`."),
                None,
            );
        }
    }
}

/// Canonical `<receiver>.<method>` signature, or just `<method>` for a
/// bare call. Returns `None` when the receiver shape isn't one the
/// configured `DebuggerMethods` syntax can spell — anonymous receivers
/// like `(some + expr).pry` are out of scope.
fn call_signature(cx: &Cx<'_>, receiver: OptNodeId, method: &str) -> Option<String> {
    let Some(recv_id) = receiver.get() else {
        return Some(method.to_string());
    };
    let recv = receiver_signature(cx, recv_id)?;
    Some(format!("{recv}.{method}"))
}

fn receiver_signature(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        // Bare method call returning self / something, e.g. `binding`.
        // Only honor it when the receiver itself is a no-receiver,
        // no-arg call (e.g. `binding`, `Kernel`'s lookup is *not* a Send).
        NodeKind::Send {
            receiver,
            method,
            args,
        } if receiver.get().is_none() && cx.list(args).is_empty() => {
            Some(cx.symbol_str(method).to_string())
        }
        NodeKind::Const { scope, name } => {
            let name_str = cx.symbol_str(name);
            match scope.get() {
                Some(s) => {
                    let outer = receiver_signature(cx, s)?;
                    Some(format!("{outer}::{name_str}"))
                }
                None => Some(name_str.to_string()),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::Debugger;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_debugger_calls_and_requires() {
        test::<Debugger>().expect_offense(indoc! {r#"
            pry
            ^^^ Remove debugger entry point `pry`.
            require 'pry'
            ^^^^^^^^^^^^^ Remove debugger entry point `require 'pry'`.
            binding.pry
            ^^^^^^^^^^^ Remove debugger entry point `binding.pry`.
            debugger
            ^^^^^^^^ Remove debugger entry point `debugger`.
            byebug
            ^^^^^^ Remove debugger entry point `byebug`.
            require 'debug/open'
            ^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `require 'debug/open'`.
        "#});
    }

    #[test]
    fn ignores_non_debugger_usage_and_multibyte_source() {
        test::<Debugger>().expect_no_offenses("名前 = 'pry'\nlogger.pry\nrequire name\n");
    }

    // murphy-dma2: binding.irb / Kernel.debugger / receiver chains.

    #[test]
    fn flags_binding_irb() {
        test::<Debugger>().expect_offense(indoc! {r#"
                binding.irb
                ^^^^^^^^^^^ Remove debugger entry point `binding.irb`.
            "#});
    }

    #[test]
    fn flags_binding_b_and_binding_break() {
        test::<Debugger>().expect_offense(indoc! {r#"
                binding.b
                ^^^^^^^^^ Remove debugger entry point `binding.b`.
                binding.break
                ^^^^^^^^^^^^^ Remove debugger entry point `binding.break`.
            "#});
    }

    #[test]
    fn flags_kernel_debugger_with_const_receiver() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.debugger
                ^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.debugger`.
            "#});
    }

    #[test]
    fn flags_pry_rescue() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Pry.rescue
                ^^^^^^^^^^ Remove debugger entry point `Pry.rescue`.
            "#});
    }

    #[test]
    fn flags_capybara_save_and_open_helpers() {
        test::<Debugger>().expect_offense(indoc! {r#"
                save_and_open_page
                ^^^^^^^^^^^^^^^^^^ Remove debugger entry point `save_and_open_page`.
                save_and_open_screenshot
                ^^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `save_and_open_screenshot`.
            "#});
    }

    #[test]
    fn flags_jard() {
        test::<Debugger>().expect_offense(indoc! {r#"
                jard
                ^^^^ Remove debugger entry point `jard`.
            "#});
    }

    #[test]
    fn flags_require_capybara_dsl() {
        test::<Debugger>().expect_offense(indoc! {r#"
                require 'capybara/dsl'
                ^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `require 'capybara/dsl'`.
            "#});
    }

    #[test]
    fn ignores_unrelated_receiver_with_same_method_name() {
        // `foo.b` is not `binding.b` — the receiver must literally be
        // the `binding` no-arg call.
        test::<Debugger>().expect_no_offenses("foo.b\nfoo.break\nfoo.irb\n");
    }
}
