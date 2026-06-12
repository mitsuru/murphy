//! `Lint/Debugger` — flag calls that drop into a debugger / REPL or
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Debugger
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Fixed: multi-level Send chains (Kernel.binding.*), missing default
//!   entries (Kernel.byebug, Kernel.remote_byebug, save_page,
//!   save_screenshot, page.save_*, Kernel.binding.* variants, debug/start).
//!   Cbase handling not needed: Murphy translates ::X to Const{scope:None}
//!   same as X, so ::Pry.rescue and ::Kernel.debugger already match.
//!   assumed_usage_context guard implemented (closed gap: murphy-rjwo).
//!   DebuggerMethods/DebuggerRequires are read live via `cx.options_or_default`.
//!   murphy-ch9j closed: (1) `DebuggerMethods` now accepts both a flat array
//!   and RuboCop's hash-of-arrays (`category => [method, ...]`) form via a
//!   hand-rolled `from_config_json` that flattens `values.flatten`; (2) the
//!   dispatch is a bare `on_node(kind="send")` (no static method filter),
//!   mirroring RuboCop's plain `on_send`, so a configured custom method whose
//!   selector is not in the default set is dispatched and flagged.
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
//!   `save_screenshot`, `jard`, ...
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
//! - **`DebuggerMethods`** -- replaces the default set of debugger
//!   entrypoints to match. Each entry is either a bare method name
//!   (`"debugger"`) or a `<receiver>.<method>` signature
//!   (`"binding.pry"`, `"Kernel.debugger"`). Constant receivers can be
//!   nested (`"Foo::Bar.method"`). Accepts both RuboCop config shapes: a
//!   flat array, or a hash-of-arrays grouped by gem category
//!   (`{ byebug: ["byebug"], pry: ["binding.pry"] }`), which is flattened
//!   via `values.flatten`.
//! - **`DebuggerRequires`** -- replaces the default set of required
//!   libraries that trigger an offense.
//!
//! Options are hand-rolled (see [`Options`]) and read live at dispatch time
//! via [`Cx::options_or_default`], so a configured
//! `[cops.rules."Lint/Debugger"]` override (e.g. a custom `DebuggerRequires`
//! entry, or a hash-of-arrays `DebuggerMethods`) takes effect.
//!
//! ## Dispatch
//!
//! The cop visits every `send` node (`#[on_node(kind = "send")]`), matching
//! RuboCop's plain `on_send`. There is no static method filter, so a custom
//! `DebuggerMethods` entry whose selector is outside the default set still
//! dispatches and is flagged.

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct Debugger;

/// Default `DebuggerMethods`, mirroring RuboCop's `config/default.yml`.
const DEFAULT_DEBUGGER_METHODS: &[&str] = &[
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
];

/// Default `DebuggerRequires`, mirroring RuboCop's `config/default.yml`.
const DEFAULT_DEBUGGER_REQUIRES: &[&str] = &[
    "byebug",
    "capybara/dsl",
    "debug",
    "debug/open",
    "debug/open_nonstop",
    "debug/start",
    "pry",
    "pry-byebug",
];

/// Cop options for [`Debugger`]. Read live at dispatch time via
/// [`Cx::options_or_default`].
///
/// `DebuggerMethods` is hand-rolled (not `#[derive(CopOptions)]`) so it can
/// accept RuboCop's two shapes: a flat array (`["debugger", ...]`) **or** a
/// hash-of-arrays grouped by gem category (`{ "byebug": ["byebug"], ... }`),
/// which is flattened via `values.flatten` exactly as RuboCop does.
#[derive(Clone, Debug)]
pub struct Options {
    pub debugger_methods: Vec<String>,
    pub debugger_requires: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            debugger_methods: DEFAULT_DEBUGGER_METHODS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            debugger_requires: DEFAULT_DEBUGGER_REQUIRES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

/// Decode a flat string array under `field`, surfacing path-qualified
/// `type_mismatch` errors for the array itself and for non-string elements.
fn decode_string_array(
    array: &[serde_json::Value],
    field: &str,
) -> Result<Vec<String>, ConfigError> {
    array
        .iter()
        .enumerate()
        .map(|(i, elem)| {
            elem.as_str()
                .map(str::to_string)
                .ok_or_else(|| ConfigError::type_mismatch(format!("{field}[{i}]"), "string"))
        })
        .collect()
}

impl CopOptions for Options {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        // Error surface mirrors `#[derive(CopOptions)]`: non-object root →
        // `not_an_object`; per-field shape mismatches → `type_mismatch` with a
        // path-qualified field name. Absent fields fall back to defaults.
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let mut opts = Self::default();

