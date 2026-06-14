//! `Style/ArgumentsForwarding` — use shorthand `...` / anonymous `*`/`**`/`&`
//! forwarding syntax.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArgumentsForwarding
//! upstream_version_checked: 1.87.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's version-gated decision tree (murphy-484s):
//!
//!   - Forward-all `...` (Ruby 2.7+): a def whose restarg/kwrestarg/blockarg
//!     all have redundant names and are forwarded unchanged to call sites is
//!     replaced with `...`.
//!   - Anonymous block `&` (Ruby 3.1+): a def forwarding only a redundant
//!     `&block` becomes `&` at both def and call.
//!   - Anonymous `*`/`**`/`&` (Ruby 3.2+, gated on `UseAnonymousForwarding`):
//!     partial-forward shapes (`*args` without `**kwargs`, kwrest-only, etc.)
//!     are anonymized per-argument instead of collapsed to `...`.
//!
//!   The per-dispatch Ruby target arrives via the `Cx::target_ruby_version()`
//!   ABI surface added in murphy-484s; murphy's default floor is Ruby 3.1, so
//!   the 3.2+ anonymous `*`/`**` offenses only fire once a project's
//!   `TargetRubyVersion` (or, later, detected `.ruby-version`) is ≥ 3.2.
//!
//!   The Ruby 3.3.0 anonymous-forwarding-in-block syntax-error workaround is
//!   ported (`allow_anonymous_forwarding_in_block?` /
//!   `all_forwarding_offenses_correctable?`): below target 3.4 a forwarding
//!   send nested inside a block is not anonymized.
//!
//!   Documented gap: RuboCop's `explicit_block_name?` consults
//!   `Naming/BlockForwarding`'s `EnforcedStyle`; murphy cannot read another
//!   cop's config here and treats it as the default (`anonymous`), so block
//!   `&` is always offered. This matches RuboCop under default config.
//! ```
//!
//! ## Autocorrect
//!
//! `...` replaces the forwardable span at def and call; anonymous offenses
//! replace the individual `*args`/`**kwargs`/`&block` node with `*`/`**`/`&`.
//! Parentheses are added (consuming the gap after the method name) when the
//! def/call lacks them.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, RubyVersion, SourceTokenKind, cop};

const FORWARDING_MSG: &str = "Use shorthand syntax `...` for arguments forwarding.";
const ARGS_MSG: &str = "Use anonymous positional arguments forwarding (`*`).";
const KWARGS_MSG: &str = "Use anonymous keyword arguments forwarding (`**`).";
const BLOCK_MSG: &str = "Use anonymous block arguments forwarding (`&`).";

/// murphy's default Ruby target floor (mirrors `config.rs`
/// `default_target_ruby_version`). Used when the host did not thread a concrete
/// version (raw-ABI test harnesses); production always supplies a value.
const DEFAULT_TARGET_RUBY: RubyVersion = RubyVersion::new(3, 1);
const RUBY_3_0: RubyVersion = RubyVersion::new(3, 0);
const RUBY_3_2: RubyVersion = RubyVersion::new(3, 2);
const RUBY_3_4: RubyVersion = RubyVersion::new(3, 4);

/// Cop options for [`ArgumentsForwarding`]. Read live at dispatch time via
/// [`Cx::options_or_default`]. Mirrors RuboCop's config keys and defaults.
#[derive(CopOptions)]
pub struct ArgumentsForwardingOptions {
    /// `AllowOnlyRestArgument` — when `true` (default), a def that forwards
    /// only a rest and/or kwrest argument (no `&block`) is NOT flagged for
    /// forward-all, because `...` would also forward a block and change
    /// behaviour. Mirrors RuboCop's `allow_offense_for_no_block?`. Only
    /// relevant for Ruby < 3.2 (at 3.2+ such shapes get anonymous `*`/`**`).
    #[option(
        name = "AllowOnlyRestArgument",
        default = true,
        description = "Allow forwarding only a rest/kwrest argument (no block) without flagging."
    )]
    pub allow_only_rest_argument: bool,

    /// `UseAnonymousForwarding` — when `true` (default), partial-forward shapes
    /// are anonymized to `*`/`**`/`&` on Ruby 3.2+. When `false`, only the
    /// version-independent forward-all `...` (and 3.1+ block `&`) paths fire.
    #[option(
        name = "UseAnonymousForwarding",
        default = true,
        description = "Use anonymous `*`/`**`/`&` forwarding on Ruby 3.2+."
    )]
    pub use_anonymous_forwarding: bool,

    /// `RedundantRestArgumentNames` — rest-arg names treated as anonymous-equivalent
    /// (`*args`, `*arguments`), so they may be replaced with `...`/`*`.
    #[option(
        name = "RedundantRestArgumentNames",
        default = ["args", "arguments"],
        description = "Rest-argument names considered redundant (forwardable)."
    )]
    pub redundant_rest_argument_names: Vec<String>,

    /// `RedundantKeywordRestArgumentNames` — kwrest-arg names treated as
    /// anonymous-equivalent (`**kwargs`, `**options`, `**opts`).
    #[option(
        name = "RedundantKeywordRestArgumentNames",
        default = ["kwargs", "options", "opts"],
        description = "Keyword-rest-argument names considered redundant (forwardable)."
    )]
    pub redundant_keyword_rest_argument_names: Vec<String>,

    /// `RedundantBlockArgumentNames` — block-arg names treated as
    /// anonymous-equivalent (`&blk`, `&block`, `&proc`).
    #[option(
        name = "RedundantBlockArgumentNames",
        default = ["blk", "block", "proc"],
        description = "Block-argument names considered redundant (forwardable)."
    )]
    pub redundant_block_argument_names: Vec<String>,
}

