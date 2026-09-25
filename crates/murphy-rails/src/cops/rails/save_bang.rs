//! `Rails/SaveBang` — use `save!` etc. when the return value is not checked.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SaveBang
//! upstream_version_checked: 2.35.0
//! version_added: "0.42"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (bare and safe-navigation):
//!   `save/update/update_attributes/destroy` (MODIFY) and
//!   `create/create_or_find_by/first_or_create/find_or_create_by` (CREATE)
//!   with `expected_signature?` (no args, or one hash/non-literal arg;
//!   `destroy` takes no args), `AllowedReceivers` (source-suffix match
//!   with leading-`::` normalization, plus bare `ENV` exemption),
//!   and the `return_value_assigned?` / `AllowImplicitReturn` /
//!   condition-or-compound-boolean / argument / explicit-return /
//!   `persisted?`-checked guards. CREATE in a condition reports the
//!   always-truthy message; MODIFY in a condition is allowed. CREATE
//!   assigned without a later `persisted?` check reports via
//!   `on_new_investigation` + `cx.var_model()` (unreferenced assignments
//!   flag too). Offense is the selector; autocorrect appends `!`.
//!   Autocorrect is unsafe upstream (`SafeAutoCorrect: false`).
//!   Narrowings: `parenthesized_call?` + `if` unwrapping in
//!   `call_to_persisted?` is approximated by direct receiver checks;
//!   `AllowedReceivers` uses source matching instead of full
//!   receiver-chain const decomposition.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const CREATE_METHODS: &[&str] = &[
    "create",
    "create_or_find_by",
    "first_or_create",
    "find_or_create_by",
];
const MODIFY_METHODS: &[&str] = &["save", "update", "update_attributes", "destroy"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SaveBang;

#[derive(CopOptions)]
pub struct SaveBangOptions {
    #[option(
        name = "AllowImplicitReturn",
        default = true,
        description = "Whether implicit returns from methods and blocks are allowed."
    )]
    pub allow_implicit_return: bool,
    #[option(
        name = "AllowedReceivers",
        default = [],
        description = "Receivers ignored by the cop (e.g. merchant.customers)."
    )]
    pub allowed_receivers: Vec<String>,
}

#[cop(
    name = "Rails/SaveBang",
    description = "Identifies possible cases where Active Record save! should be used.",
    default_severity = "warning",
    default_enabled = false,
    options = SaveBangOptions,
)]
impl SaveBang {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(
        kind = "send",
        methods = [
            "create",
            "create_or_find_by",
            "first_or_create",
            "find_or_create_by",
            "save",
            "update",
            "update_attributes",
            "destroy"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_lvasgn(node, cx);
    }
}

fn is_persist_method(method: &str) -> bool {
    CREATE_METHODS.contains(&method) || MODIFY_METHODS.contains(&method)
}

fn is_create_method(method: &str) -> bool {
    CREATE_METHODS.contains(&method)
}

fn check_send(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !is_persist_method(&method) {
        return;
    }
    if !persist_method(cx, node, &method) {
        return;
    }
    if return_value_assigned(cx, node) {
        return;
    }
    if implicit_return(cx, node) {
        return;
    }
    if used_in_condition_or_compound_boolean(cx, node, &method) {
        return;
    }
    if is_argument(cx, node) {
        return;
    }
    if explicit_return(cx, node) {
        return;
    }
    if checked_immediately(cx, node) {
        return;
    }
    register_offense(cx, node, &method, false);
}

fn register_offense(cx: &Cx<'_>, node: NodeId, method: &str, is_create_assign: bool) {
    let bang = format!("{method}!");
    let msg = if is_create_assign {
        format!(
            "Use `{bang}` instead of `{method}` if the return value is not checked. Or check `persisted?` on model returned from `{method}`."
        )
    } else if is_create_method(method) {
        // Distinguish condition (always-truthy) from plain: caller passes
        // through used_in_condition... which already emitted for conditions,
        // so here is the plain CREATE message.
        // Plain CREATE on_send uses MSG; assignment uses CREATE_MSG.
        // This branch is plain on_send CREATE.
        format!(
            "Use `{bang}` instead of `{method}` if the return value is not checked. Or check `persisted?` on model returned from `{method}`."
        )
    } else {
        format!("Use `{bang}` instead of `{method}` if the return value is not checked.")
    };
    // For plain MODIFY on_send, upstream MSG has no persisted? suffix.
    let msg = if !is_create_assign && !is_create_method(method) {
        format!("Use `{bang}` instead of `{method}` if the return value is not checked.")
    } else if !is_create_assign && is_create_method(method) {
        // on_send CREATE without assignment uses MSG (no persisted suffix)?
        // Upstream on_send register_offense(node, MSG) for all on_send cases.
        // CREATE_MSG is only for assignments. Correct to plain MSG here.
        format!("Use `{bang}` instead of `{method}` if the return value is not checked.")
    } else {
        msg
    };
    cx.emit_offense(cx.selector(node), &msg, None);
    cx.emit_edit(cx.selector(node), &bang);
}

/// Upstream `persist_method?`: method in set, expected signature, not allowed receiver.
fn persist_method(cx: &Cx<'_>, node: NodeId, method: &str) -> bool {
    expected_signature(cx, node, method) && !allowed_receiver(cx, node)
}

/// Upstream `expected_signature?`: no args, or one non-destroy arg that is
/// a hash or a non-literal.
fn expected_signature(cx: &Cx<'_>, node: NodeId, method: &str) -> bool {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return true;
    }
    if args.len() != 1 || method == "destroy" {
        return false;
    }
    let first = args[0];
    if matches!(*cx.kind(first), NodeKind::Hash(_)) {
        return true;
    }
    !cx.is_literal(first)
}