        if let Some(methods_value) = obj.get("DebuggerMethods") {
            opts.debugger_methods = if let Some(array) = methods_value.as_array() {
                // Flat array: `["debugger", "binding.pry"]`.
                decode_string_array(array, "DebuggerMethods")?
            } else if let Some(map) = methods_value.as_object() {
                // Hash-of-arrays: `{ category => [method, ...] }`, flattened
                // exactly like RuboCop's `config.values.flatten`.
                let mut flattened = Vec::new();
                for (key, group) in map {
                    let array = group.as_array().ok_or_else(|| {
                        ConfigError::type_mismatch(
                            format!("DebuggerMethods.{key}"),
                            "array of strings",
                        )
                    })?;
                    flattened.extend(decode_string_array(array, &format!("DebuggerMethods.{key}"))?);
                }
                flattened
            } else {
                return Err(ConfigError::type_mismatch(
                    "DebuggerMethods",
                    "array or object of arrays",
                ));
            };
        }

        if let Some(requires_value) = obj.get("DebuggerRequires") {
            let array = requires_value
                .as_array()
                .ok_or_else(|| ConfigError::type_mismatch("DebuggerRequires", "array of strings"))?;
            opts.debugger_requires = decode_string_array(array, "DebuggerRequires")?;
        }

        Ok(opts)
    }

    fn to_config_json(&self) -> String {
        let methods: Vec<serde_json::Value> = self
            .debugger_methods
            .iter()
            .map(|m| serde_json::Value::String(m.clone()))
            .collect();
        let requires: Vec<serde_json::Value> = self
            .debugger_requires
            .iter()
            .map(|r| serde_json::Value::String(r.clone()))
            .collect();
        let mut top = serde_json::Map::new();
        top.insert("DebuggerMethods".to_string(), serde_json::Value::Array(methods));
        top.insert(
            "DebuggerRequires".to_string(),
            serde_json::Value::Array(requires),
        );
        serde_json::Value::Object(top).to_string()
    }
}

#[cop(
    name = "Lint/Debugger",
    description = "Flag debugger calls and debugger requires.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl Debugger {
    // Visit EVERY send, mirroring RuboCop's plain `on_send`. A static method
    // filter would exclude custom `DebuggerMethods` entries whose selector is
    // not in the default set; running on all sends lets a configured custom
    // method (flat array or hash-of-arrays) dispatch through `check_send`
    // (murphy-ch9j).
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        let opts = cx.options_or_default::<Options>();
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

        // Mirror RuboCop's `assumed_usage_context?`: suppress when the
        // debugger call (no args, no receiver) is used as an argument to another
        // call and is not inside a block/proc/lambda. This avoids false positives
        // for patterns like `let(:p) { foo }; expect(do_something(p)).to eq bar`
        // where `p` is a variable, not the `Kernel#p` debugger entrypoint.
        //
        // The guard intentionally applies only to bare calls (no receiver).
        // Receiver-qualified calls like `binding.pry` or `Kernel.debugger`
        // are always unambiguous debugger entrypoints and must not be suppressed.
        if cx.list(args).is_empty() && receiver.get().is_none() && assumed_usage_context(cx, node) {
            return;
        }

        // Build the call's canonical signature and look it up in the
        // configured `debugger_methods` list.
        let Some(sig) = call_signature(cx, receiver, method_str) else {
            return;
        };
        if opts.debugger_methods.iter().any(|e| e == &sig) {
            // Suppress this match if the parent Send will produce a longer
            // match -- prevents double-flagging e.g. both `Kernel.binding`
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

/// Mirror RuboCop's `assumed_usage_context?`. Single-pass parent traversal.
fn assumed_usage_context(cx: &Cx<'_>, node: NodeId) -> bool {
    let is_assumed = is_assumed_argument(cx, node);
    let mut current = node;
    let mut has_send = false;
    let mut has_block = false;
    while let Some(parent_id) = cx.parent(current).get() {
        match *cx.kind(parent_id) {
            NodeKind::Send { .. } => {
                has_send = true;
            }
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Kwbegin(_)
            | NodeKind::Lambda => {
                has_block = true;
            }
            _ => {}
        }
        current = parent_id;
    }
    if !has_send {
        return false;
    }
    if is_assumed {
        return true;
    }
    !has_block
}

/// Returns `true` if the immediate parent of `node` is a call (Send),
/// a literal, or a Pair. Uses `cx.is_literal()` for full literal coverage,
/// matching RuboCop's `node.parent&.literal?` check.
fn is_assumed_argument(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent_id) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        *cx.kind(parent_id),
        NodeKind::Send { .. } | NodeKind::Pair { .. }
    ) || cx.is_literal(parent_id)
}