#[derive(Default)]
pub struct ArgumentsForwarding;

#[cop(
    name = "Style/ArgumentsForwarding",
    description = "Use arguments forwarding.",
    default_severity = "warning",
    default_enabled = true,
    options = ArgumentsForwardingOptions,
)]
impl ArgumentsForwarding {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_node(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_node(node, cx);
    }
}

/// RuboCop's send classification (`SendNodeClassifier#classification`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForwardClass {
    /// `:all` — every forwardable arg forwarded; can collapse to `...`.
    All,
    /// `:all_anonymous` — def and send are already fully anonymous (`*, **, &`).
    AllAnonymous,
    /// `:rest_or_kwrest` — only partially forwardable; anonymize per-arg (3.2+).
    RestOrKwrest,
}

/// Redundant-named forwardable args extracted from the def (RuboCop's
/// `forwardable_args`: `[restarg, kwrestarg, blockarg]`, each `Some` only when
/// present AND its name is redundant).
struct ForwardableArgs {
    rest: Option<NodeId>,
    kwrest: Option<NodeId>,
    block: Option<NodeId>,
}

/// One classified call/super/yield site (`[send_node, classification,
/// forward_rest, forward_kwrest, forward_block]`).
struct SendClass {
    send: NodeId,
    class: ForwardClass,
    /// The `(splat (lvar rest))` node in the call, when the rest is forwarded.
    forward_rest: Option<NodeId>,
    /// The `(kwsplat (lvar kwrest))` node inside the call's hash, when forwarded.
    forward_kwrest: Option<NodeId>,
    /// The `(block_pass …)` node in the call, when the block is forwarded.
    forward_block: Option<NodeId>,
}

fn check_def_node(node: NodeId, cx: &Cx<'_>) {
    // `def foo(...)` itself has a ForwardArgs, not restarg/kwrestarg — nothing
    // to do, and this guards against re-flagging.
    if cx.is_argument_forwarding(node) {
        return;
    }

    let Some(body_id) = cx.def_body(node).get() else {
        return;
    };
    let Some(args_id) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(args_list) = *cx.kind(args_id) else {
        return;
    };
    let def_args = cx.list(args_list);
    if def_args.is_empty() {
        return;
    }

    let opts = cx.options_or_default::<ArgumentsForwardingOptions>();
    let target = cx.target_ruby_version().unwrap_or(DEFAULT_TARGET_RUBY);

    let restarg = find_arg_kind(def_args, cx, |k| matches!(k, NodeKind::Restarg(_)));
    let kwrestarg = find_arg_kind(def_args, cx, |k| matches!(k, NodeKind::Kwrestarg(_)));
    let blockarg = find_arg_kind(def_args, cx, |k| matches!(k, NodeKind::Blockarg(_)));

    let fa = ForwardableArgs {
        rest: forwardable_restarg(restarg, &opts.redundant_rest_argument_names, cx),
        kwrest: forwardable_kwrestarg(kwrestarg, &opts.redundant_keyword_rest_argument_names, cx),
        block: forwardable_blockarg(blockarg, &opts.redundant_block_argument_names, cx),
    };

    let referenced = collect_referenced_lvars(body_id, cx);

    let classifications: Vec<SendClass> = collect_call_nodes(body_id, cx)
        .into_iter()
        .filter_map(|send| {
            classify_send(
                node,
                send,
                &referenced,
                &fa,
                def_args,
                target,
                opts.allow_only_rest_argument,
                cx,
            )
        })
        .collect();

    if classifications.is_empty() {
        return;
    }

    let mut fixer = Fixer::new(cx);
    if classifications
        .iter()
        .all(|c| matches!(c.class, ForwardClass::All | ForwardClass::AllAnonymous))
    {
        fixer.add_forward_all_offenses(node, &classifications, &fa, target);
    } else if target >= RUBY_3_2 {
        fixer.add_post_ruby_32_offenses(node, &classifications, &fa, &opts, target);
    }
}

// ── Classification (RuboCop's SendNodeClassifier) ───────────────────────────