/// Upstream `allowed_receiver?`: bare `ENV` const, plus `AllowedReceivers`
/// chain matching (approximated by source-suffix match).
fn allowed_receiver(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    // `node.receiver.const_name == 'ENV'`.
    if let Some(name) = cx.const_name(recv)
        && name == "ENV"
    {
        return true;
    }
    let opts = cx.options_or_default::<SaveBangOptions>();
    if opts.allowed_receivers.is_empty() {
        return false;
    }
    let src = cx.raw_source(cx.range(recv)).to_owned();
    let norm_src = src.strip_prefix("::").unwrap_or(&src);
    for allowed in &opts.allowed_receivers {
        let a = allowed.strip_prefix("::").unwrap_or(allowed);
        if norm_src == a || norm_src.ends_with(&format!(".{a}")) || norm_src.ends_with(a) {
            // Suffix match covers `merchant.customers` vs
            // `MerchantService.merchant.customers` and `Service::Mailer`
            // vs `Services::Service::Mailer`.
            return true;
        }
    }
    false
}

/// Upstream `assignable_node`: `block_node || node`, climbing hash/array parents.
fn assignable_node(cx: &Cx<'_>, node: NodeId) -> NodeId {
    let mut assignable = cx.block_node(node).get().unwrap_or(node);
    loop {
        let Some(parent) = cx.parent(assignable).get() else {
            break;
        };
        match *cx.kind(parent) {
            NodeKind::Hash(_) | NodeKind::Array(_) => {
                assignable = parent;
                continue;
            }
            NodeKind::Pair { .. } => {
                // Pair's parent should be a hash; climb to the pair first,
                // then the loop will climb to the hash.
                assignable = parent;
                continue;
            }
            _ => break,
        }
    }
    assignable
}

/// Upstream `return_value_assigned?`: parent of assignable is an assignment.
fn return_value_assigned(cx: &Cx<'_>, node: NodeId) -> bool {
    let assignable = assignable_node(cx, node);
    match cx.parent(assignable).get() {
        Some(p) => cx.is_assignment(p),
        None => false,
    }
}