/// Canonical `<receiver>.<method>` signature, or just `<method>` for a
/// bare call. Returns `None` when the receiver shape is not something the
/// configured `DebuggerMethods` syntax can spell -- anonymous receivers
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
        // `irb`'s receiver is `binding` (Send, recv=Const(Kernel)) ->
        // recurse -> "Kernel.binding"; outer result -> "Kernel.binding.irb".
        NodeKind::Send {
            receiver,
            method,
            args,
        } if cx.list(args).is_empty() => {
            let method_name = cx.symbol_str(method);
            match receiver.get() {
                None => Some(method_name.to_string()),
                Some(recv_id) => {
                    let receiver_sig = receiver_signature(cx, recv_id)?;
                    Some(format!("{receiver_sig}.{method_name}"))
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
    opts.debugger_methods.iter().any(|e| {
        e.strip_prefix(current_sig)
            .and_then(|rest| rest.strip_prefix('.'))
            == Some(parent_method_str)
    })
}

#[cfg(test)]
mod tests {
    use super::{Debugger, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_custom_require_from_debugger_requires_option() {
        // A custom `DebuggerRequires` entry is read live via
        // `cx.options_or_default`, so `require 'my_custom_debug'` is flagged.
        test::<Debugger>()
            .with_options(&Options {
                debugger_requires: vec!["my_custom_debug".to_string()],
                ..Options::default()
            })
            .expect_offense(indoc! {r#"
                require 'my_custom_debug'
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove debugger entry point `require 'my_custom_debug'`.
            "#});
    }

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
        // `foo.b` is not `binding.b` -- the receiver must literally be
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

    // Regression: ::X constants are normalised to X at the AST level,
    // so ::Kernel and ::Pry already match the same entries as Kernel / Pry.

    #[test]
    fn flags_absolute_const_kernel_debugger() {
        test::<Debugger>().expect_offense(indoc! {r#"
                ::Kernel.debugger
                ^^^^^^^^^^^^^^^^^ Remove debugger entry point `::Kernel.debugger`.
            "#});
    }

    #[test]
    fn flags_absolute_const_pry_rescue() {
        test::<Debugger>().expect_offense(indoc! {r#"
                ::Pry.rescue
                ^^^^^^^^^^^^ Remove debugger entry point `::Pry.rescue`.
            "#});
    }

    // --- assumed_usage_context? guard (murphy-rjwo) ---

    /// Debugger call passed as a positional argument to another method should
    /// be suppressed -- it's likely a variable reference (e.g. `let(:p) { foo }`)
    /// used as an argument, not a deliberate debugger entrypoint.
    #[test]
    fn suppresses_debugger_as_method_argument() {
        test::<Debugger>().expect_no_offenses("puts(pry)\n");
    }

    /// Debugger call as a keyword argument value is also suppressed.
    #[test]
    fn suppresses_debugger_as_keyword_argument_value() {
        test::<Debugger>().expect_no_offenses("foo(k: pry)\n");
    }

    /// Standalone debugger calls are still flagged even with the guard.
    #[test]
    fn still_flags_standalone_debugger() {
        test::<Debugger>().expect_offense(indoc! {r#"
            pry
            ^^^ Remove debugger entry point `pry`.
        "#});
    }

    /// Debugger inside a block body that is itself passed as an argument to a
    /// call is still flagged -- the block boundary breaks the guard condition.
    #[test]
    fn still_flags_debugger_inside_block_passed_to_call() {
        test::<Debugger>().expect_offense(indoc! {r#"
            foo(bar { pry })
                      ^^^ Remove debugger entry point `pry`.
        "#});
    }

    /// Receiver-qualified debugger calls (binding.pry, Kernel.debugger) passed
    /// as arguments must NOT be suppressed by assumed_usage_context. These are
    /// always unambiguous debugger entrypoints regardless of context.
    #[test]
    fn flags_receiver_qualified_debugger_as_argument() {
        test::<Debugger>().expect_offense(indoc! {r#"
            puts(binding.pry)
                 ^^^^^^^^^^^ Remove debugger entry point `binding.pry`.
        "#});
    }

    #[test]
    fn flags_kernel_debugger_as_argument() {
        test::<Debugger>().expect_offense(indoc! {r#"
            foo(Kernel.debugger)
                ^^^^^^^^^^^^^^^ Remove debugger entry point `Kernel.debugger`.
        "#});
    }

    // --- murphy-ch9j: custom-method dispatch + hash-of-arrays config ---

    #[test]
    fn flags_custom_bare_debugger_method_from_flat_array() {
        // A custom `DebuggerMethods` entry whose selector is not in any static
        // dispatch list must still be flagged — the cop now visits every send.
        test::<Debugger>()
            .with_options(&Options {
                debugger_methods: vec!["my_custom_debugger".to_string()],
                debugger_requires: vec![],
            })
            .expect_offense(indoc! {r#"
                my_custom_debugger
                ^^^^^^^^^^^^^^^^^^ Remove debugger entry point `my_custom_debugger`.
            "#});
    }

    #[test]
    fn flags_custom_chained_debugger_method() {
        test::<Debugger>()
            .with_options(&Options {
                debugger_methods: vec!["my_object.my_debug".to_string()],
                debugger_requires: vec![],
            })
            .expect_offense(indoc! {r#"
                my_object.my_debug
                ^^^^^^^^^^^^^^^^^^ Remove debugger entry point `my_object.my_debug`.
            "#});
    }

    #[test]
    fn flags_custom_method_from_hash_of_arrays_config() {
        // RuboCop accepts `DebuggerMethods` as a hash-of-arrays
        // (`category => [method, ...]`) and flattens `values.flatten`.
        let opts = <Options as murphy_plugin_api::CopOptions>::from_config_json(
            br#"{"DebuggerMethods": {"my_gem": ["custom_debug", "another_break"]}}"#,
        )
        .expect("hash-of-arrays DebuggerMethods is valid");
        test::<Debugger>().with_options(&opts).expect_offense(indoc! {r#"
                custom_debug
                ^^^^^^^^^^^^ Remove debugger entry point `custom_debug`.
                another_break
                ^^^^^^^^^^^^^ Remove debugger entry point `another_break`.
            "#});
    }

    #[test]
    fn accepts_flat_array_debugger_methods_config() {
        let opts = <Options as murphy_plugin_api::CopOptions>::from_config_json(
            br#"{"DebuggerMethods": ["custom_debug"]}"#,
        )
        .expect("flat-array DebuggerMethods is valid");
        assert_eq!(opts.debugger_methods, vec!["custom_debug".to_string()]);
    }

    #[test]
    fn debugger_methods_wrong_shape_errors() {
        // A scalar (not array/object) is a shape error, not a silent default.
        let err = <Options as murphy_plugin_api::CopOptions>::from_config_json(
            br#"{"DebuggerMethods": "oops"}"#,
        )
        .expect_err("scalar DebuggerMethods is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "DebuggerMethods");
        assert_eq!(*expected, "array or object of arrays");
    }

    #[test]
    fn debugger_methods_element_wrong_shape_errors() {
        let err = <Options as murphy_plugin_api::CopOptions>::from_config_json(
            br#"{"DebuggerMethods": {"g": [1]}}"#,
        )
        .expect_err("non-string element is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "DebuggerMethods.g[0]");
        assert_eq!(*expected, "string");
    }

    #[test]
    fn debugger_options_roundtrip_via_to_config_json() {
        let opts = Options {
            debugger_methods: vec!["custom_debug".to_string()],
            debugger_requires: vec!["my_lib".to_string()],
        };
        let json = <Options as murphy_plugin_api::CopOptions>::to_config_json(&opts);
        let decoded = <Options as murphy_plugin_api::CopOptions>::from_config_json(json.as_bytes())
            .expect("roundtrip");
        assert_eq!(decoded.debugger_methods, opts.debugger_methods);
        assert_eq!(decoded.debugger_requires, opts.debugger_requires);
    }
}
murphy_plugin_api::submit_cop!(Debugger);