#[allow(clippy::too_many_arguments)]
fn classify_send(
    def: NodeId,
    send: NodeId,
    referenced: &[&str],
    fa: &ForwardableArgs,
    def_args: &[NodeId],
    target: RubyVersion,
    allow_only_rest: bool,
    cx: &Cx<'_>,
) -> Option<SendClass> {
    let rest_name = fa.rest.and_then(|id| restarg_name(id, cx));
    let kwrest_name = fa.kwrest.and_then(|id| kwrestarg_name(id, cx));
    let block_name = fa.block.and_then(|id| blockarg_name(id, cx));

    let call_args = get_call_args(send, cx);

    let forward_rest = match rest_name {
        Some(rn) if !referenced.contains(&rn) => call_args
            .iter()
            .copied()
            .find(|&a| is_forwarded_rest(a, rn, cx)),
        _ => None,
    };
    let forward_kwrest = match kwrest_name {
        Some(kn) if !referenced.contains(&kn) => {
            call_args.iter().copied().find_map(|a| forwarded_kwrest(a, kn, cx))
        }
        _ => None,
    };
    let forward_block = match block_name {
        Some(bn) if !referenced.contains(&bn) => call_args
            .iter()
            .copied()
            .find(|&a| is_forwarded_block(a, bn, cx)),
        _ => None,
    };

    if forward_rest.is_none() && forward_kwrest.is_none() && forward_block.is_none() {
        return None;
    }

    let class = if ruby_32_only_anonymous(def, send, def_args, &call_args, target, cx) {
        ForwardClass::AllAnonymous
    } else if can_forward_all(
        fa,
        forward_rest,
        forward_kwrest,
        forward_block,
        rest_name,
        kwrest_name,
        block_name,
        referenced,
        def_args,
        &call_args,
        target,
        allow_only_rest,
        cx,
    ) {
        ForwardClass::All
    } else {
        ForwardClass::RestOrKwrest
    };

    Some(SendClass {
        send,
        class,
        forward_rest,
        forward_kwrest,
        forward_block,
    })
}

#[allow(clippy::too_many_arguments)]
fn can_forward_all(
    fa: &ForwardableArgs,
    forward_rest: Option<NodeId>,
    forward_kwrest: Option<NodeId>,
    forward_block: Option<NodeId>,
    rest_name: Option<&str>,
    kwrest_name: Option<&str>,
    block_name: Option<&str>,
    referenced: &[&str],
    def_args: &[NodeId],
    call_args: &[NodeId],
    target: RubyVersion,
    allow_only_rest: bool,
    cx: &Cx<'_>,
) -> bool {
    // any_arg_referenced?
    if [rest_name, kwrest_name, block_name]
        .into_iter()
        .flatten()
        .any(|n| referenced.contains(&n))
    {
        return false;
    }
    // ruby_30_or_lower_optarg?
    if target <= RUBY_3_0 && def_args.iter().any(|&a| matches!(*cx.kind(a), NodeKind::Optarg { .. }))
    {
        return false;
    }
    // ruby_32_or_higher_missing_rest_or_kwest?
    if target >= RUBY_3_2 && !(forward_rest.is_some() && forward_kwrest.is_some()) {
        return false;
    }
    // offensive_block_forwarding?
    let offensive_block = if fa.block.is_some() {
        forward_block.is_some()
    } else {
        !allow_only_rest
    };
    if !offensive_block {
        return false;
    }
    // additional_kwargs_or_forwarded_kwargs?
    let additional_kwargs = def_args
        .iter()
        .any(|&a| matches!(*cx.kind(a), NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. }));
    let forward_additional_kwargs = forward_kwrest.is_some_and(|kw| hash_child_count(kw, cx) != 1);
    if additional_kwargs || forward_additional_kwargs {
        return false;
    }
    // no_additional_args? || (target >= 3.0 && no_post_splat_args?)
    no_additional_args(fa, forward_rest, forward_kwrest, rest_name, kwrest_name, def_args, call_args)
        || (target >= RUBY_3_0 && no_post_splat_args(forward_rest, call_args, cx))
}

#[allow(clippy::too_many_arguments)]
fn no_additional_args(
    fa: &ForwardableArgs,
    forward_rest: Option<NodeId>,
    forward_kwrest: Option<NodeId>,
    rest_name: Option<&str>,
    kwrest_name: Option<&str>,
    def_args: &[NodeId],
    call_args: &[NodeId],
) -> bool {
    let forwardable_count = [fa.rest, fa.kwrest, fa.block]
        .into_iter()
        .flatten()
        .count();
    // missing_rest_arg_or_kwrest_arg?
    let missing = (rest_name.is_some() && forward_rest.is_none())
        || (kwrest_name.is_some() && forward_kwrest.is_none());
    if missing {
        return false;
    }
    def_args.len() == forwardable_count && call_args.len() == forwardable_count
}

fn no_post_splat_args(forward_rest: Option<NodeId>, call_args: &[NodeId], cx: &Cx<'_>) -> bool {
    let Some(splat) = forward_rest else {
        return true;
    };
    let Some(idx) = call_args.iter().position(|&a| a == splat) else {
        return true;
    };
    match call_args.get(idx + 1) {
        None => true,
        Some(&after) => matches!(*cx.kind(after), NodeKind::Hash(_) | NodeKind::BlockPass(_)),
    }
}

fn ruby_32_only_anonymous(
    _def: NodeId,
    send: NodeId,
    def_args: &[NodeId],
    call_args: &[NodeId],
    _target: RubyVersion,
    cx: &Cx<'_>,
) -> bool {
    // A block arg and an anonymous block arg are never passed together.
    if send_in_any_block(send, cx) {
        return false;
    }
    def_all_anonymous_args(def_args, cx) && send_all_anonymous_args(call_args, cx)
}

/// `(args ... (restarg) (kwrestarg) (blockarg nil?))` — the def ends with bare
/// anonymous `*`, `**`, `&`.
fn def_all_anonymous_args(def_args: &[NodeId], cx: &Cx<'_>) -> bool {
    let [.., r, k, b] = def_args else {
        return false;
    };
    matches!(*cx.kind(*r), NodeKind::Restarg(s) if cx.symbol_str(s).is_empty())
        && matches!(*cx.kind(*k), NodeKind::Kwrestarg(s) if cx.symbol_str(s).is_empty())
        && matches!(*cx.kind(*b), NodeKind::Blockarg(s) if cx.symbol_str(s).is_empty())
}

