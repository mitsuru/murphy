//! `Lint/ConstantResolution` — flag unqualified constant references, which can
//! resolve ambiguously depending on the enclosing lexical scope.
//!
//! Disabled by default (RuboCop `Enabled: false`): it fires on essentially
//! every bare constant, so it is only useful when narrowed via `Only`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ConstantResolution
//! upstream_version_checked: 1.86.2
//! version_added: "0.86"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `(const nil? #const_name?)` matcher with the
//!   `on_const` guard `node.parent&.defined_module || node.loc.nil?`. An
//!   unqualified constant is a `Const` with no scope; the cbase form `::Foo`
//!   is excluded by checking the node source does not start with `::` (Murphy
//!   drops the `cbase` scope node during translation, so `::Foo` and `Foo`
//!   share a `None` scope and must be distinguished by source). The
//!   `defined_module` guard skips a constant whose parent is a `class`/`module`
//!   definition or a `casgn` assigned `Class.new`/`Module.new`, mirroring
//!   rubocop-ast's `defined_module0`. `Only`/`Ignore` filter on the
//!   constant's short name. Disabled by default per upstream.
//! ```
//!
//! ## Matched shapes
//!
//! `Const { scope: None }` whose source does not begin with `::`, whose short
//! name passes the `Only`/`Ignore` filter, and whose parent is not a module
//! definition. The offense range is the constant node's own range — for
//! `Foo::Bar`, only the inner unqualified `Foo` fires.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Lint/ConstantResolution` `defined_module0` only treats
// `Class.new` / `Module.new` (with optional block) as module definitions
// (`(send (const {nil? cbase} {:Class :Module}) :new)`, send-only).
// Verbatim as `(send (const nil? {:Class :Module}) :new ...)`.
// In Murphy `::Class` / `::Module` collapse to `Const{scope:None}`: `nil?`
// covers bare + `::` (both skip the definition const, pinned by
// `boundary_skips_cbase_class_new_definition`). Namespaced `Foo::Class` still
// flags (pinned by `boundary_flags_namespaced_class_new_definition`).
// `send` covers `Send` only, matching `is_class_constructor` Send-only
// (csend still flags, pinned by `boundary_flags_csend_class_new_definition`).
// Block extraction (`Block`/`Numblock`/`Itblock`) + `parent_defines_module`
// walk + `Only`/`Ignore` filter stay hand-rolled below.
def_node_matcher!(
    is_class_or_module_new_call,
    "(send (const nil? {:Class :Module}) :new ...)"
);

#[derive(Default)]
pub struct ConstantResolution;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "Only",
        default = [],
        description = "Restrict the cop to only these constant names (empty = all)."
    )]
    pub only: Vec<String>,

    #[option(
        name = "Ignore",
        default = [],
        description = "Exclude these constant names from the check."
    )]
    pub ignore: Vec<String>,
}

#[cop(
    name = "Lint/ConstantResolution",
    description = "Checks that constants are fully qualified with `::`.",
    default_severity = "warning",
    default_enabled = false,
    options = Options
)]
impl ConstantResolution {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let NodeKind::Const { scope, name } = *cx.kind(node) else {
            return;
        };

        // `nil?` scope: bare constant. A scoped const (`Foo::Bar`) is the
        // qualified outer node; only its unqualified inner segment fires.
        if scope.get().is_some() {
            return;
        }

        // Exclude the cbase form `::Foo` — already fully qualified. Murphy drops
        // the `cbase` scope node, so distinguish by source prefix.
        if cx.raw_source(cx.range(node)).starts_with("::") {
            return;
        }

        // `#const_name?`: `Only`/`Ignore` filter on the short name.
        let short = cx.symbol_str(name);
        if !const_name_allowed(short, &opts) {
            return;
        }

        // `node.parent&.defined_module`: skip a constant that names a module
        // being defined (class/module def, or `Class.new`/`Module.new` casgn).
        if parent_defines_module(node, cx) {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "Fully qualify this constant to avoid possibly ambiguous resolution.",
            None,
        );
    }
}

/// RuboCop's `const_name?`: `(Only.empty? || Only.include?(name)) &&
/// !Ignore.include?(name)`.
fn const_name_allowed(name: &str, opts: &Options) -> bool {
    (opts.only.is_empty() || opts.only.iter().any(|n| n == name))
        && !opts.ignore.iter().any(|n| n == name)
}

/// Mirror of rubocop-ast `Node#defined_module` applied to the const's parent:
/// truthy when the parent is a `class`/`module` definition, or a `casgn` whose
/// value is a `Class.new`/`Module.new` constructor.
fn parent_defines_module(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Class { .. } | NodeKind::Module { .. } => true,
        NodeKind::Casgn { value, .. } => value
            .get()
            .is_some_and(|v| cx.is_class_constructor(v) && is_class_or_module_new(v, cx)),
        _ => false,
    }
}

/// `defined_module0` only treats `Class.new` / `Module.new` (with optional
/// block) as module definitions, not `Struct.new` / `Data.define`. Narrow the
/// broader `is_class_constructor` helper accordingly.
fn is_class_or_module_new(node: NodeId, cx: &Cx<'_>) -> bool {
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => node,
    };
    // `(send (const nil? {:Class :Module}) :new ...)` (`Class`/`Module` /
    // `::Class`/`::Module`, top-level only). `send` covers `Send` only.
    is_class_or_module_new_call(call, cx)
}

