//! `Rails/TableNameAssignment` — do not use `self.table_name =`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/TableNameAssignment
//! upstream_version_checked: 2.35.0
//! version_added: "2.14"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` with `base_class?`
//!   (`(class (const ... :Base) ...)` — any class whose name const is
//!   `Base`, namespaced or not) and `find_set_table_name` (any
//!   `self.table_name = ...` setter inside the class body). Offense is
//!   the whole setter send; no autocorrect. Disabled upstream by
//!   default (`Enabled: false`). File scope (`Include:
//!   ['**/app/models/**/*.rb']`) is enforced via the murphy-rails pack
//!   default.yml (engine `cop_applies_to_file` gate).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TableNameAssignment;

#[cop(
    name = "Rails/TableNameAssignment",
    description = "Do not use `self.table_name =`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl TableNameAssignment {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class { name, .. } = *cx.kind(node) else {
        return;
    };
    // Upstream `base_class?`: `(class (const ... :Base) ...)` — STI base
    // classes named `Base` are ignored.
    if class_const_name(cx, name).as_deref() == Some("Base") {
        return;
    }
    for id in cx.descendants(node) {
        if is_table_name_setter(cx, id) {
            cx.emit_offense(cx.range(id), "Do not use `self.table_name =`.", None);
        }
    }
}

/// The rightmost const name of a (possibly namespaced) class name node.
fn class_const_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(name).to_owned()),
        NodeKind::Cbase => None,
        _ => {
            // Namespaced `A::B::Base`: the const node carries the full
            // path; `const_name` returns it dotted.
            cx.const_name(id)
                .and_then(|full| full.rsplit("::").next().map(|s| s.to_owned()))
        }
    }
}

/// Upstream `find_set_table_name`: `self.table_name = ...`.
fn is_table_name_setter(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("table_name=") {
        return false;
    }
    cx.is_self_receiver(id)
}

#[cfg(test)]
mod tests {
    use super::TableNameAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_assignment() {
        test::<TableNameAssignment>().expect_offense(indoc! {r#"
            class AModule::SomeModel < ApplicationRecord
              self.table_name = 'some_other_table_name'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `self.table_name =`.
            end
        "#});
    }

    #[test]
    fn flags_symbol_assignment() {
        test::<TableNameAssignment>().expect_offense(indoc! {r#"
            class AModule::SomeModel < ApplicationRecord
              self.table_name = :foo
              ^^^^^^^^^^^^^^^^^^^^^^ Do not use `self.table_name =`.
            end
        "#});
    }

    #[test]
    fn allows_class_without_assignment() {
        test::<TableNameAssignment>().expect_no_offenses(indoc! {r#"
            class AModule::SomeModel < ApplicationRecord
              has_many :other_thing
              has_one :parent_thing
            end
        "#});
    }

    #[test]
    fn allows_base_class_with_namespace() {
        test::<TableNameAssignment>().expect_no_offenses(indoc! {r#"
            class A::B::Base < ApplicationRecord
              has_many :other_thing

              self.table_name = 'special_table_name'
            end
        "#});
    }

    #[test]
    fn allows_base_class_in_modules() {
        test::<TableNameAssignment>().expect_no_offenses(indoc! {r#"
            module A
              module B
                class Base < ApplicationRecord
                  has_many :other_thing

                  self.table_name = 'special_table_name'
                end
              end
            end
        "#});
    }

    #[test]
    fn ignores_bare_table_name_call() {
        test::<TableNameAssignment>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              table_name
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(TableNameAssignment);