/// `... (forwarded_restarg) (hash (forwarded_kwrestarg)) (block_pass nil?)` —
/// the send ends with bare `*`, `**` (inside a single-element hash), `&`.
fn send_all_anonymous_args(call_args: &[NodeId], cx: &Cx<'_>) -> bool {
    let [.., r, k, b] = call_args else {
        return false;
    };
    let rest_ok = matches!(*cx.kind(*r), NodeKind::Splat(inner) if inner.get().is_none());
    let kwrest_ok = match *cx.kind(*k) {
        NodeKind::Hash(list) => matches!(
            cx.list(list),
            [only] if matches!(*cx.kind(*only), NodeKind::Kwsplat(inner) if inner.get().is_none())
        ),
        _ => false,
    };
    let block_ok = matches!(*cx.kind(*b), NodeKind::BlockPass(inner) if inner.get().is_none());
    rest_ok && kwrest_ok && block_ok
}

// ── Offense emission (RuboCop's add_*_offenses) ─────────────────────────────

/// Holds the dispatch `Cx` and tracks which nodes have already had parentheses
/// inserted, so the same def/call gets one `(` … `)` pair across multiple
/// per-argument anonymous offenses.
struct Fixer<'a, 'cx> {
    cx: &'a Cx<'cx>,
    parens_added: Vec<NodeId>,
}

impl<'a, 'cx> Fixer<'a, 'cx> {
    fn new(cx: &'a Cx<'cx>) -> Self {
        Self {
            cx,
            parens_added: Vec::new(),
        }
    }

    fn add_forward_all_offenses(
        &mut self,
        def: NodeId,
        classifications: &[SendClass],
        fa: &ForwardableArgs,
        target: RubyVersion,
    ) {
        let mut registered_block = false;
        for c in classifications {
            if c.forward_rest.is_none()
                && c.forward_kwrest.is_none()
                && c.class != ForwardClass::AllAnonymous
            {
                // Forwards only a block: anonymize `&` instead of `...`.
                if allow_anon_in_block(c.forward_block, c.send, target, self.cx) {
                    self.register_block_offense(true, def, fa.block, target);
                    self.register_block_offense(true, c.send, c.forward_block, target);
                }
                registered_block = true;
                break;
            }
            let first = c
                .forward_rest
                .or(c.forward_kwrest)
                .or_else(|| forward_all_first_argument(c.send, self.cx));
            if let Some(first) = first {
                self.register_forward_all(c.send, c.send, first);
            }
        }

        if registered_block {
            return;
        }

        if let Some(first) = fa.rest.or(fa.kwrest) {
            self.register_forward_all(def, def, first);
        }
    }

    fn add_post_ruby_32_offenses(
        &mut self,
        def: NodeId,
        classifications: &[SendClass],
        fa: &ForwardableArgs,
        opts: &ArgumentsForwardingOptions,
        target: RubyVersion,
    ) {
        if !opts.use_anonymous_forwarding {
            return;
        }
        if !all_forwarding_offenses_correctable(classifications, target, self.cx) {
            return;
        }

        let (mut def_rest, mut def_kwrest, mut def_block) = (false, false, false);
        for c in classifications {
            if allow_anon_in_block(c.forward_rest, c.send, target, self.cx) {
                if !def_rest {
                    self.register_args_offense(def, fa.rest);
                    def_rest = true;
                }
                self.register_args_offense(c.send, c.forward_rest);
            }
            if allow_anon_in_block(c.forward_kwrest, c.send, target, self.cx) {
                let add_parens = c.forward_rest.is_none();
                if !def_kwrest {
                    self.register_kwargs_offense(add_parens, def, fa.kwrest);
                    def_kwrest = true;
                }
                self.register_kwargs_offense(add_parens, c.send, c.forward_kwrest);
            }
            if allow_anon_in_block(c.forward_block, c.send, target, self.cx) {
                let add_parens = c.forward_rest.is_none();
                if !def_block {
                    self.register_block_offense(add_parens, def, fa.block, target);
                    def_block = true;
                }
                self.register_block_offense(add_parens, c.send, c.forward_block, target);
            }
        }
    }

    /// `register_forward_all_offense`: offense over `first .. last-arg-of(node)`,
    /// replaced with `...` (adding parens to `paren_node` if missing).
    fn register_forward_all(&mut self, node: NodeId, paren_node: NodeId, first: NodeId) {
        let Some(end) = last_argument_end(node, self.cx) else {
            return;
        };
        let range = Range {
            start: self.cx.range(first).start,
            end,
        };
        self.cx.emit_offense(range, FORWARDING_MSG, None);
        if node_has_arg_parens(paren_node, self.cx) {
            self.cx.emit_edit(range, "...");
        } else if let Some(callee_end) = node_callee_end(paren_node, self.cx) {
            self.cx
                .emit_edit(Range { start: callee_end, end: range.end }, "(...)");
        }
    }

