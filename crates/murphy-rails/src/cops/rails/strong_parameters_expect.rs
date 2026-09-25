//! `Rails/StrongParametersExpect` — use `params.expect` for strong parameters.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/StrongParametersExpect
//! upstream_version_checked: 2.35.0
//! version_added: "2.29"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (send + csend,
//!   `RESTRICT_ON_SEND = %i[[] require permit]`) behind
//!   `minimum_target_rails_version 8.0`: `params[key]` used as a method
//!   receiver (non-comparison, non-`[]`, non presence-check) or passed to
//!   a raising finder (`find`/`find_by!`/`find_sole_by`, including via an
//!   enclosing hash), `params.require(:u).permit(...)` and
//!   `params.permit(...).require(:u)` (single-pair hash with a matching
//!   key). Offense ranges and surgical edits mirror upstream
//!   (`[]`-span to `.expect(key)`, require/permit collapsing to
//!   `expect`). Autocorrect is unsafe upstream
//!   (`SafeAutoCorrect: false`). File scope (`Include:
//!   ['**/app/controllers/**/*.rb']`) is enforced via the murphy-rails
//!   pack default.yml (engine `cop_applies_to_file` gate).
//!   Narrowings: upstream `part_of_ignored_node?` / `ignore_node`
//!   de-duplication is not tracked (a `params[]` nested inside an
//!   already-flagged require/permit chain could report twice);
//!   multiline require/permit correction was additionally verified
//!   byte-for-byte against `rubocop -A` (comment-preserving case).
//!   `require_key` uses `: `-separator for sym/str/int args and ` => `
//!   otherwise (upstream branches on `respond_to?(:value)`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const RAISING_FINDER_METHODS: &[&str] = &["find", "find_by!", "find_sole_by"];
const PRESENCE_CHECK_METHODS: &[&str] = &["nil?", "blank?", "present?", "presence"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct StrongParametersExpect;

#[cop(
    name = "Rails/StrongParametersExpect",
    description = "Enforces the use of `ActionController::Parameters#expect`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl StrongParametersExpect {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    // Upstream `minimum_target_rails_version 8.0`.
    if !cx.rails_version_at_least(8, 0) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    match method.as_str() {
        "[]" => check_bracket(node, cx),
        "permit" => {
            check_require_permit(cx, node);
        }
        "require" => {
            check_permit_require(cx, node);
        }
        _ => {}
    }
}

