//! `Lint/StructNewOverride` — checks `Struct.new` members that override
//! built-in `Struct` methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/StructNewOverride
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's `Struct.new` send coverage for Struct/::Struct,
//!   symbol and string member names, optional class-name first argument,
//!   keyword options, block form, and the Ruby 4.0 Struct method-name list.
//!   No autocorrect.
//! ```

use murphy_plugin_api::{cop, def_node_matcher, Cx, NoOptions, NodeId, NodeKind};

// RuboCop parity: RuboCop's `Lint/StructNewOverride` matcher `struct_new` is
// `(send (const {nil? cbase} :Struct) :new ...)`. In Murphy's AST `::Struct`
// collapses to `Const{scope:None}`, so a single `nil?` scope covers bare and
// `::`-prefixed forms — equivalent to the prior `receiver present &&
// is_global_const(receiver, "Struct")` check.
def_node_matcher!(struct_new, "(send (const nil? :Struct) :new ...)");

const MSG_PREFIX: &str = "member overrides";

const STRUCT_METHOD_NAMES: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "[]", "[]=", "__id__", "__send__", "all?", "any?",
    "chain", "chunk", "chunk_while", "class", "clone", "collect", "collect_concat", "compact", "count", "cycle",
    "deconstruct", "deconstruct_keys", "define_singleton_method", "detect", "dig", "display", "drop", "drop_while",
    "dup", "each", "each_cons", "each_entry", "each_pair", "each_slice", "each_with_index", "each_with_object",
    "entries", "enum_for", "eql?", "equal?", "extend", "filter", "filter_map", "find", "find_all", "find_index",
    "first", "flat_map", "freeze", "frozen?", "grep", "grep_v", "group_by", "hash", "include?", "inject",
    "inspect", "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?", "instance_variable_get",
    "instance_variable_set", "instance_variables", "is_a?", "itself", "kind_of?", "lazy", "length", "map", "max",
    "max_by", "member?", "members", "method", "methods", "min", "min_by", "minmax", "minmax_by", "nil?",
    "none?", "object_id", "one?", "partition", "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "reduce", "reject", "remove_instance_variable", "respond_to?", "reverse_each", "select", "send",
    "singleton_class", "singleton_method", "singleton_methods", "size", "slice_after", "slice_before", "slice_when", "sort",
    "sort_by", "sum", "take", "take_while", "tally", "tap", "then", "to_a", "to_enum", "to_h", "to_s", "to_set",
    "uniq", "values", "values_at", "yield_self", "zip",
];

#[derive(Default)]
pub struct StructNewOverride;

#[cop(
    name = "Lint/StructNewOverride",
    description = "Checks Struct.new members that override built-in methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl StructNewOverride {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send (const nil? :Struct) :new ...)` — top-level `Struct.new(...)`.
        if !struct_new(node, cx) {
            return;
        }

        for (index, &arg) in cx.call_arguments(node).iter().enumerate() {
            if index == 0 && matches!(*cx.kind(arg), NodeKind::Str(_)) {
                continue;
            }
            let Some(member_name) = member_name(arg, cx) else {
                continue;
            };
            if STRUCT_METHOD_NAMES.contains(&member_name) {
                let member_source = cx.raw_source(cx.range(arg));
                let message = format!(
                    "`{member_source}` {MSG_PREFIX} `Struct#{member_name}` and it may be unexpected."
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
    use super::StructNewOverride;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_struct_members_override() {
        test::<StructNewOverride>().expect_offense(indoc! {r#"
            Bad = Struct.new(:members)
                             ^^^^^^^^ `:members` member overrides `Struct#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_cbase_and_string_member_overrides() {
        test::<StructNewOverride>().expect_offense(indoc! {r#"
            Bad = ::Struct.new(:members)
                               ^^^^^^^^ `:members` member overrides `Struct#members` and it may be unexpected.
            Bad = Struct.new(:name, "members")
                                    ^^^^^^^^^ `"members"` member overrides `Struct#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_class_name_and_options_forms() {
        test::<StructNewOverride>().expect_offense(indoc! {r#"
            Struct.new('Bad', :members, :name)
                              ^^^^^^^^ `:members` member overrides `Struct#members` and it may be unexpected.
            Struct.new(:members, keyword_init: true)
                       ^^^^^^^^ `:members` member overrides `Struct#members` and it may be unexpected.
        "#});
    }

    #[test]
    fn flags_multiple_overrides() {
        test::<StructNewOverride>().expect_offense(indoc! {r#"
            Struct.new(:members, :clone, :zip)
                       ^^^^^^^^ `:members` member overrides `Struct#members` and it may be unexpected.
                                 ^^^^^^ `:clone` member overrides `Struct#clone` and it may be unexpected.
                                         ^^^^ `:zip` member overrides `Struct#zip` and it may be unexpected.
        "#});
    }

    #[test]
    fn accepts_non_overrides_and_block_method_definitions() {
        test::<StructNewOverride>().expect_no_offenses(indoc! {r#"
            Good = Struct.new(:id, :name)
            Good = Struct.new(:id, :name) do
              def members
                super
              end
            end
        "#});
    }

    #[test]
    fn accepts_namespaced_struct_new() {
        // `(const nil? :Struct)` matches only top-level `Struct`; a namespaced
        // `Foo::Struct.new(:members)` has a non-nil scope and is not flagged.
        test::<StructNewOverride>().expect_no_offenses("Bad = Foo::Struct.new(:members)\n");
    }
}

murphy_plugin_api::submit_cop!(StructNewOverride);