    fn register_args_offense(&mut self, paren_node: NodeId, arg: Option<NodeId>) {
        let Some(arg) = arg else { return };
        self.cx.emit_offense(self.cx.range(arg), ARGS_MSG, None);
        self.add_parens_if_missing(paren_node);
        self.cx.emit_edit(self.cx.range(arg), "*");
    }

    fn register_kwargs_offense(&mut self, add_parens: bool, paren_node: NodeId, arg: Option<NodeId>) {
        let Some(arg) = arg else { return };
        self.cx.emit_offense(self.cx.range(arg), KWARGS_MSG, None);
        if add_parens {
            self.add_parens_if_missing(paren_node);
        }
        self.cx.emit_edit(self.cx.range(arg), "**");
    }

    /// `register_forward_block_arg_offense`: skip below 3.1, when absent, when
    /// already `&`, or under `Naming/BlockForwarding: explicit` (a documented
    /// gap — treated as default `anonymous`).
    fn register_block_offense(
        &mut self,
        add_parens: bool,
        paren_node: NodeId,
        arg: Option<NodeId>,
        target: RubyVersion,
    ) {
        let Some(arg) = arg else { return };
        if target <= RUBY_3_0 {
            return;
        }
        if self.cx.raw_source(self.cx.range(arg)) == "&" {
            return;
        }
        self.cx.emit_offense(self.cx.range(arg), BLOCK_MSG, None);
        if add_parens {
            self.add_parens_if_missing(paren_node);
        }
        self.cx.emit_edit(self.cx.range(arg), "&");
    }

    /// `add_parens_if_missing`: insert `(` (consuming the gap after the method
    /// name) and `)` after the last argument, once per node. Skips `foo[...]`.
    fn add_parens_if_missing(&mut self, node: NodeId) {
        if self.parens_added.contains(&node) {
            return;
        }
        if node_has_arg_parens(node, self.cx) {
            return;
        }
        if self.cx.method_name(node) == Some("[]") {
            return;
        }
        let (Some(callee_end), Some(last_end)) =
            (node_callee_end(node, self.cx), last_argument_end(node, self.cx))
        else {
            return;
        };
        self.parens_added.push(node);
        // Replace the whitespace between the method name and the first argument
        // with `(`, matching RuboCop's `def foo *a` -> `def foo(*a)`.
        let first_start = first_argument_start(node, self.cx).unwrap_or(callee_end);
        self.cx
            .emit_edit(Range { start: callee_end, end: first_start }, "(");
        self.cx.emit_edit(Range { start: last_end, end: last_end }, ")");
    }
}

fn all_forwarding_offenses_correctable(
    classifications: &[SendClass],
    target: RubyVersion,
    cx: &Cx<'_>,
) -> bool {
    if target >= RUBY_3_4 {
        return true;
    }
    !classifications
        .iter()
        .any(|c| send_in_any_block(c.send, cx))
}

/// `allow_anonymous_forwarding_in_block?`: anonymous forwarding of `node` is
/// allowed unless the send is inside a block on a target below 3.4 (Ruby 3.3.0
/// syntax-error workaround).
fn allow_anon_in_block(
    node: Option<NodeId>,
    send: NodeId,
    target: RubyVersion,
    cx: &Cx<'_>,
) -> bool {
    if node.is_none() {
        return false;
    }
    if target >= RUBY_3_4 {
        return true;
    }
    !send_in_any_block(send, cx)
}

fn send_in_any_block(send: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(send).any(|a| {
        matches!(
            *cx.kind(a),
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
        )
    })
}

/// `forward_all_first_argument`: the last bare `*` (forwarded_restarg) in the call.
fn forward_all_first_argument(send: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    get_call_args(send, cx)
        .into_iter()
        .rev()
        .find(|&a| matches!(*cx.kind(a), NodeKind::Splat(inner) if inner.get().is_none()))
}

// ── Range / parens helpers ──────────────────────────────────────────────────

fn last_argument_end(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    arg_list_of(node, cx)
        .last()
        .map(|&last| cx.range(last).end)
}

fn first_argument_start(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    arg_list_of(node, cx)
        .first()
        .map(|&first| cx.range(first).start)
}

/// The argument list of a def/defs (its `Args` children) or a call.
fn arg_list_of<'cx>(node: NodeId, cx: &Cx<'cx>) -> Vec<NodeId> {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => match cx.def_arguments(node).get() {
            Some(args_id) => match *cx.kind(args_id) {
                NodeKind::Args(list) => cx.list(list).to_vec(),
                _ => vec![],
            },
            None => vec![],
        },
        _ => get_call_args(node, cx),
    }
}

/// `true` when the def/call already has an argument-list `(`.
fn node_has_arg_parens(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => def_has_parens(node, cx),
        _ => cx.loc(node).begin() != Range::ZERO,
    }
}

/// Byte offset just after the method name / keyword — where a `(` is inserted.
fn node_callee_end(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => def_name_end(node, cx),
        NodeKind::Send { .. } | NodeKind::Csend { .. } => Some(cx.loc(node).name.end),
        // `super`/`yield` keywords are exactly 5 bytes at the node start.
        NodeKind::Super(_) | NodeKind::Yield(_) => Some(cx.range(node).start + 5),
        _ => None,
    }
}

