//! `Rails/ActionOrder` — enforce consistent ordering of controller actions.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActionOrder
//! upstream_version_checked: 2.35.0
//! version_added: "2.17"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` with configurable `ExpectedOrder`
//!   (default `index show new edit create update destroy`): consecutive
//!   action `def`s in source order flag when the later action sorts before
//!   the earlier one ("Action `index` should appear before `show`").
//!   Non-listed methods are ignored; duplicate actions never
//!   flag (`>=`); non-public actions are skipped (inline `private def` /
//!   `protected def` modifiers and bare `private` / `protected` sections,
//!   mirroring `VisibilityHelp`). Autocorrect swaps the whole-line ranges
//!   (with own-line comments) of the two actions, where an action wrapped in
//!   `if` / `unless` moves with its nearest `if` ancestor
//!   (`correction_target`). Upstream loops corrections to a fixpoint; Murphy
//!   applies one pass per run, so multi-offense files converge over repeated
//!   `--fix` runs. File scope (`Include: controllers`) is enforced via the
//!   murphy-rails pack default.yml.
//! ```
//!
//! ## Matched shapes
//!
//! - `class C < ApplicationController; def show; end; def index; end; end` →
//!   offense on `def index`, swapped before `def show`
//! - `def commit` (non-standard) between actions → ignored
//! - `private` section / `private def` actions → skipped
//!
//! ## Autocorrect
//!
//! Swap the two actions' whole-line ranges (comments travel along).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActionOrder;

#[derive(CopOptions)]
pub struct ActionOrderOptions {
    #[option(
        name = "ExpectedOrder",
        default = ["index", "show", "new", "edit", "create", "update", "destroy"],
        description = "The expected order of controller actions."
    )]
    pub expected_order: Vec<String>,
}

#[cop(
    name = "Rails/ActionOrder",
    description = "Enforce consistent ordering of controller actions.",
    default_enabled = false,
    options = ActionOrderOptions,
)]
impl ActionOrder {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(class_node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ActionOrderOptions>();
    let expected: Vec<String> = opts.expected_order.clone();
    // Upstream `action_declarations(node, actions)`: all descendant `def`s
    // whose name is in the expected set, in source order.
    let actions: Vec<NodeId> = cx
        .descendants(class_node)
        .into_iter()
        .filter(|&d| {
            if let NodeKind::Def { receiver, name, .. } = *cx.kind(d) {
                receiver.get().is_none() && expected.iter().any(|e| e == cx.symbol_str(name))
            } else {
                false
            }
        })
        .collect();
    let index_of = |node: NodeId| -> usize {
        let name = cx.method_name(node).unwrap_or_default();
        expected.iter().position(|e| e == name).unwrap_or(usize::MAX)
    };
    for pair in actions.windows(2) {
        let (previous, current) = (pair[0], pair[1]);
        // Upstream: `next if node_visibility(current) != :public || non_public?(current)`.
        if is_non_public(cx, current) {
            continue;
        }
        if index_of(current) >= index_of(previous) {
            continue;
        }
        let current_name = cx.method_name(current).unwrap_or_default().to_owned();
        let previous_name = cx.method_name(previous).unwrap_or_default().to_owned();
        let msg = format!("Action `{current_name}` should appear before `{previous_name}`.");
        cx.emit_offense(cx.range(current), &msg, None);
        // Upstream `correction_target`: nearest `if` ancestor, else the def.
        let current_target = correction_target(cx, current);
        let previous_target = correction_target(cx, previous);
        let current_range = cx.range_with_comments_and_lines(current_target);
        let previous_range = cx.range_with_comments_and_lines(previous_target);
        let current_src = cx.raw_source(current_range).to_owned();
        // `swap_range`: `insert_before(previous, current source)` as a
        // zero-width insert; the current range is removed below.
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: previous_range.start,
                end: previous_range.start,
            },
            &current_src,
        );
        cx.emit_edit(current_range, "");
    }
}

/// Upstream `correction_target`: `def_node.each_ancestor(:if).first || def_node`.
fn correction_target(cx: &Cx<'_>, def_node: NodeId) -> NodeId {
    for anc in cx.ancestors(def_node) {
        if matches!(*cx.kind(anc), NodeKind::If { .. }) {
            return anc;
        }
    }
    def_node
}

/// `private def` / `protected def` modifier plus bare-section visibility
/// (same pattern as the `Delegate` cop).
fn is_non_public(cx: &Cx<'_>, node: NodeId) -> bool {
    // `private def foo` — def is the argument of a modifier send.
    for anc in cx.ancestors(node) {
        if let NodeKind::Send { method, .. } = *cx.kind(anc) {
            let m = cx.symbol_str(method);
            if (m == "private" || m == "protected") && reaches_def(cx, anc, node) {
                return true;
            }
        }
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
        if matches!(*cx.kind(id), NodeKind::Send { .. }) {
            cur = cx.call_arguments(id).first().copied();
        } else {
            break;
        }
    }
    false
}

