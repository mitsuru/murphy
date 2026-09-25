//! `Rails/FindByOrAssignmentMemoization` — avoid memoizing `find_by` with `||=`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FindByOrAssignmentMemoization
//! upstream_version_checked: 2.35.0
//! version_added: "2.33"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: OrAsgn dispatch (ivasgn target with nil
//!   value, Send/Csend `find_by` value), initialize-assignment exemption,
//!   if-ancestor exemption, and both corrections (def-body-only succinct
//!   form with endless-def handling; general if/else form otherwise).
//!   Upstream ships `Enabled: pending`; Murphy maps that to
//!   `default_enabled = false`.
//! ```
//!
//! Avoid memoizing `find_by` results with `||=`: `find_by` may return
//! `nil`, in which case the memoization silently re-queries on every call.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FindByOrAssignmentMemoization;

const MSG: &str = "Avoid memoizing `find_by` results with `||=`.";

#[cop(
    name = "Rails/FindByOrAssignmentMemoization",
    description = "Avoid memoizing `find_by` results with `||=`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl FindByOrAssignmentMemoization {
    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::OrAsgn { target, value } = *cx.kind(node) else {
        return;
    };
    // Target must be a bare ivar write (`@x` with no value).
    let NodeKind::Ivasgn { name, value: target_value } = *cx.kind(target) else {
        return;
    };
    if target_value.get().is_some() {
        return;
    }
    // Value must be a `find_by` call (Send or Csend, any args).
    if !matches!(*cx.kind(value), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(value) != Some("find_by") {
        return;
    }
    let var_name = cx.symbol_str(name).to_owned();
    // Exemption: ivar assigned in `initialize` (Ruby 3.2 object-shape idiom).
    if instance_variable_assigned(cx, &var_name) {
        return;
    }
    // Exemption: inside a conditional (mirrors upstream if-ancestor gate).
    if cx
        .ancestors(node)
        .any(|a| matches!(*cx.kind(a), NodeKind::If { .. }))
    {
        return;
    }
    let find_by_src = cx.raw_source(cx.range(value)).to_owned();
    let indent = line_indent(cx, node);
    // Def-body-only shape gets the succinct correction.
    if let Some(def_node) = cx
        .parent(node)
        .get()
        .filter(|p| matches!(*cx.kind(*p), NodeKind::Def { .. }))
        && def_body_is(cx, def_node, node)
    {
        cx.emit_offense(cx.range(node), MSG, None);
        if is_endless_def(cx, def_node, node) {
            // `def foo = @x ||= ...` → regular def with succinct body.
            let assign_range = endless_assign_range(cx, def_node, node);
            let body_replacement = format!(
                "\n  return {var_name} if defined?({var_name})\n\n  {var_name} = {find_by_src}\n"
            );
            if let Some(ar) = assign_range {
                cx.emit_edit(ar, "\n");
            }
            cx.emit_edit(cx.range(node), body_replacement.trim());
            cx.emit_edit(
                Range {
                    start: cx.range(def_node).end,
                    end: cx.range(def_node).end,
                },
                "\nend",
            );
        } else {
            let replacement = format!(
                "return {var_name} if defined?({var_name})\n\n{indent}{var_name} = {find_by_src}"
            );
            cx.emit_edit(cx.range(node), &replacement);
        }
        return;
    }
    cx.emit_offense(cx.range(node), MSG, None);
    let replacement = format!(
        "if defined?({var_name})\n{indent}  {var_name}\n{indent}else\n{indent}  {var_name} = {find_by_src}\n{indent}end"
    );
    cx.emit_edit(cx.range(node), &replacement);
}

/// True when `def_node`'s body is exactly `or_asgn`.
fn def_body_is(cx: &Cx<'_>, def_node: NodeId, or_asgn: NodeId) -> bool {
    let NodeKind::Def { body, .. } = *cx.kind(def_node) else {
        return false;
    };
    body.get() == Some(or_asgn)
}

