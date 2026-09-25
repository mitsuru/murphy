//! `Rails/Delegate` — flag trivial delegations that could use `delegate`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Delegate
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 core shapes: single-expression `def`
//!   whose body is a bare `Send` (safe navigation `&.`/Csend is ignored)
//!   with a simple receiver (bare call, `self`, `self.class`, cvar/gvar/ivar,
//!   const), exact method-name or `prefix: true` match
//!   (`EnforceForPrefixed`, default true), and exact positional-arg match
//!   (plain `arg` ↔ `lvar`). Private/protected (`private def`, bare
//!   `private` sections) and `module_function` declarations are skipped.
//!   Offense on the `def` keyword; autocorrect replaces the whole `def`
//!   with `delegate :m, to: :recv[, prefix: true]`. Controllers exemption
//!   (upstream disables for controllers) is not implemented — Murphy has no
//!   path gating in v1. Keyword/block-arg delegation is not implemented.
//! ```
//!
//! ## Matched shapes
//!
//! - `def bar; foo.bar; end` → `delegate :bar, to: :foo`.
//! - `def foo_bar; foo.bar; end` (prefixed) → `delegate :bar, to: :foo, prefix: true`.
//! - `def bar(x); foo.bar(x); end` → `delegate :bar, to: :foo` (args pass through the macro form).
//!
//! `def bar; foo&.bar; end`, `private def bar; ...`, and
//! `def bar; foo.bar(1); end` (arg mismatch) do not flag.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Delegate;

#[derive(CopOptions)]
pub struct DelegateOptions {
    #[option(
        name = "EnforceForPrefixed",
        default = true,
        description = "Whether to flag prefixed delegations (`def foo_bar; foo.bar; end`)."
    )]
    pub enforce_for_prefixed: bool,
}

#[cop(
    name = "Rails/Delegate",
    description = "Use `delegate` to define delegations.",
    default_severity = "warning",
    default_enabled = true,
    options = DelegateOptions,
)]
impl Delegate {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_node(node, cx);
    }
}

fn check_def_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Def {
        receiver, name, args, body,
    } = *cx.kind(node)
    else {
        return;
    };
    // Only instance-method defs; `def self.x` is not a delegation.
    if receiver.get().is_some() {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    // Body must be exactly one `Send` (Csend/`&.` is ignored upstream).
    let NodeKind::Send {
        receiver: body_recv,
        method: body_method,
        ..
    } = *cx.kind(body_id)
    else {
        return;
    };
    let Some(recv_id) = body_recv.get() else {
        return;
    };
    if !is_simple_receiver(cx, recv_id) {
        return;
    }
    let def_name = cx.symbol_str(name).to_owned();
    let body_name = cx.symbol_str(body_method).to_owned();
    let opts = cx.options_or_default::<DelegateOptions>();
    let prefixed = prefixed_name(cx, recv_id, &body_name);
    let name_ok = def_name == body_name
        || (opts.enforce_for_prefixed
            && !prefixed.is_empty()
            && def_name == prefixed);
    if !name_ok {
        return;
    }
    if !args_match(cx, args, body_id) {
        return;
    }
    if is_private_or_protected(cx, node) || has_module_function(cx, node) {
        return;
    }
    cx.emit_offense(
        cx.loc(node).keyword(),
        "Use `delegate` to define delegations.",
        None,
    );
    let to = receiver_inner(cx, recv_id);
    let mut delegation = format!("delegate :{body_name}, to: {to}");
    if def_name == prefixed && def_name != body_name {
        delegation.push_str(", prefix: true");
    }
    cx.emit_edit(cx.range(node), &delegation);
}

/// Upstream receiver allow-list: bare call, `self`, `self.class`,
/// cvar/gvar/ivar, const. Bare call must be zero-arg.
fn is_simple_receiver(cx: &Cx<'_>, recv: NodeId) -> bool {
    match *cx.kind(recv) {
        NodeKind::Send { receiver, .. } => {
            // `(send nil? _)` — bare zero-arg call.
            receiver.get().is_none() && cx.call_arguments(recv).is_empty()
        }
        NodeKind::SelfExpr => true,
        NodeKind::Cvar(_) | NodeKind::Gvar(_) | NodeKind::Ivar(_) => true,
        NodeKind::Const { .. } => true,
        _ => {
            // `(send (self) :class)` — `self.class`.
            if let NodeKind::Send {
                receiver: inner_recv,
                method,
                ..
            } = *cx.kind(recv)
            {
                if cx.symbol_str(method) != "class" {
                    return false;
                }
                if let Some(inner) = inner_recv.get() {
                    return matches!(*cx.kind(inner), NodeKind::SelfExpr)
                        && cx.call_arguments(recv).is_empty();
                }
            }
            false
        }
    }
}