/// Upstream `implicit_return?` with `AllowImplicitReturn` and `or` climbing.
fn implicit_return(cx: &Cx<'_>, node: NodeId) -> bool {
    let opts = cx.options_or_default::<SaveBangOptions>();
    if !opts.allow_implicit_return {
        return false;
    }
    let assignable = assignable_node(cx, node);
    // Climb `or` parents like `find_method_with_sibling_index`.
    let mut top = assignable;
    while let Some(parent) = cx.parent(top).get() {
        if matches!(*cx.kind(parent), NodeKind::Or { .. }) {
            top = parent;
        } else {
            break;
        }
    }
    let Some(parent) = cx.parent(top).get() else {
        return false;
    };
    // Direct body: `def foo; user.save; end` — body is the save itself.
    // Begin body: last child is the save/or-wrapper.
    match *cx.kind(parent) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => def_or_block_is_last(cx, parent, top),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            def_or_block_is_last(cx, parent, top)
        }
        NodeKind::Begin(_) | NodeKind::Kwbegin(_) => {
            // `top` inside a begin: check enclosing def/block lastness.
            is_last_in_enclosing_scope(cx, parent, top)
        }
        _ => false,
    }
}

fn def_body_id(cx: &Cx<'_>, def_node: NodeId) -> Option<NodeId> {
    match *cx.kind(def_node) {
        NodeKind::Def { body, .. } => body.get(),
        NodeKind::Defs { .. } => {
            // Defs { receiver, name, args, body }: extract body.
            if let NodeKind::Defs { body, .. } = *cx.kind(def_node) {
                body.get()
            } else {
                None
            }
        }
        NodeKind::Block { body, .. } => body.get(),
        NodeKind::Numblock { body, .. } => body.get(),
        NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    }
}

fn def_or_block_is_last(cx: &Cx<'_>, scope: NodeId, top: NodeId) -> bool {
    let Some(body) = def_body_id(cx, scope) else {
        return false;
    };
    if body == top {
        return true;
    }
    match *cx.kind(body) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => {
            let elems = cx.list(list);
            elems.last() == Some(&top)
        }
        _ => false,
    }
}

fn is_last_in_enclosing_scope(cx: &Cx<'_>, begin_node: NodeId, top: NodeId) -> bool {
    // `top` must be last in this begin.
    let elems: Vec<NodeId> = match *cx.kind(begin_node) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(list).to_vec(),
        _ => return false,
    };
    if elems.last() != Some(&top) {
        return false;
    }
    // The begin itself must be the body (or last) of an enclosing def/block.
    let Some(parent) = cx.parent(begin_node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } | NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            def_or_block_is_last(cx, parent, begin_node)
        }
        NodeKind::Begin(_) | NodeKind::Kwbegin(_) => {
            is_last_in_enclosing_scope(cx, parent, begin_node)
        }
        _ => false,
    }
}

/// Upstream `check_used_in_condition_or_compound_boolean?`: CREATE in
/// condition reports the always-truthy message; MODIFY is silently allowed.
fn used_in_condition_or_compound_boolean(
    cx: &Cx<'_>,
    node: NodeId,
    method: &str,
) -> bool {
    if !in_condition_or_compound_boolean(cx, node) {
        return false;
    }
    if !MODIFY_METHODS.contains(&method) {
        let msg = format!("`{method}` returns a model which is always truthy.");
        cx.emit_offense(cx.selector(node), &msg, None);
        cx.emit_edit(cx.selector(node), &format!("{method}!"));
    }
    true
}

