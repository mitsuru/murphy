//! `Rails/ActionControllerFlashBeforeRender` — use `flash.now` before `render`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActionControllerFlashBeforeRender
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! safe: false
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND [:flash]:
//!   `flash[k] = v` (bare `flash` receiver of `[]=`) followed by a `render`
//!   (explicit sibling/descendant search, or implicit trailing render when
//!   no siblings and no `redirect_to`). Requires an enclosing instance
//!   `def`/`block` and an ancestor `class` whose superclass is
//!   `ApplicationController` or `ActionController::Base`. `redirect_to`
//!   siblings (including `return redirect_to`) suppress. Offense on the
//!   inner `flash`; autocorrect to `flash.now`. Disabled upstream by
//!   default (`Enabled: pending`, `SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActionControllerFlashBeforeRender;

const MSG: &str = "Use `flash.now` before `render`.";

#[cop(
    name = "Rails/ActionControllerFlashBeforeRender",
    description = "Use `flash.now` instead of `flash` before `render`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ActionControllerFlashBeforeRender {
    #[on_node(kind = "send", methods = ["flash"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(flash_node: NodeId, cx: &Cx<'_>) {
    // Must be bare `flash` (no receiver).
    if cx.call_receiver(flash_node).get().is_some() {
        return;
    }
    if cx.method_name(flash_node) != Some("flash") {
        return;
    }
    // Must be the receiver of `[]=` — i.e. `flash[k] = v`.
    let Some(assign) = cx.parent(flash_node).get() else {
        return;
    };
    if !matches!(*cx.kind(assign), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(assign) != Some("[]=") {
        return;
    }
    if cx.call_receiver(assign).get() != Some(flash_node) {
        return;
    }
    if !followed_by_render(cx, assign) {
        return;
    }
    if !in_instance_method_or_block(cx, flash_node) {
        return;
    }
    if !inherits_action_controller(cx, flash_node) {
        return;
    }
    cx.emit_offense(cx.range(flash_node), MSG, None);
    cx.emit_edit(cx.range(flash_node), "flash.now");
}

fn followed_by_render(cx: &Cx<'_>, assign: NodeId) -> bool {
    // Nearest if/rescue ancestor of the assignment.
    if let Some(flow) = cx
        .ancestors(assign)
        .find(|&a| matches!(*cx.kind(a), NodeKind::If { .. } | NodeKind::Rescue { .. }))
    {
        if uses_redirect(cx, assign) {
            return false;
        }
        return right_siblings(cx, flow).iter().any(|&s| contains_render(cx, s));
    }
    let sibs = right_siblings(cx, assign);
    if sibs.is_empty() {
        // Implicit render (trailing flash renders the template) unless a
        // `redirect_to` sits beside the parent.
        if let Some(parent) = cx.parent(assign).get() {
            if uses_redirect_in_parent(cx, parent, assign) {
                return false;
            }
            // Upstream: `elsif context.right_siblings.empty? &&
            // !use_redirect_to?(context.parent); return true`.
            // `use_redirect_to?(parent)` checks parent's right siblings.
            // Our `uses_redirect_in_parent` covers the case where parent is
            // a statement container; when parent itself has no siblings we
            // treat it as implicit render (matches upstream spec for a
            // trailing flash in a def).
            return true;
        }
        return true;
    }
    sibs.iter().any(|&s| contains_render(cx, s))
}

/// Right siblings of `node` within its parent statement list.
fn right_siblings(cx: &Cx<'_>, node: NodeId) -> Vec<NodeId> {
    let Some(parent) = cx.parent(node).get() else {
        return Vec::new();
    };
    // Parent statement list: Begin children, Def body, Block body, etc.
    // Fall back to generic children order.
    let kids = cx.children(parent);
    if let Some(pos) = kids.iter().position(|&k| k == node) {
        kids[pos + 1..].to_vec()
    } else {
        Vec::new()
    }
}

/// Direct `redirect_to` among the right siblings of `node`
/// (unwrapping `return redirect_to ...`).
fn uses_redirect(cx: &Cx<'_>, node: NodeId) -> bool {
    right_siblings(cx, node).iter().any(|&s| is_redirect(cx, s))
}

/// `use_redirect_to?(parent)` for the implicit-render branch: whether the
/// parent's right siblings (when parent is itself a statement) contain a
/// redirect. When parent is a Def/Block/Class/Begin body container we check
/// the assignment's own siblings (already empty here), so this is the
/// `context.parent` check from upstream.
fn uses_redirect_in_parent(cx: &Cx<'_>, parent: NodeId, assign: NodeId) -> bool {
    // If parent is a statement list container (Begin/Def/Block), the
    // assignment's siblings were already empty; check parent's own siblings
    // (e.g. flash inside an `if` body whose `if` is followed by redirect).
    // Walk one level up: parent's right siblings.
    let parent_sibs = right_siblings(cx, parent);
    if parent_sibs.iter().any(|&s| is_redirect(cx, s)) {
        return true;
    }
    // Also check remaining children of parent after assign when parent is
    // a Begin-like container (defensive; siblings already empty).
    let kids = cx.children(parent);
    if let Some(pos) = kids.iter().position(|&k| k == assign) {
        return kids[pos + 1..].iter().any(|&s| is_redirect(cx, s));
    }
    false
}

