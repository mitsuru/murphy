//! `Lint/IneffectiveAccessModifier` — checks ineffective access modifiers.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/IneffectiveAccessModifier
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covers class/module bodies where bare `private` or `protected` precedes a
//!   singleton method definition (`def self.foo` / `def Foo.foo`) and ignores
//!   methods listed by bare `private_class_method :foo`. Nested `begin` bodies
//!   are scanned, while singleton-class bodies (`class << self`) are separate
//!   scopes and intentionally ignored, matching RuboCop.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, OptNodeId};

const ALTERNATIVE_PRIVATE: &str = "`private_class_method` or `private` inside a `class << self` block";
const ALTERNATIVE_PROTECTED: &str = "`protected` inside a `class << self` block";

#[derive(Default)]
pub struct IneffectiveAccessModifier;

#[cop(
    name = "Lint/IneffectiveAccessModifier",
    description = "Checks ineffective access modifiers before singleton methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IneffectiveAccessModifier {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_scope(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check_scope(node, cx);
    }
}

fn check_scope(node: NodeId, cx: &Cx<'_>) {
    let children = body_children(node, cx);
    if children.is_empty() {
        return;
    }
    let ignored = private_class_method_names(&children, cx);
    check_children(&children, None, &ignored, cx);
}

fn check_children(
    children: &[NodeId],
    mut modifier: Option<NodeId>,
    ignored: &[String],
    cx: &Cx<'_>,
) -> Option<NodeId> {
    for &child in children {
        if let Some(name) = bare_modifier(child, cx) {
            modifier = (name != "public").then_some(child);
            continue;
        }
        if matches!(cx.kind(child), NodeKind::Sclass { .. }) {
            continue;
        }
        if matches!(cx.kind(child), NodeKind::Begin(_) | NodeKind::Kwbegin(_)) {
            modifier = check_children(&body_children(child, cx), modifier, ignored, cx);
            continue;
        }
        if is_singleton_def(child, cx) {
            let Some(modifier_node) = modifier else {
                continue;
            };
            if ignored.iter().any(|name| name == &singleton_def_name(child, cx)) {
                continue;
            }
            let modifier_name = bare_modifier(modifier_node, cx).unwrap_or("private");
            let alternative = if modifier_name == "private" {
                ALTERNATIVE_PRIVATE
            } else {
                ALTERNATIVE_PROTECTED
            };
            let line = line_number(cx.range(modifier_node).start, cx.source());
            let message = format!(
                "`{modifier_name}` (on line {line}) does not make singleton methods {modifier_name}. Use {alternative} instead."
            );
            cx.emit_offense(cx.loc(child).keyword(), &message, None);
        }
    }
    modifier
}

fn body_children(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match *cx.kind(node) {
        NodeKind::Class { body, .. } | NodeKind::Module { body, .. } | NodeKind::Sclass { body, .. } => {
            body.get().map_or_else(Vec::new, |body| body_children(body, cx))
        }
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(list).to_vec(),
        _ => vec![node],
    }
}

fn bare_modifier(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };
    if receiver != OptNodeId::NONE || !cx.list(args).is_empty() {
        return None;
    }
    match cx.symbol_str(method) {
        "private" => Some("private"),
        "protected" => Some("protected"),
        "public" => Some("public"),
        _ => None,
    }
}

fn private_class_method_names(children: &[NodeId], cx: &Cx<'_>) -> Vec<String> {
    children
        .iter()
        .copied()
        .filter_map(|child| match *cx.kind(child) {
            NodeKind::Send { receiver, method, args }
                if receiver == OptNodeId::NONE && cx.symbol_str(method) == "private_class_method" =>
            {
                Some(cx.list(args))
            }
            _ => None,
        })
        .flatten()
        .filter_map(|&arg| match *cx.kind(arg) {
            NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_string()),
            _ => None,
        })
        .collect()
}

fn is_singleton_def(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Def { receiver, .. } if receiver != OptNodeId::NONE)
        || matches!(cx.kind(node), NodeKind::Defs { .. })
}

fn singleton_def_name(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => cx.symbol_str(name).to_string(),
        _ => String::new(),
    }
}

fn line_number(offset: u32, source: &str) -> usize {
    source[..offset as usize].bytes().filter(|&b| b == b'\n').count() + 1
}

murphy_plugin_api::submit_cop!(IneffectiveAccessModifier);

#[cfg(test)]
mod tests {
    use super::IneffectiveAccessModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_private_before_singleton_method() {
        test::<IneffectiveAccessModifier>().expect_offense(indoc! {r#"
            class C
              private

              def self.method
              ^^^ `private` (on line 2) does not make singleton methods private. Use `private_class_method` or `private` inside a `class << self` block instead.
              end
            end
        "#});
    }

    #[test]
    fn accepts_private_class_method_and_sclass() {
        test::<IneffectiveAccessModifier>()
            .expect_no_offenses(indoc! {r#"
                class C
                  private
                  def self.method
                  end
                  private_class_method :method
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                class C
                  private
                  class << self
                    def method
                    end
                  end
                end
            "#});
    }

    #[test]
    fn begin_does_not_create_visibility_scope() {
        test::<IneffectiveAccessModifier>()
            .expect_no_offenses(indoc! {r#"
                class C
                  private
                  begin
                    public
                  end

                  def self.method
                  end
                end
            "#})
            .expect_offense(indoc! {r#"
                class C
                  public
                  begin
                    private
                  end

                  def self.method
                  ^^^ `private` (on line 4) does not make singleton methods private. Use `private_class_method` or `private` inside a `class << self` block instead.
                  end
                end
            "#});
    }
}