fn prefixed_name(cx: &Cx<'_>, recv: NodeId, body_method: &str) -> String {
    // `self` receiver never prefixes (upstream returns '').
    if matches!(*cx.kind(recv), NodeKind::SelfExpr) {
        return String::new();
    }
    let recv_name = match *cx.kind(recv) {
        NodeKind::Send { method, .. } => cx.symbol_str(method).to_owned(),
        NodeKind::Cvar(s) | NodeKind::Gvar(s) | NodeKind::Ivar(s) => {
            cx.symbol_str(s).to_owned()
        }
        NodeKind::Const { .. } => const_name(cx, recv),
        _ => {
            // `self.class` — not a useful prefix; treat as empty.
            return String::new();
        }
    };
    format!("{recv_name}_{body_method}")
}

fn const_name(cx: &Cx<'_>, node: NodeId) -> String {
    // Walk const scope chain to build `A::B`.
    let mut parts = vec![];
    let mut cur = Some(node);
    while let Some(id) = cur {
        if let NodeKind::Const { scope, name } = *cx.kind(id) {
            parts.push(cx.symbol_str(name).to_owned());
            cur = scope.get();
            // Cbase (`::Foo`) terminates; its marker has no name.
            if let Some(s) = cur
                && matches!(*cx.kind(s), NodeKind::Cbase) {
                    break;
                }
        } else {
            break;
        }
    }
    parts.reverse();
    parts.join("::")
}

fn receiver_inner(cx: &Cx<'_>, recv: NodeId) -> String {
    match *cx.kind(recv) {
        NodeKind::SelfExpr => "self".to_owned(),
        NodeKind::Cvar(s) | NodeKind::Gvar(s) | NodeKind::Ivar(s) => {
            format!(":{}", cx.symbol_str(s))
        }
        NodeKind::Const { .. } => {
            let full = const_name(cx, recv);
            if full.contains("::") {
                format!(":'{full}'")
            } else {
                format!(":{full}")
            }
        }
        NodeKind::Send { method, .. } => {
            // Bare call or `self.class`.
            if cx.symbol_str(method) == "class" {
                "self".to_owned()
            } else {
                format!(":{}", cx.symbol_str(method))
            }
        }
        _ => "self".to_owned(),
    }
}

/// Plain positional args only: same length, each def `arg` ↔ body `lvar`
/// with the same name.
fn args_match(cx: &Cx<'_>, def_args: NodeId, body_send: NodeId) -> bool {
    let def_params = match *cx.kind(def_args) {
        NodeKind::Args(list) => cx.list(list).to_vec(),
        _ => return false,
    };
    let body_args = cx.call_arguments(body_send);
    if def_params.len() != body_args.len() {
        return false;
    }
    for (d, b) in def_params.iter().zip(body_args.iter()) {
        let NodeKind::Arg(name) = *cx.kind(*d) else {
            return false;
        };
        let NodeKind::Lvar(bname) = *cx.kind(*b) else {
            return false;
        };
        if name != bname {
            return false;
        }
    }
    true
}

/// `private def` / `protected def` modifier plus bare-section visibility.
fn is_private_or_protected(cx: &Cx<'_>, node: NodeId) -> bool {
    // `private def foo` — def is the argument of a modifier send.
    for anc in cx.ancestors(node) {
        if let NodeKind::Send { method, .. } = *cx.kind(anc) {
            let m = cx.symbol_str(method);
            if m == "private" || m == "protected" {
                // If this send's modifier chain reaches our def, it applies.
                if reaches_def(cx, anc, node) {
                    return true;
                }
            }
        }
        // Only walk through directly-wrapping nodes; a bare section lives
        // in the enclosing Begin, handled below.
        if matches!(
            *cx.kind(anc),
            NodeKind::Class { .. } | NodeKind::Module { .. }
        ) {
            break;
        }
    }
    // Bare `private` / `protected` section: last access modifier before the
    // def in the enclosing body determines visibility.
    if let Some(vis) = section_visibility(cx, node) {
        return vis == "private" || vis == "protected";
    }
    false
}

fn reaches_def(cx: &Cx<'_>, send: NodeId, def: NodeId) -> bool {
    let mut cur = cx.call_arguments(send).first().copied();
    while let Some(id) = cur {
        if id == def {
            return true;
        }
        // Chained modifiers: `private(public(def))` — descend.
        if matches!(*cx.kind(id), NodeKind::Send { .. }) {
            cur = cx.call_arguments(id).first().copied();
        } else {
            break;
        }
    }
    false
}

