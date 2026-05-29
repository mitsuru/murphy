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
//!   Fixed: multi-level Send chains (Kernel.binding.*), missing default
//!   entries (Kernel.byebug, Kernel.remote_byebug, save_page,
//!   save_screenshot, page.save_*, Kernel.binding.* variants, debug/start).
//!   Cbase handling not needed: Murphy translates ::X to Const{scope:None}
//!   same as X, so ::Pry.rescue and ::Kernel.debugger already match.
//!   Remaining gap: assumed_usage_context guard (deferred to murphy-rjwo).
//! ```
//!
//! `require`s that load one. Defaults mirror RuboCop's `DebuggerMethods`
//! and `DebuggerRequires` so a `.rubocop.yml`-aware Ruby developer
//! sees the same set of offenses.
//!
//! ## Matched shapes
//!
//! - **Bare** debugger entrypoints: `debugger`, `byebug`, `remote_byebug`,
//!   `pry`, `save_and_open_page`, `save_and_open_screenshot`, `save_page`,
//!   `save_screenshot`, `jard`, …
//! - **Chained** entrypoints: `binding.irb`, `binding.pry`,
//!   `binding.remote_pry`, `binding.pry_remote`, `binding.b`,
//!   `binding.break`, `binding.console`, and the `Kernel.binding.*` forms.
//! - **Const-receiver** entrypoints: `Kernel.debugger`, `Kernel.binding`,
//!   `Kernel.byebug`, `Kernel.remote_byebug`, `Pry.rescue` (and any
//!   nested `Foo::Bar.method` path). `::Pry` and `::Kernel` are treated
//!   identically to `Pry` and `Kernel` (Murphy normalises them at the
//!   AST level).
//! - **Debugger requires**: `require 'debug'`, `require 'debug/open'`,
//!   `require 'debug/open_nonstop'`, `require 'debug/start'`,
//!   `require 'byebug'`, `require 'pry'`, `require 'pry-byebug'`,
//!   `require 'capybara/dsl'`.
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
            "save_page",
            "save_screenshot",
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
            "Kernel.byebug",
            "Kernel.remote_byebug",
            "Pry.rescue",
            // page.* Capybara helpers
            "page.save_and_open_page",
            "page.save_and_open_screenshot",
            "page.save_page",
            "page.save_screenshot",
            // Kernel.binding.* three-level chains
            "Kernel.binding.b",
            "Kernel.binding.break",
            "Kernel.binding.pry",
            "Kernel.binding.remote_pry",
            "Kernel.binding.pry_remote",
            "Kernel.binding.irb",
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
            "debug/start",
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
            "save_page",
            "save_screenshot",
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
            // Suppress this match if the parent Send will produce a longer
            // match — prevents double-flagging e.g. both `Kernel.binding`
            // and `Kernel.binding.irb` when the latter is what is written.
            if parent_will_match(cx, node, &sig, &opts) {
                return;
            }
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
/// bare call. Returns `None` when the receiver shape is not something the
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
        // No-arg Send call, e.g. `binding` or `page`. Recurse into the
        // receiver so multi-level chains like `Kernel.binding.irb` work:
        // `irb`'s receiver is `binding` (Send, recv=Const(Kernel)) →
        // recurse → "Kernel.binding"; outer result → "Kernel.binding.irb".
        NodeKind::Send {
            receiver,
            method,
            args,
        } if cx.list(args).is_empty() => {
            let method_name = cx.symbol_str(method).to_string();
            match receiver.get() {
                None => Some(method_name),
                Some(recv_id) => {
                    let outer = receiver_signature(cx, recv_id)?;
                    Some(format!("{outer}.{method_name}"))
                }
            }
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

/// Returns `true` when the immediate parent of `node` is a no-arg Send
/// whose full signature would also match a (longer) entry in
/// `opts.debugger_methods`. Used to suppress the shorter match when
/// both `Kernel.binding` and `Kernel.binding.irb` are in the list and
/// the source says `Kernel.binding.irb`.
fn parent_will_match(cx: &Cx<'_>, node: NodeId, current_sig: &str, opts: &Options) -> bool {
    let Some(parent_id) = cx.parent(node).get() else {
        return false;
    };
    let NodeKind::Send {
        receiver: parent_recv,
        method: parent_method,
        args: parent_args,
    } = *cx.kind(parent_id)
    else {
        return false;
    };
    // Only consider the parent if `node` is the receiver of `parent_id`.
    let Some(actual_recv) = parent_recv.get() else {
        return false;
    };
    if actual_recv != node {
        return false;
    }
    // The parent must be a no-arg call (same constraint as receiver_signature).
    if !cx.list(parent_args).is_empty() {
        return false;
    }
    let parent_method_str = cx.symbol_str(parent_method);
    let longer_sig = format!("{current_sig}.{parent_method_str}");
    opts.debugger_methods.iter().any(|e| e == &longer_sig)
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

    // --- parity gap tests ---

    #[test]
    fn flags_kernel_byebug() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.byebug
                ^^^^^^^^^^^^^ Remove debugger entry point `Kernel.byebug`.
            "#});
    }

    #[test]
    fn flags_kernel_remote_byebug() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.remote_byebug
                ^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.remote_byebug`.
            "#});
    }

    #[test]
    fn flags_bare_save_page_and_save_screenshot() {
        test::<Debugger>().expect_offense(indoc! {r#"
                save_page
                ^^^^^^^^^ Remove debugger entry point `save_page`.
                save_screenshot
                ^^^^^^^^^^^^^^^ Remove debugger entry point `save_screenshot`.
            "#});
    }

    #[test]
    fn flags_page_dot_save_helpers() {
        test::<Debugger>().expect_offense(indoc! {r#"
                page.save_and_open_page
                ^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `page.save_and_open_page`.
                page.save_and_open_screenshot
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `page.save_and_open_screenshot`.
                page.save_page
                ^^^^^^^^^^^^^^ Remove debugger entry point `page.save_page`.
                page.save_screenshot
                ^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `page.save_screenshot`.
            "#});
    }

    #[test]
    fn flags_kernel_binding_chain_b_and_break() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.binding.b
                ^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.b`.
                Kernel.binding.break
                ^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.break`.
            "#});
    }

    #[test]
    fn flags_kernel_binding_pry_variants() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.binding.pry
                ^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.pry`.
                Kernel.binding.remote_pry
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.remote_pry`.
                Kernel.binding.pry_remote
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.pry_remote`.
            "#});
    }

    #[test]
    fn flags_kernel_binding_irb() {
        test::<Debugger>().expect_offense(indoc! {r#"
                Kernel.binding.irb
                ^^^^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.binding.irb`.
            "#});
    }

    #[test]
    fn flags_require_debug_start() {
        test::<Debugger>().expect_offense(indoc! {r#"
                require 'debug/start'
                ^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `require 'debug/start'`.
            "#});
    }
}