/// Upstream `params_bracket_access`: `(send (send nil? :params) :[] $_)`.
fn bracket_key(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    if cx.method_name(node) != Some("[]") {
        return None;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    let recv = cx.call_receiver(node).get()?;
    if cx.method_name(recv) != Some("params") {
        return None;
    }
    if cx.call_receiver(recv).get().is_some() || !cx.call_arguments(recv).is_empty() {
        return None;
    }
    Some(args[0])
}

fn check_bracket(node: NodeId, cx: &Cx<'_>) {
    let Some(key) = bracket_key(cx, node) else {
        return;
    };
    if !offensive_bracket_access(cx, node) {
        return;
    }
    // Upstream `offense_range(node, node)` + replace with `.expect(key)`.
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    let key_src = cx.raw_source(cx.range(key)).to_owned();
    cx.emit_offense(range, &format!("Use `expect({key_src})` instead."), None);
    cx.emit_edit(range, &format!(".expect({key_src})"));
}

/// Upstream `offensive_bracket_access?`.
fn offensive_bracket_access(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if matches!(*cx.kind(parent), NodeKind::Or { .. }) {
        return false;
    }
    // `parent.each_ancestor(:call).any? { raising_finder_method? }`
    for anc in cx.ancestors(parent) {
        if matches!(*cx.kind(anc), NodeKind::Send { .. } | NodeKind::Csend { .. })
            && is_raising_finder(cx, anc)
        {
            return true;
        }
    }
    if !matches!(
        *cx.kind(parent),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return false;
    }
    if cx.call_receiver(parent).get() == Some(node) {
        if cx.is_comparison_method(parent) {
            return false;
        }
        let method = cx.method_name(parent);
        if method == Some("[]") {
            return false;
        }
        if method.is_some_and(|m| PRESENCE_CHECK_METHODS.contains(&m)) {
            return false;
        }
        true
    } else {
        is_raising_finder(cx, parent)
    }
}

fn is_raising_finder(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.method_name(node)
        .is_some_and(|m| RAISING_FINDER_METHODS.contains(&m))
}

/// Bare `params` with no receiver and no arguments.
fn is_bare_params(cx: &Cx<'_>, node: NodeId) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    cx.method_name(node) == Some("params")
        && cx.call_receiver(node).get().is_none()
        && cx.call_arguments(node).is_empty()
}

/// Upstream `params_require_permit` on a `permit` node: returns
/// `(require_node, permit_node)` = `(receiver, self)`.
fn require_receiver(cx: &Cx<'_>, permit: NodeId) -> Option<NodeId> {
    if cx.method_name(permit) != Some("permit") {
        return None;
    }
    if cx.call_arguments(permit).is_empty() {
        return None;
    }
    let require = cx.call_receiver(permit).get()?;
    if !matches!(*cx.kind(require), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return None;
    }
    if cx.method_name(require) != Some("require") {
        return None;
    }
    if cx.call_arguments(require).len() != 1 {
        return None;
    }
    let recv = cx.call_receiver(require).get()?;
    if !is_bare_params(cx, recv) {
        return None;
    }
    Some(require)
}

/// `params.require(:u).permit(...)` → `params.expect(u: [...])`.
fn check_require_permit(cx: &Cx<'_>, node: NodeId) {
    let Some(require) = require_receiver(cx, node) else {
        return;
    };
    let require_arg = cx.call_arguments(require)[0];
    let key = require_key(cx, require_arg);
    let permit_args: Vec<String> = cx
        .call_arguments(node)
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .collect();
    let prefer = format!("expect({key}[{}])", permit_args.join(", "));
    let range = Range {
        start: cx.selector(require).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(range, &format!("Use `{prefer}` instead."), None);
    // Remove `.require(:u)`: end of `params` through end of require call.
    let Some(params_recv) = cx.call_receiver(require).get() else {
        return;
    };
    let params_end = cx.range(params_recv).end;
    cx.emit_edit(
        Range {
            start: params_end,
            end: cx.range(require).end,
        },
        "",
    );
    cx.emit_edit(cx.selector(node), "expect");
    // Wrap the permit args: `user: [` before the first, `]` after the last.
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return;
    };
    let Some(&last) = args.last() else {
        return;
    };
    let first_start = cx.range(first).start;
    cx.emit_edit(
        Range {
            start: first_start,
            end: first_start,
        },
        &format!("{key}["),
    );
    let last_end = cx.range(last).end;
    cx.emit_edit(
        Range {
            start: last_end,
            end: last_end,
        },
        "]",
    );
}

/// Upstream `params_permit_require` on a `require` node: returns
/// `(require_node, permit_node)` = `(self, receiver)` when the receiver is
/// `params.permit(hash)` with a single pair whose key matches the require arg.
fn matching_permit_receiver(cx: &Cx<'_>, require: NodeId) -> Option<NodeId> {
    if cx.method_name(require) != Some("require") {
        return None;
    }
    let require_args = cx.call_arguments(require);
    if require_args.len() != 1 {
        return None;
    }
    let permit = cx.call_receiver(require).get()?;
    if !matches!(*cx.kind(permit), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return None;
    }
    if cx.method_name(permit) != Some("permit") {
        return None;
    }
    let permit_args = cx.call_arguments(permit);
    if permit_args.len() != 1 {
        return None;
    }
    if !matches!(*cx.kind(permit_args[0]), NodeKind::Hash(_)) {
        return None;
    }
    let pairs = cx.hash_pairs(permit_args[0]);
    if pairs.len() != 1 {
        return None;
    }
    let key = cx.pair_key(pairs[0]).get()?;
    // Upstream unifies `_require_param_name` by node equality (`:user`
    // equals the `user:` pair key), so compare values, not source.
    if !same_key(cx, require_args[0], key) {
        return None;
    }
    let recv = cx.call_receiver(permit).get()?;
    if !is_bare_params(cx, recv) {
        return None;
    }
    Some(permit)
}

/// `params.permit(...).require(:u)` → `params.expect(...)`.
fn check_permit_require(cx: &Cx<'_>, node: NodeId) {
    let Some(permit) = matching_permit_receiver(cx, node) else {
        return;
    };
    let permit_src = cx
        .call_arguments(permit)
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .collect::<Vec<_>>()
        .join(", ");
    let prefer = format!("expect({permit_src})");
    let range = Range {
        start: cx.selector(permit).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(range, &format!("Use `{prefer}` instead."), None);
    // Remove `.require(:u)`: end of the permit call through end of require.
    cx.emit_edit(
        Range {
            start: cx.range(permit).end,
            end: cx.range(node).end,
        },
        "",
    );
    cx.emit_edit(cx.selector(permit), "expect");
}

/// Upstream node equality for the require/permit key match.
fn same_key(cx: &Cx<'_>, require_arg: NodeId, pair_key: NodeId) -> bool {
    match (*cx.kind(require_arg), *cx.kind(pair_key)) {
        (NodeKind::Sym(a), NodeKind::Sym(b)) => cx.symbol_str(a) == cx.symbol_str(b),
        (NodeKind::Str(a), NodeKind::Str(b)) => cx.string_str(a) == cx.string_str(b),
        _ => cx.raw_source(cx.range(require_arg)) == cx.raw_source(cx.range(pair_key)),
    }
}

/// Upstream `require_key`: `value`-like args use `name: `, others `src => `.
fn require_key(cx: &Cx<'_>, arg: NodeId) -> String {
    match *cx.kind(arg) {
        NodeKind::Sym(sym) => format!("{}: ", cx.symbol_str(sym)),
        NodeKind::Str(id) => format!("{}: ", cx.string_str(id)),
        NodeKind::Int(_) => format!("{}: ", cx.raw_source(cx.range(arg))),
        _ => format!("{} => ", cx.raw_source(cx.range(arg))),
    }
}

#[cfg(test)]
mod tests {
    use super::StrongParametersExpect;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bracket_with_method_call() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                params[:key].do_something
                      ^^^^^^ Use `expect(:key)` instead.
            "#},
            "params.expect(:key).do_something
",
        );
    }

    #[test]
    fn allows_bare_bracket() {
        test::<StrongParametersExpect>().expect_no_offenses("params[:key]
");
    }

    #[test]
    fn allows_presence_checks() {
        for src in [
            "params[:key].nil?
",
            "params[:key].blank?
",
            "params[:key].present?
",
            "params[:key].presence || default
",
        ] {
            test::<StrongParametersExpect>().expect_no_offenses(src);
        }
    }

    #[test]
    fn allows_comparison() {
        test::<StrongParametersExpect>().expect_no_offenses("params[:key] == 'value'
");
    }

    #[test]
    fn flags_find_with_bracket() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                Model.find(params[:id])
                                 ^^^^^ Use `expect(:id)` instead.
            "#},
            "Model.find(params.expect(:id))
",
        );
    }

    #[test]
    fn flags_find_bang_with_bracket() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                Model.find_by!(key: params[:key])
                                          ^^^^^^ Use `expect(:key)` instead.
            "#},
            "Model.find_by!(key: params.expect(:key))
",
        );
    }

    #[test]
    fn allows_plain_find_by() {
        test::<StrongParametersExpect>().expect_no_offenses("Model.find_by(key: params[:key])
");
    }

    #[test]
    fn allows_bracket_with_fallback() {
        test::<StrongParametersExpect>().expect_no_offenses("Model.find(params[:id] || DEFAULT_VALUE)
");
    }

    #[test]
    fn allows_chained_bracket() {
        test::<StrongParametersExpect>().expect_no_offenses("params[:user][:name]
");
    }

    #[test]
    fn flags_require_permit() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                params.require(:user).permit(:name, :age)
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `expect(user: [:name, :age])` instead.
            "#},
            "params.expect(user: [:name, :age])
",
        );
    }

    #[test]
    fn flags_permit_require() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                params.permit(user: [:name, :age]).require(:user)
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `expect(user: [:name, :age])` instead.
            "#},
            "params.expect(user: [:name, :age])
",
        );
    }

    #[test]
    fn flags_csend_require_permit() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                params&.require(:user)&.permit(:name, :age)
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `expect(user: [:name, :age])` instead.
            "#},
            "params&.expect(user: [:name, :age])