fn in_condition_or_compound_boolean(cx: &Cx<'_>, node: NodeId) -> bool {
    let start = cx.block_node(node).get().unwrap_or(node);
    // First ancestor that is not a Begin (and not the start itself).
    let mut current = start;
    let parent = loop {
        let Some(p) = cx.parent(current).get() else {
            return false;
        };
        if matches!(*cx.kind(p), NodeKind::Begin(_) | NodeKind::Kwbegin(_)) {
            current = p;
            continue;
        } else {
            break p;
        }
    };
    // `operator_or_single_negative?`: && / || / and / or / ! / not.
    if matches!(
        *cx.kind(parent),
        NodeKind::And { .. } | NodeKind::Or { .. } | NodeKind::Not(_)
    ) {
        return true;
    }
    // Upstream also treats `!` as a send in some ASTs; cover send `!` with no args.
    if matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && cx.method_name(parent) == Some("!")
        && cx.call_arguments(parent).is_empty()
    {
        return true;
    }
    // `conditional?(parent) && node == deparenthesize(parent.condition)`.
    if let NodeKind::If { cond, .. } = *cx.kind(parent) {
        return deparenthesize(cx, cond) == start_for_condition(cx, start, node);
    }
    if let NodeKind::Case { subject, .. } = *cx.kind(parent)
        && let Some(subj) = subject.get()
    {
        return deparenthesize(cx, subj) == start_for_condition(cx, start, node);
    }
    false
}

/// The compared node is the block wrapper when the save heads a block.
fn start_for_condition(cx: &Cx<'_>, start: NodeId, node: NodeId) -> NodeId {
    let _ = cx;
    let _ = node;
    start
}