#[cfg(test)]
mod tests {
    use super::{ConstantResolution, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unqualified_const() {
        test::<ConstantResolution>().expect_offense(indoc! {r#"
            MyConst
            ^^^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
        "#});
    }

    #[test]
    fn flags_only_first_segment_of_namespace_const() {
        test::<ConstantResolution>().expect_offense(indoc! {r#"
            MyConst::MY_CONST
            ^^^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
        "#});
    }

    #[test]
    fn does_not_flag_cbase_qualified() {
        test::<ConstantResolution>().expect_no_offenses(indoc! {r#"
            ::MyConst
        "#});
    }

    #[test]
    fn does_not_flag_cbase_namespace() {
        test::<ConstantResolution>().expect_no_offenses(indoc! {r#"
            ::MyConst::MY_CONST
        "#});
    }

    #[test]
    fn does_not_flag_module_and_class_definitions() {
        test::<ConstantResolution>().expect_no_offenses(indoc! {r#"
            module Foo; end
            class Bar; end
        "#});
    }

    /// RuboCop parity (verified against 1.86.2 → offense at 1:7): the inner
    /// segment of a namespaced *definition* name (`class Foo::Bar`) is an
    /// unqualified `Const { scope: None }` whose immediate parent is the outer
    /// `Bar` const — not the `class` node — so `parent_defines_module` (a
    /// single `.parent` hop, mirroring RuboCop's `node.parent&.defined_module`)
    /// does not skip it. Walking up through `Const` parents would wrongly
    /// suppress an offense RuboCop emits.
    #[test]
    fn flags_inner_const_of_class_definition_namespace() {
        test::<ConstantResolution>().expect_offense(indoc! {r#"
            class Foo::Bar
                  ^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
            end
        "#});
    }

    /// RuboCop parity (verified against 1.86.2 → offense at 3:8 on `Class`): in
    /// `A::B = Class.new`, the namespace `A`'s immediate parent is the `casgn`
    /// whose value is `Class.new`, so it is skipped; the bare `Class` receiver
    /// is flagged. Confirms the immediate-parent check matches RuboCop on the
    /// `casgn` definition shape.
    #[test]
    fn class_new_casgn_skips_namespace_and_flags_constructor() {
        test::<ConstantResolution>().expect_offense(indoc! {r#"
            A::B = Class.new
                   ^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
        "#});
    }

    #[test]
    fn only_restricts_to_listed_names() {
        let opts = Options {
            only: vec!["MY_CONST".to_string()],
            ignore: vec![],
        };
        test::<ConstantResolution>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                MY_CONST
                ^^^^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
            "#});
        test::<ConstantResolution>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                MyConst
            "#});
    }

    #[test]
    fn only_namespace_const_fires_on_first_segment() {
        let opts = Options {
            only: vec!["MY_CONST".to_string()],
            ignore: vec![],
        };
        test::<ConstantResolution>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                MY_CONST::B
                ^^^^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
            "#});
    }

    #[test]
    fn ignore_excludes_listed_names() {
        let opts = Options {
            only: vec![],
            ignore: vec!["MY_CONST".to_string()],
        };
        test::<ConstantResolution>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                MyConst
                ^^^^^^^ Fully qualify this constant to avoid possibly ambiguous resolution.
            "#});
        test::<ConstantResolution>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                MY_CONST
            "#});
    }

    // --- Boundary characterization (murphy-ft88.11): pin the exact node set
    // the hand-rolled `is_global_const(Class/Module)` guard matches, so the
    // verbatim `(send (const nil? {:Class :Module}) :new ...)` refactor can be
    // proven equivalent. `::Class` collapses to `Const{scope:None}`: `nil?`
    // covers bare + `::` (both skip the definition const). Namespaced
    // `Foo::Class` is not top-level, so the definition const still flags.
    // `send` covers `Send` only, matching `is_class_constructor` Send-only
    // (csend still flags). `Only=[A]` isolates the definition const `A`
    // from the constructor receiver consts.

    fn only_a() -> Options {
        Options {
            only: vec!["A".to_string()],
            ignore: vec![],
        }
    }

    #[test]
    fn boundary_skips_cbase_class_new_definition() {
        test::<ConstantResolution>()
            .with_options(&only_a())
            .expect_no_offenses("A::B = ::Class.new\n");
    }

    #[test]
    fn boundary_flags_namespaced_class_new_definition() {
        test::<ConstantResolution>()
            .with_options(&only_a())
            .expect_offense(indoc! {r#"
                A::B = Foo::Class.new
                ^ Fully qualify this constant to avoid possibly ambiguous resolution.
            "#});
    }

    #[test]
    fn boundary_flags_csend_class_new_definition() {
        test::<ConstantResolution>()
            .with_options(&only_a())
            .expect_offense(indoc! {r#"
                A::B = Class&.new
                ^ Fully qualify this constant to avoid possibly ambiguous resolution.
            "#});
    }
}

murphy_plugin_api::submit_cop!(ConstantResolution);