/// Byte offset just after a def's method-name token (handles `def self.foo`,
/// operator names, etc.).
fn def_name_end(def_node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let sym = match *cx.kind(def_node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => name,
        _ => return None,
    };
    let name_str = cx.symbol_str(sym);
    let node_range = cx.range(def_node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_str.as_bytes()
        })
        .map(|t| t.range.end)
}

fn def_has_parens(def_node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(name_end) = def_name_end(def_node, cx) else {
        return false;
    };
    let toks = cx.sorted_tokens();
    let after_idx = toks.partition_point(|t| t.range.start < name_end);
    toks.get(after_idx)
        .is_some_and(|t| t.kind == SourceTokenKind::LeftParen)
}

// ── Small node helpers ──────────────────────────────────────────────────────

fn is_forwarded_rest(arg: NodeId, rest_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Splat(inner) = *cx.kind(arg) else {
        return false;
    };
    inner
        .get()
        .is_some_and(|id| matches!(*cx.kind(id), NodeKind::Lvar(s) if cx.symbol_str(s) == rest_name))
}

/// Find a `(kwsplat (lvar kwrest_name))` inside a call's hash argument; returns
/// the kwsplat node (RuboCop's captured `$(kwsplat (lvar %1))`).
fn forwarded_kwrest(arg: NodeId, kwrest_name: &str, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Hash(list) = *cx.kind(arg) else {
        return None;
    };
    cx.list(list).iter().copied().find(|&c| {
        matches!(*cx.kind(c), NodeKind::Kwsplat(inner)
            if inner.get().is_some_and(|id| matches!(*cx.kind(id), NodeKind::Lvar(s) if cx.symbol_str(s) == kwrest_name)))
    })
}

/// `(block_pass {(lvar block_name) nil?})` — a block pass of the named lvar, or
/// a bare `&` (nil inner, which matches for any block name).
fn is_forwarded_block(arg: NodeId, block_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::BlockPass(inner) = *cx.kind(arg) else {
        return false;
    };
    match inner.get() {
        None => true,
        Some(id) => matches!(*cx.kind(id), NodeKind::Lvar(s) if cx.symbol_str(s) == block_name),
    }
}

/// Number of children in the hash that owns a forwarded kwsplat node.
fn hash_child_count(kwsplat: NodeId, cx: &Cx<'_>) -> usize {
    match cx.parent(kwsplat).get() {
        Some(parent) => match *cx.kind(parent) {
            NodeKind::Hash(list) => cx.list(list).len(),
            _ => 1,
        },
        None => 1,
    }
}

fn get_call_args(call_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match *cx.kind(call_id) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => cx.list(args).to_vec(),
        NodeKind::Super(list) | NodeKind::Yield(list) => cx.list(list).to_vec(),
        _ => vec![],
    }
}

fn is_call_node(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::Send { .. } | NodeKind::Csend { .. } | NodeKind::Super(_) | NodeKind::Yield(_)
    )
}

fn collect_call_nodes(body_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let mut result: Vec<NodeId> = Vec::new();
    if is_call_node(body_id, cx) {
        result.push(body_id);
    }
    result.extend(
        cx.descendants(body_id)
            .into_iter()
            .filter(|&id| is_call_node(id, cx)),
    );
    result
}

/// RuboCop's `non_splat_or_block_pass_lvar_references`: names of every `lvar`
/// (unless its parent is splat/kwsplat/block_pass) and every `lvasgn` in the body.
fn collect_referenced_lvars<'a>(body_id: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for id in cx.descendants(body_id) {
        match *cx.kind(id) {
            NodeKind::Lvar(sym) => {
                let skip = cx.parent(id).get().is_some_and(|p| {
                    matches!(
                        *cx.kind(p),
                        NodeKind::Splat(_) | NodeKind::Kwsplat(_) | NodeKind::BlockPass(_)
                    )
                });
                if !skip {
                    out.push(cx.symbol_str(sym));
                }
            }
            NodeKind::Lvasgn { name, .. } => out.push(cx.symbol_str(name)),
            _ => {}
        }
    }
    out.dedup();
    out
}

fn find_arg_kind(args: &[NodeId], cx: &Cx<'_>, pred: impl Fn(&NodeKind) -> bool) -> Option<NodeId> {
    args.iter().copied().find(|&id| pred(cx.kind(id)))
}

/// True when `name` is anonymous (empty — the bare `*`/`**`/`&`) or appears in
/// the configured redundant-name list. Mirrors RuboCop's `redundant_named_arg`
/// (`[keyword+name …] << keyword`), so the bare keyword always counts.
fn is_redundant_name(name: &str, redundant_names: &[String]) -> bool {
    name.is_empty() || redundant_names.iter().any(|n| n == name)
}

fn forwardable_restarg(id: Option<NodeId>, redundant: &[String], cx: &Cx<'_>) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Restarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant).then_some(id)
}

fn forwardable_kwrestarg(id: Option<NodeId>, redundant: &[String], cx: &Cx<'_>) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Kwrestarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant).then_some(id)
}

fn forwardable_blockarg(id: Option<NodeId>, redundant: &[String], cx: &Cx<'_>) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Blockarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant).then_some(id)
}