/// Heuristic endless-def detection: the def source does not end with `end`
/// but contains `=` between the argument list and the body.
fn is_endless_def(cx: &Cx<'_>, def_node: NodeId, body: NodeId) -> bool {
    let src = cx.raw_source(cx.range(def_node));
    if src.trim_end().ends_with("end") {
        return false;
    }
    let body_start = cx.range(body).start - cx.range(def_node).start;
    let prefix = &src[..body_start as usize];
    prefix.contains('=')
}

/// Range from the endless `=` to the body start (for the `\n` replacement).
fn endless_assign_range(cx: &Cx<'_>, def_node: NodeId, body: NodeId) -> Option<Range> {
    let def_range = cx.range(def_node);
    let src = cx.raw_source(def_range);
    let body_off = (cx.range(body).start - def_range.start) as usize;
    let prefix = &src[..body_off];
    let eq = prefix.rfind('=')?;
    Some(Range {
        start: def_range.start + eq as u32,
        end: cx.range(body).start,
    })
}

/// True when some `def initialize` in the file assigns the same ivar.
fn instance_variable_assigned(cx: &Cx<'_>, var_name: &str) -> bool {
    for id in cx.descendants(cx.root()) {
        let NodeKind::Def { name, body, .. } = *cx.kind(id) else {
            continue;
        };
        if cx.symbol_str(name) != "initialize" {
            continue;
        }
        let Some(b) = body.get() else {
            continue;
        };
        if let NodeKind::Ivasgn { name: n, .. } = *cx.kind(b)
            && cx.symbol_str(n) == var_name
        {
            return true;
        }
        for sub in cx.descendants(b) {
            if let NodeKind::Ivasgn { name: n, .. } = *cx.kind(sub)
                && cx.symbol_str(n) == var_name
            {
                return true;
            }
        }
    }
    false
}

/// Whitespace indent of the line containing `node` (for correction layout).
fn line_indent(cx: &Cx<'_>, node: NodeId) -> String {
    let src = cx.source();
    let start = cx.range(node).start as usize;
    let line_start = src[..start].rfind("\n").map_or(0, |i| i + 1);
    src[line_start..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::FindByOrAssignmentMemoization;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_def_body_only() {
        test::<FindByOrAssignmentMemoization>().expect_offense(indoc! {r#"
            def current_user
              @current_user ||= User.find_by(id: session[:user_id])
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid memoizing `find_by` results with `||=`.
            end
        "#});
    }

    #[test]
    fn autocorrects_def_body_only() {
        test::<FindByOrAssignmentMemoization>().expect_correction(
            indoc! {r#"
                def current_user
                  @current_user ||= User.find_by(id: session[:user_id])
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid memoizing `find_by` results with `||=`.
                end
            "#},
            "def current_user\n  return @current_user if defined?(@current_user)\n\n  @current_user = User.find_by(id: session[:user_id])\nend\n",
        );
    }

    #[test]
    fn autocorrects_method_with_other_code() {
        test::<FindByOrAssignmentMemoization>().expect_correction(
            indoc! {r#"
                def current_user
                  @current_user ||= User.find_by(id: session[:user_id])
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid memoizing `find_by` results with `||=`.
                  @current_user.do_something
                end
            "#},
            "def current_user\n  if defined?(@current_user)\n    @current_user\n  else\n    @current_user = User.find_by(id: session[:user_id])\n  end\n  @current_user.do_something\nend\n",
        );
    }

    #[test]
    fn does_not_flag_initialized_ivar() {
        test::<FindByOrAssignmentMemoization>().expect_no_offenses(indoc! {r#"
            class Foo
              def initialize
                @current_user = nil
              end

              def current_user
                @current_user ||= User.find_by(id: 1)
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_non_find_by_memoization() {
        test::<FindByOrAssignmentMemoization>().expect_no_offenses(indoc! {r#"
            def current_user
              @current_user ||= User.where(id: 1).first
            end
        "#});
    }

    #[test]
    fn does_not_flag_lvar_memoization() {
        test::<FindByOrAssignmentMemoization>().expect_no_offenses(indoc! {r#"
            def current_user
              x ||= User.find_by(id: 1)
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(FindByOrAssignmentMemoization);