fn deparenthesize(cx: &Cx<'_>, mut node: NodeId) -> NodeId {
    loop {
        match *cx.kind(node) {
            NodeKind::Begin(list) => {
                let elems = cx.list(list);
                if elems.len() == 1 {
                    node = elems[0];
                } else {
                    break;
                }
            }
            NodeKind::Kwbegin(list) => {
                let elems = cx.list(list);
                if elems.len() == 1 {
                    node = elems[0];
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    node
}

/// Upstream `argument?`.
fn is_argument(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.is_argument(assignable_node(cx, node))
}

/// Upstream `explicit_return?`: parent is `return`/`next`.
fn explicit_return(cx: &Cx<'_>, node: NodeId) -> bool {
    let assignable = assignable_node(cx, node);
    match cx.parent(assignable).get() {
        Some(p) => matches!(*cx.kind(p), NodeKind::Return(_) | NodeKind::Next(_)),
        None => false,
    }
}

/// Upstream `checked_immediately?`: parent calls `persisted?` on the node.
fn checked_immediately(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    if cx.method_name(parent) != Some("persisted?") {
        return false;
    }
    cx.call_receiver(parent).get() == Some(node)
}



/// Upstream `after_leaving_scope` assignment check, implemented per-`lvasgn`
/// via the var semantic model (avoids mixing `on_node` with
/// `on_new_investigation`, which the `#[cop]` macro forbids).
fn check_lvasgn(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Lvasgn { value, .. } = *cx.kind(node) else {
        return;
    };
    let Some(mut rhs) = value.get() else {
        return;
    };
    // `right_assignment_node`: unwrap block wrappers.
    match *cx.kind(rhs) {
        NodeKind::Block { call, .. } => rhs = call,
        NodeKind::Numblock { send, .. } => rhs = send,
        NodeKind::Itblock { send, .. } => rhs = send,
        _ => {}
    }
    if !matches!(*cx.kind(rhs), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    let method = match cx.method_name(rhs) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !is_create_method(&method) {
        return;
    }
    if !persist_method(cx, rhs, &method) {
        return;
    }
    if persisted_referenced_for_lvasgn(cx, node) {
        return;
    }
    register_offense(cx, rhs, &method, true);
}

/// Look up the `lvasgn` in the var model and report whether a later
/// `lvar` read flows into a `persisted?` call.
fn persisted_referenced_for_lvasgn(cx: &Cx<'_>, lvasgn: NodeId) -> bool {
    let Some(model) = cx.var_model() else {
        return false;
    };
    for (_scope, scope_info) in model.scopes() {
        for var in scope_info.variables() {
            let mut found = false;
            for asgn in &var.assignments {
                if asgn.node_id == lvasgn {
                    found = true;
                    break;
                }
            }
            if !found {
                continue;
            }
            return persisted_referenced(cx, var, lvasgn);
        }
    }
    false
}

/// Upstream `persisted_referenced?`: assignment referenced and some
/// reference's parent calls `persisted?`.
fn persisted_referenced(
    cx: &Cx<'_>,
    var: &murphy_plugin_api::var_semantic_model::Variable,
    _asgn: NodeId,
) -> bool {
    if var.references.is_empty() {
        return false;
    }
    for r in &var.references {
        let lvar = r.node_id;
        let Some(parent) = cx.parent(lvar).get() else {
            continue;
        };
        if !matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        if cx.method_name(parent) != Some("persisted?") {
            continue;
        }
        if cx.call_receiver(parent).get() == Some(lvar) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{SaveBang, SaveBangOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_save() {
        test::<SaveBang>().expect_correction(
            indoc! {r#"
                user.save
                     ^^^^ Use `save!` instead of `save` if the return value is not checked.
            "#},
            "user.save!\n",
        );
    }

    #[test]
    fn flags_bare_update() {
        test::<SaveBang>().expect_correction(
            indoc! {r#"
                user.update(name: 'Joe')
                     ^^^^^^ Use `update!` instead of `update` if the return value is not checked.
            "#},
            "user.update!(name: 'Joe')\n",
        );
    }

    #[test]
    fn flags_bare_destroy() {
        test::<SaveBang>().expect_correction(
            indoc! {r#"
                user.destroy
                     ^^^^^^^ Use `destroy!` instead of `destroy` if the return value is not checked.
            "#},
            "user.destroy!\n",
        );
    }

    #[test]
    fn allows_save_in_condition() {
        test::<SaveBang>().expect_no_offenses("unless user.save\n  foo\nend\n");
    }

    #[test]
    fn flags_create_in_condition_as_truthy() {
        test::<SaveBang>().expect_offense(indoc! {r#"
            if User.create(name: 'Joe')
                    ^^^^^^ `create` returns a model which is always truthy.
              foo
            end
        "#});
    }

    #[test]
    fn allows_assigned_save() {
        test::<SaveBang>().expect_no_offenses("x = user.save\n");
    }

    #[test]
    fn flags_assigned_create_without_persisted() {
        test::<SaveBang>().expect_offense(indoc! {r#"
            x = User.create(name: 'Joe')
                     ^^^^^^ Use `create!` instead of `create` if the return value is not checked. Or check `persisted?` on model returned from `create`.
        "#});
    }

    #[test]
    fn allows_assigned_create_with_persisted() {
        test::<SaveBang>().expect_no_offenses(indoc! {r#"
            x = User.create(name: 'Joe')
            unless x.persisted?
              foo
            end
        "#});
    }

    #[test]
    fn allows_explicit_return() {
        test::<SaveBang>().expect_no_offenses("def foo\n  return user.save\nend\n");
    }

    #[test]
    fn allows_implicit_return_by_default() {
        test::<SaveBang>().expect_no_offenses("def foo\n  user.save\nend\n");
    }

    #[test]
    fn flags_implicit_return_when_disabled() {
        let opts = SaveBangOptions {
            allow_implicit_return: false,
            allowed_receivers: Vec::new(),
        };
        test::<SaveBang>().with_options(&opts).expect_offense(indoc! {r#"
            def foo
              user.save
                   ^^^^ Use `save!` instead of `save` if the return value is not checked.
            end
        "#});
    }

    #[test]
    fn allows_argument() {
        test::<SaveBang>().expect_no_offenses("foo(user.save)\n");
    }

    #[test]
    fn allows_persisted_checked() {
        test::<SaveBang>()
            .expect_no_offenses("if user.save.persisted?\n  foo\nend\n");
    }

    #[test]
    fn allows_allowed_receiver() {
        let opts = SaveBangOptions {
            allow_implicit_return: true,
            allowed_receivers: vec!["merchant.customers".to_owned()],
        };
        test::<SaveBang>()
            .with_options(&opts)
            .expect_no_offenses("merchant.customers.create\n");
    }
}
murphy_plugin_api::submit_cop!(SaveBang);