fn restarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Restarg(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

fn kwrestarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Kwrestarg(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

fn blockarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Blockarg(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

murphy_plugin_api::submit_cop!(ArgumentsForwarding);

#[cfg(test)]
mod tests {
    use super::{ArgumentsForwarding, ArgumentsForwardingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ── Forward-all `...` (Ruby 2.7+, murphy default target 3.1) ────────────

    #[test]
    fn flags_restarg_and_block_arg() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, &block)
                        ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, &block)
                      ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_restarg_kwrestarg_and_block_arg() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **kwargs, &block)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_redundant_opts_name() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **opts, &block)
                        ^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **opts, &block)
                      ^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_redundant_blk_name_as_dots() {
        // `*args` + redundant `&blk` is forward-all `...` at 3.1.
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, &blk)
                        ^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, &blk)
                      ^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_leading_required_arg_before_splat() {
        // `arguments_range` starts at the splat, leaving the leading `a` intact.
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(a, *args, &block)
                           ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(a, *args, &block)
                         ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(a, ...)
                  bar(a, ...)
                end
            "#},
        );
    }

    #[test]
    fn flags_partial_forward_sites_as_dots_at_3_1() {
        // At 3.1 even a call forwarding a subset (`baz(*args, &block)`) collapses
        // to `...`; RuboCop accepts the resulting over-forwarding.
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **kwargs, &block)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  baz(*args, &block)
                      ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                  baz(...)
                end
            "#},
        );
    }

    // ── Anonymous block `&` (Ruby 3.1+) ─────────────────────────────────────

    #[test]
    fn flags_block_only_as_anonymous() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(&block)
                        ^^^^^^ Use anonymous block arguments forwarding (`&`).
                  bar(&block)
                      ^^^^^^ Use anonymous block arguments forwarding (`&`).
                end
            "#},
            indoc! {r#"
                def foo(&)
                  bar(&)
                end
            "#},
        );
    }

    #[test]
    fn flags_block_when_rest_name_is_meaningful() {
        // `*meaningful` is left alone; only the redundant block is anonymized.
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*meaningful, &block)
                                     ^^^^^^ Use anonymous block arguments forwarding (`&`).
                  bar(*meaningful, &block)
                                   ^^^^^^ Use anonymous block arguments forwarding (`&`).
                end
            "#},
            indoc! {r#"
                def foo(*meaningful, &)
                  bar(*meaningful, &)
                end
            "#},
        );
    }

    #[test]
    fn flags_block_when_kwrest_name_is_meaningful() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(**my_special_kwargs, &block)
                                             ^^^^^^ Use anonymous block arguments forwarding (`&`).
                  bar(**my_special_kwargs, &block)
                                           ^^^^^^ Use anonymous block arguments forwarding (`&`).
                end
            "#},
            indoc! {r#"
                def foo(**my_special_kwargs, &)
                  bar(**my_special_kwargs, &)
                end
            "#},
        );
    }

    // ── Anonymous `*`/`**`/`&` (Ruby 3.2+) ──────────────────────────────────

    #[test]
    fn anon_rest_and_block_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(*args, &block)
                            ^^^^^ Use anonymous positional arguments forwarding (`*`).
                                   ^^^^^^ Use anonymous block arguments forwarding (`&`).
                      bar(*args, &block)
                          ^^^^^ Use anonymous positional arguments forwarding (`*`).
                                 ^^^^^^ Use anonymous block arguments forwarding (`&`).
                    end
                "#},
                indoc! {r#"
                    def foo(*, &)
                      bar(*, &)
                    end
                "#},
            );
    }

    #[test]
    fn keeps_dots_when_rest_and_kwrest_at_3_2() {
        // Forwarding BOTH rest and kwrest (plus block) stays `...` even at 3.2.
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(*args, **kwargs, &block)
                            ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, **kwargs, &block)
                          ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn anon_kwrest_only_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(**kwargs)
                            ^^^^^^^^ Use anonymous keyword arguments forwarding (`**`).
                      bar(**kwargs)
                          ^^^^^^^^ Use anonymous keyword arguments forwarding (`**`).
                    end
                "#},
                indoc! {r#"
                    def foo(**)
                      bar(**)
                    end
                "#},
            );
    }

    #[test]
    fn anon_rest_only_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(*args)
                            ^^^^^ Use anonymous positional arguments forwarding (`*`).
                      bar(*args)
                          ^^^^^ Use anonymous positional arguments forwarding (`*`).
                    end
                "#},
                indoc! {r#"
                    def foo(*)
                      bar(*)
                    end
                "#},
            );
    }

    #[test]
    fn anon_rest_and_kwrest_without_block_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(*args, **kwargs)
                            ^^^^^ Use anonymous positional arguments forwarding (`*`).
                                   ^^^^^^^^ Use anonymous keyword arguments forwarding (`**`).
                      bar(*args, **kwargs)
                          ^^^^^ Use anonymous positional arguments forwarding (`*`).
                                 ^^^^^^^^ Use anonymous keyword arguments forwarding (`**`).
                    end
                "#},
                indoc! {r#"
                    def foo(*, **)
                      bar(*, **)
                    end
                "#},
            );
    }

    #[test]
    fn collapses_already_anonymous_to_dots_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_correction(
                indoc! {r#"
                    def foo(*, **, &)
                            ^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*, **, &)
                          ^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn use_anonymous_forwarding_false_suppresses_at_3_2() {
        // With anonymous forwarding disabled, a partial-forward shape that is
        // not forward-all-eligible produces no offense at 3.2.
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .with_options(&ArgumentsForwardingOptions {
                use_anonymous_forwarding: false,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                def foo(*args, &block)
                  bar(*args, &block)
                end
            "#});
    }

    // ── Block-in-block gate (Ruby 3.3.0 syntax-error workaround) ─────────────

    #[test]
    fn accepts_block_forwarding_inside_block_below_3_4() {
        // At 3.1 anonymous forwarding inside a block is suppressed.
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(&block)
              thing { bar(&block) }
            end
        "#});
    }

    #[test]
    fn suppresses_block_forwarding_inside_block_at_3_2() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 2)
            .expect_no_offenses(indoc! {r#"
                def foo(&block)
                  thing { bar(&block) }
                end
            "#});
    }

    #[test]
    fn flags_block_forwarding_inside_block_at_3_4() {
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 4)
            .expect_correction(
                indoc! {r#"
                    def foo(&block)
                            ^^^^^^ Use anonymous block arguments forwarding (`&`).
                      thing { bar(&block) }
                                  ^^^^^^ Use anonymous block arguments forwarding (`&`).
                    end
                "#},
                indoc! {r#"
                    def foo(&)
                      thing { bar(&) }
                    end
                "#},
            );
    }

    // ── Ruby ≤ 3.0 (anonymous block forwarding unavailable) ─────────────────

    #[test]
    fn accepts_block_only_below_3_1() {
        // Anonymous block forwarding `&` is Ruby 3.1+; at 3.0 there is no offense.
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses(indoc! {r#"
                def foo(&block)
                  bar(&block)
                end
            "#});
    }

    #[test]
    fn flags_rest_and_block_as_dots_below_3_1() {
        // Forward-all `...` is valid from Ruby 2.7.
        test::<ArgumentsForwarding>()
            .with_target_ruby_version(3, 0)
            .expect_correction(
                indoc! {r#"
                    def foo(*args, &block)
                            ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, &block)
                          ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    // ── Acceptances (version-stable at murphy's default 3.1) ────────────────

    #[test]
    fn accepts_already_forwarding() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(...)
              bar(...)
            end
        "#});
    }

    #[test]
    fn accepts_args_used_outside_forwarding() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              args.do_something
              bar(*args, **kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_args_reassigned() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              args = new_args
              bar(*args, **kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_empty_body() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
            end
        "#});
    }

    #[test]
    fn accepts_meaningful_block_name() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &my_callback)
              bar(*args, &my_callback)
            end
        "#});
    }

    #[test]
    fn accepts_not_always_forwarding_block() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
              bar(*args, &block)
              baz(*args)
            end
        "#});
    }

    #[test]
    fn accepts_block_forwarded_to_separate_call() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
              bar(*args).baz(&block)
            end
        "#});
    }

    #[test]
    fn accepts_kwargs_with_additional_kwarg() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(first:, **kwargs, &block)
              forwarded(**kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_args_forwarded_to_separate_receiver_methods() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              bar(first(*args), second(**kwargs), third(&block))
            end
        "#});
    }

    #[test]
    fn accepts_only_rest_arg_by_default() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args)
              bar(*args)
            end
        "#});
    }

    #[test]
    fn accepts_only_kwrest_arg_by_default() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(**kwargs)
              bar(**kwargs)
            end
        "#});
    }

    #[test]
    fn accepts_only_rest_and_kwrest_without_block_by_default() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs)
              bar(*args, **kwargs)
            end
        "#});
    }

    // ── AllowOnlyRestArgument ───────────────────────────────────────────────

    #[test]
    fn options_defaults_match_rubocop() {
        let d = ArgumentsForwardingOptions::default();
        assert!(d.allow_only_rest_argument);
        assert!(d.use_anonymous_forwarding);
        assert_eq!(d.redundant_rest_argument_names, ["args", "arguments"]);
        assert_eq!(
            d.redundant_keyword_rest_argument_names,
            ["kwargs", "options", "opts"]
        );
        assert_eq!(d.redundant_block_argument_names, ["blk", "block", "proc"]);
    }

    #[test]
    fn flags_only_rest_arg_when_allow_only_rest_argument_false() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                allow_only_rest_argument: false,
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args)
                            ^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args)
                          ^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn flags_rest_and_kwrest_without_block_when_allow_only_rest_argument_false() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                allow_only_rest_argument: false,
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args, **kwargs)
                            ^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, **kwargs)
                          ^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    // ── Redundant*ArgumentNames ─────────────────────────────────────────────

    #[test]
    fn flags_custom_redundant_rest_name() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_rest_argument_names: vec!["sploosh".to_string()],
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*sploosh, &block)
                            ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*sploosh, &block)
                          ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn flags_block_when_rest_name_not_in_custom_list() {
        // With `args` no longer redundant, only the block is anonymized at 3.1.
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_rest_argument_names: vec!["sploosh".to_string()],
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args, &block)
                                   ^^^^^^ Use anonymous block arguments forwarding (`&`).
                      bar(*args, &block)
                                 ^^^^^^ Use anonymous block arguments forwarding (`&`).
                    end
                "#},
                indoc! {r#"
                    def foo(*args, &)
                      bar(*args, &)
                    end
                "#},
            );
    }

    #[test]
    fn flags_custom_redundant_block_name() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_block_argument_names: vec!["callback".to_string()],
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args, &callback)
                            ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, &callback)
                          ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }
}