fn section_visibility(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    for anc in cx.ancestors(node) {
        let kids: Vec<NodeId> = match *cx.kind(anc) {
            NodeKind::Begin(list) => cx.list(list).to_vec(),
            NodeKind::Class { body, .. } | NodeKind::Module { body, .. } => match body.get() {
                Some(b) => match *cx.kind(b) {
                    NodeKind::Begin(list) => cx.list(list).to_vec(),
                    _ => vec![b],
                },
                None => vec![],
            },
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
                    && cx.call_arguments(kid).is_empty()
                {
                    vis = Some(m.to_owned());
                }
            }
        }
        return vis;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{ActionOrder, ActionOrderOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unconventional_order() {
        test::<ActionOrder>().expect_correction(
            indoc! {r#"
                class UserController < ApplicationController
                  def show; end
                  def index; end
                  ^^^^^^^^^^^^^^ Action `index` should appear before `show`.
                end
            "#},
            "class UserController < ApplicationController\n  def index; end\n  def show; end\nend\n",
        );
    }

    #[test]
    fn flags_multiple_out_of_order() {
        // Multi-offense files converge over repeated runs; detection is
        // exact in a single pass.
        test::<ActionOrder>().expect_offense(indoc! {r#"
            class UserController < ApplicationController
              def create; end
              def edit; end
              ^^^^^^^^^^^^^ Action `edit` should appear before `create`.
              def show; end
              ^^^^^^^^^^^^^ Action `show` should appear before `edit`.
            end
        "#});
    }

    #[test]
    fn ignores_non_standard_actions() {
        test::<ActionOrder>().expect_no_offenses(indoc! {r#"
            class UserController < ApplicationController
              def index; end
              def commit; end
              def show; end
            end
        "#});
    }

    #[test]
    fn ignores_protected_section() {
        test::<ActionOrder>().expect_no_offenses(indoc! {r#"
            class UserController < ApplicationController
              def show; end
              protected
              def index; end
            end
        "#});
    }

    #[test]
    fn ignores_inline_protected() {
        test::<ActionOrder>().expect_no_offenses(indoc! {r#"
            class UserController < ApplicationController
              def show; end
              protected def index; end
            end
        "#});
    }

    #[test]
    fn ignores_private_section() {
        test::<ActionOrder>().expect_no_offenses(indoc! {r#"
            class UserController < ApplicationController
              def show; end
              private
              def index; end
            end
        "#});
    }

    #[test]
    fn ignores_inline_private() {
        test::<ActionOrder>().expect_no_offenses(indoc! {r#"
            class UserController < ApplicationController
              def show; end
              private def index; end
            end
        "#});
    }

    #[test]
    fn flags_actions_in_conditions() {
        // Block-form `if`/`unless` defs are multi-line ranges (not
        // caret-annotatable); modifier-form exercises the same
        // `correction_target` (`if`-ancestor) path on single lines.
        test::<ActionOrder>().expect_offense(indoc! {r#"
            class TestController < BaseController
              def edit; end unless Rails.env.development?
              def index; end if Rails.env.development?
              ^^^^^^^^^^^^^^ Action `index` should appear before `edit`.
            end
        "#});
    }

    #[test]
    fn corrects_actions_in_conditions() {
        test::<ActionOrder>().expect_correction(
            indoc! {r#"
                class TestController < BaseController
                  def edit; end unless Rails.env.development?
                  def index; end if Rails.env.development?
                  ^^^^^^^^^^^^^^ Action `index` should appear before `edit`.
                end
            "#},
            "class TestController < BaseController\n  def index; end if Rails.env.development?\n  def edit; end unless Rails.env.development?\nend\n",
        );
    }

    #[test]
    fn enforces_custom_order() {
        test::<ActionOrder>()
            .with_options(&ActionOrderOptions {
                expected_order: ["show", "index", "new", "edit", "create", "update", "destroy"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            })
            .expect_correction(
                indoc! {r#"
                    class UserController < ApplicationController
                      def index; end
                      def show; end
                      ^^^^^^^^^^^^^ Action `show` should appear before `index`.
                    end
                "#},
                "class UserController < ApplicationController\n  def show; end\n  def index; end\nend\n",
            );
    }

    #[test]
    fn corrects_with_comments() {
        test::<ActionOrder>().expect_correction(
            indoc! {r#"
                class UserController < ApplicationController
                  # show
                  def show; end

                  # index
                  def index; end
                  ^^^^^^^^^^^^^^ Action `index` should appear before `show`.
                end
            "#},
            "class UserController < ApplicationController\n  # index\n  def index; end\n  # show\n  def show; end\n\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(ActionOrder);