",
        );
    }

    #[test]
    fn flags_variable_require_key() {
        test::<StrongParametersExpect>().expect_correction(
            indoc! {r#"
                var = :user
                params.require(var).permit(:name, some_ids: [])
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `expect(var => [:name, some_ids: []])` instead.
            "#},
            indoc! {r#"
                var = :user
                params.expect(var => [:name, some_ids: []])
            "#},
        );
    }

    #[test]
    fn allows_lone_require_and_permit() {
        test::<StrongParametersExpect>().expect_no_offenses("params.require(:name)
");
        test::<StrongParametersExpect>().expect_no_offenses("params.permit(:name)
");
        test::<StrongParametersExpect>().expect_no_offenses("params.require(:target).permit
");
    }

    #[test]
    fn allows_mismatched_permit_require() {
        test::<StrongParametersExpect>().expect_no_offenses(
            "params.permit(unmatch_require_param: [:name, :age]).require(:user)
",
        );
    }

    #[test]
    fn allows_non_params_bracket() {
        test::<StrongParametersExpect>().expect_no_offenses("Model.find(non_params[:key])
");
    }

    #[test]
    fn gates_on_rails_version() {
        test::<StrongParametersExpect>()
            .with_target_rails_version(7, 2)
            .expect_no_offenses("params.require(:user).permit(:name, :age)
");
    }
}
murphy_plugin_api::submit_cop!(StrongParametersExpect);