fn section_visibility(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    // Find the nearest enclosing list-like body (Begin/Class/Module) and
    // scan for the last bare access modifier before our def.
    for anc in cx.ancestors(node) {
        let kids: Vec<NodeId> = match *cx.kind(anc) {
            NodeKind::Begin(list) => cx.list(list).to_vec(),
            NodeKind::Class { body, .. } | NodeKind::Module { body, .. } => {
                match body.get() {
                    Some(b) => match *cx.kind(b) {
                        NodeKind::Begin(list) => cx.list(list).to_vec(),
                        _ => vec![b],
                    },
                    None => vec![],
                }
            }
            _ => continue,
        };
        let pos = kids.iter().position(|&k| k == node)?;
        let mut vis: Option<String> = None;
        for &kid in &kids[..pos] {
            if let NodeKind::Send { receiver, method, .. } = *cx.kind(kid) {
                if receiver.get().is_some() {
                    continue;
                }
                let m = cx.symbol_str(method);
                if (m == "private" || m == "protected" || m == "public")
                    && cx.call_arguments(kid).is_empty() {
                        vis = Some(m.to_owned());
                    }
            }
        }
        return vis;
    }
    None
}

fn has_module_function(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        match *cx.kind(anc) {
            NodeKind::Module { body, .. } => {
                if body_has_module_function(cx, body.get()) {
                    return true;
                }
            }
            NodeKind::Begin(list) => {
                for &kid in cx.list(list) {
                    if let NodeKind::Send { method, .. } = *cx.kind(kid)
                        && cx.symbol_str(method) == "module_function" {
                            return true;
                        }
                }
            }
            _ => {}
        }
    }
    false
}

fn body_has_module_function(cx: &Cx<'_>, body: Option<NodeId>) -> bool {
    let Some(b) = body else {
        return false;
    };
    let kids: Vec<NodeId> = match *cx.kind(b) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![b],
    };
    kids.iter().any(|&k| {
        matches!(*cx.kind(k), NodeKind::Send { method, .. } if cx.symbol_str(method) == "module_function")
    })
}

#[cfg(test)]
mod tests {
    use super::{Delegate, DelegateOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_simple_delegation() {
        test::<Delegate>().expect_offense(indoc! {r#"
            def bar
            ^^^ Use `delegate` to define delegations.
              foo.bar
            end
        "#});
    }

    #[test]
    fn flags_prefixed_delegation() {
        test::<Delegate>().expect_offense(indoc! {r#"
            def foo_bar
            ^^^ Use `delegate` to define delegations.
              foo.bar
            end
        "#});
    }

    #[test]
    fn does_not_flag_prefixed_when_disabled() {
        let opts = DelegateOptions {
            enforce_for_prefixed: false,
        };
        test::<Delegate>()
            .with_options(&opts)
            .expect_no_offenses("def foo_bar\n  foo.bar\nend\n");
    }

    #[test]
    fn flags_with_args() {
        test::<Delegate>().expect_offense(indoc! {r#"
            def bar(x)
            ^^^ Use `delegate` to define delegations.
              foo.bar(x)
            end
        "#});
    }

    #[test]
    fn does_not_flag_arg_mismatch() {
        test::<Delegate>().expect_no_offenses("def bar\n  foo.bar(1)\nend\n");
    }

    #[test]
    fn does_not_flag_safe_navigation() {
        test::<Delegate>().expect_no_offenses("def bar\n  foo&.bar\nend\n");
    }

    #[test]
    fn does_not_flag_private_section() {
        test::<Delegate>().expect_no_offenses("private\ndef bar\n  foo.bar\nend\n");
    }

    #[test]
    fn corrects_simple_delegation() {
        test::<Delegate>()
            .expect_correction(
                indoc! {r#"
                    def bar
                    ^^^ Use `delegate` to define delegations.
                      foo.bar
                    end
                "#},
                "delegate :bar, to: :foo\n",
            )
            .expect_no_offenses("delegate :bar, to: :foo\n");
    }

    #[test]
    fn corrects_prefixed_delegation() {
        test::<Delegate>()
            .expect_correction(
                indoc! {r#"
                    def foo_bar
                    ^^^ Use `delegate` to define delegations.
                      foo.bar
                    end
                "#},
                "delegate :bar, to: :foo, prefix: true\n",
            )
            .expect_no_offenses("delegate :bar, to: :foo, prefix: true\n");
    }
}
murphy_plugin_api::submit_cop!(Delegate);
