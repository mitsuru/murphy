//! `Lint/DataDefineOverride` — checks `Data.define` members that override
//! built-in `Data` methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DataDefineOverride
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's `Data.define` send coverage for Data/::Data, symbol and
//!   string member names, and the Ruby 4.0 `Data` instance-method-name list.
//!   Unlike `Struct.new`, `Data.define` takes no class-name first argument, so
//!   every argument (including a first-position string) is checked. No
//!   autocorrect (RuboCop has none either). The offense backtick-quotes the
//!   member via `raw_source` rather than RuboCop's `value.inspect`; these match
//!   for symbol members and double-quoted strings, and differ only in quote
//!   style for single-quoted string members (e.g. `'members'` vs `"members"`) —
//!   same precedent as `Lint/StructNewOverride`.
//! ```

use murphy_plugin_api::{cop, def_node_matcher, Cx, NoOptions, NodeId, NodeKind};

// RuboCop parity: RuboCop's matcher `data_define` is
// `(send (const {nil? cbase} :Data) :define ...)`. In Murphy's AST `::Data`
// collapses to `Const{scope:None}`, so a single `nil?` scope covers bare and
// `::`-prefixed forms, while a namespaced `Foo::Data` keeps a non-nil scope and
// is not matched — equivalent to RuboCop's `{nil? cbase}`.
def_node_matcher!(data_define, "(send (const nil? :Data) :define ...)");

const MSG_PREFIX: &str = "member overrides";

// `Data.define.instance_methods.sort` in Ruby 4.0.0 (RuboCop 1.87.0).
const DATA_METHOD_NAMES: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "__id__", "__send__", "class", "clone", "deconstruct",
    "deconstruct_keys", "define_singleton_method", "display", "dup", "enum_for", "eql?", "equal?",
    "extend", "freeze", "frozen?", "hash", "inspect", "instance_eval", "instance_exec",
    "instance_of?", "instance_variable_defined?", "instance_variable_get", "instance_variable_set",
    "instance_variables", "is_a?", "itself", "kind_of?", "members", "method", "methods", "nil?",
    "object_id", "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "remove_instance_variable", "respond_to?", "send", "singleton_class",
    "singleton_method", "singleton_methods", "tap", "then", "to_enum", "to_h", "to_s", "with",
    "yield_self",
];

#[derive(Default)]
pub struct DataDefineOverride;

#[cop(
    name = "Lint/DataDefineOverride",
    description = "Checks Data.define members that override built-in methods.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DataDefineOverride {
    #[on_node(kind = "send", methods = ["define"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send (const nil? :Data) :define ...)` — top-level `Data.define(...)`.
        if !data_define(node, cx) {
            return;
        }

        // Unlike `Struct.new`, `Data.define` has no class-name first argument:
        // RuboCop iterates every argument with no index exemption.
        for &arg in cx.call_arguments(node) {
            let Some(member_name) = member_name(arg, cx) else {
                continue;
            };
            if DATA_METHOD_NAMES.contains(&member_name) {
                let member_source = cx.raw_source(cx.range(arg));
                let message = format!(
                    "`{member_source}` {MSG_PREFIX} `Data#{member_name}` and it may be unexpected."
                );
                cx.emit_offense(cx.range(arg), &message, None);
            }
        }
    }
}

fn member_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
        NodeKind::Str(string) => Some(cx.string_str(string)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::DataDefineOverride;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_data_member_override() {
        test::<DataDefineOverride>().expect_offense(indoc! {r#"
            Bad = Data.define(:members)
                              ^^^^^^^^ `:members` member overrides `Data#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_cbase_and_string_member_overrides() {
        test::<DataDefineOverride>().expect_offense(indoc! {r#"
            Bad = ::Data.define(:members)
                                ^^^^^^^^ `:members` member overrides `Data#members` and it may be unexpected.
            Bad = Data.define(:name, "members")
                                     ^^^^^^^^^ `"members"` member overrides `Data#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_first_position_string_member() {
        // `Data.define` has no class-name argument, so a first-position string
        // member is still flagged (unlike `Struct.new`).
        test::<DataDefineOverride>().expect_offense(indoc! {r#"
            Data.define("members")
                        ^^^^^^^^^ `"members"` member overrides `Data#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_multiple_overrides() {
        test::<DataDefineOverride>().expect_offense(indoc! {r#"
            Data.define(:members, :clone, :with)
                        ^^^^^^^^ `:members` member overrides `Data#members` and it may be unexpected.
                                  ^^^^^^ `:clone` member overrides `Data#clone` and it may be unexpected.
                                          ^^^^^ `:with` member overrides `Data#with` and it may be unexpected.
        "#});
    }

    #[test]
    fn accepts_non_overrides_and_block_method_definitions() {
        test::<DataDefineOverride>().expect_no_offenses(indoc! {r#"
            Good = Data.define(:id, :name)
            Good = Data.define(:id, :name) do
              def members
                super
              end
            end
        "#});
    }

    #[test]
    fn accepts_namespaced_data_define() {
        // `(const nil? :Data)` matches only top-level `Data`; a namespaced
        // `Foo::Data.define(:members)` has a non-nil scope and is not flagged.
        test::<DataDefineOverride>().expect_no_offenses("Bad = Foo::Data.define(:members)\n");
    }

    #[test]
    fn accepts_enumerable_members_not_on_data() {
        // `Data` is not `Enumerable`; methods like `map`/`each`/`first` are NOT
        // in the Data method list and must not be flagged (discriminator vs the
        // `Struct.new` list).
        test::<DataDefineOverride>()
            .expect_no_offenses("Good = Data.define(:map, :each, :first, :size)\n");
    }
}

murphy_plugin_api::submit_cop!(DataDefineOverride);