fn is_redirect(cx: &Cx<'_>, node: NodeId) -> bool {
    // Unwrap `return redirect_to ...`.
    let mut target = node;
    if let NodeKind::Return(inner) = *cx.kind(node) {
        if let Some(id) = inner.get() {
            target = id;
        } else {
            return false;
        }
    }
    if !matches!(*cx.kind(target), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    cx.method_name(target) == Some("redirect_to")
}

/// Whether `node` is or contains a bare `render` call.
fn contains_render(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut stack = vec![node];
    stack.extend(cx.descendants(node));
    for id in stack {
        if !matches!(*cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        if cx.method_name(id) != Some("render") {
            continue;
        }
        if cx.call_receiver(id).get().is_some() {
            continue;
        }
        return true;
    }
    false
}

fn in_instance_method_or_block(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.ancestors(node).any(|a| {
        matches!(
            *cx.kind(a),
            NodeKind::Def { .. }
                | NodeKind::Block { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. }
        )
    })
}

fn inherits_action_controller(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Class { superclass, .. } = *cx.kind(anc) {
            let Some(sup) = superclass.get() else {
                return false;
            };
            if let Some(name) = cx.const_name(sup) {
                return name == "ApplicationController" || name == "ActionController::Base";
            }
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ActionControllerFlashBeforeRender;
    use murphy_plugin_api::test_support::{indoc, test};

    fn klass(body: &str) -> String {
        format!("class HomeController < ApplicationController\n  def create\n{body}  end\nend\n")
    }

    fn check_no(body: &str) {
        let src = klass(body);
        test::<ActionControllerFlashBeforeRender>().expect_no_offenses(&src);
    }

    #[test]
    fn flags_flash_before_render() {
        test::<ActionControllerFlashBeforeRender>().expect_correction(
            indoc! {r#"
                class HomeController < ApplicationController
                  def create
                    flash[:alert] = "msg"
                    ^^^^^ Use `flash.now` before `render`.
                    render :index
                  end
                end
            "#},
            "class HomeController < ApplicationController\n  def create\n    flash.now[:alert] = \"msg\"\n    render :index\n  end\nend\n",
        );
    }

    #[test]
    fn flags_trailing_flash_implicit_render() {
        test::<ActionControllerFlashBeforeRender>().expect_offense(indoc! {r#"
            class HomeController < ApplicationController
              def create
                flash[:alert] = "msg"
                ^^^^^ Use `flash.now` before `render`.
              end
            end
        "#});
    }

    #[test]
    fn allows_flash_now() {
        check_no(
            "    flash.now[:alert] = \"msg\"\n    render :index\n",
        );
    }

    #[test]
    fn allows_redirect_instead_of_render() {
        check_no(
            "    flash[:alert] = \"msg\"\n    redirect_to root_path\n",
        );
    }

    #[test]
    fn allows_return_redirect() {
        check_no(
            "    flash[:alert] = \"msg\"\n    return redirect_to root_path\n",
        );
    }

    #[test]
    fn allows_non_controller_class() {
        test::<ActionControllerFlashBeforeRender>().expect_no_offenses(indoc! {r#"
            class Foo
              def create
                flash[:alert] = "msg"
                render :index
              end
            end
        "#});
    }

    #[test]
    fn allows_action_controller_base() {
        test::<ActionControllerFlashBeforeRender>().expect_correction(
            indoc! {r#"
                class HomeController < ActionController::Base
                  def create
                    flash[:alert] = "msg"
                    ^^^^^ Use `flash.now` before `render`.
                    render :index
                  end
                end
            "#},
            "class HomeController < ActionController::Base\n  def create\n    flash.now[:alert] = \"msg\"\n    render :index\n  end\nend\n",
        );
    }

    #[test]
    fn allows_top_level() {
        test::<ActionControllerFlashBeforeRender>().expect_no_offenses(
            "flash[:alert] = \"msg\"\nrender :index\n",
        );
    }

    #[test]
    fn flags_flash_in_block() {
        test::<ActionControllerFlashBeforeRender>().expect_offense(indoc! {r#"
            class HomeController < ApplicationController
              def create
                do_something do
                  flash[:alert] = "msg"
                  ^^^^^ Use `flash.now` before `render`.
                  render :index
                end
              end
            end
        "#});
    }

    #[test]
    fn allows_flash_without_render() {
        check_no(
            "    flash[:alert] = \"msg\"\n    do_something\n",
        );
    }
}

murphy_plugin_api::submit_cop!(ActionControllerFlashBeforeRender);
