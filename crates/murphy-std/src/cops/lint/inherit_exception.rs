//! `Lint/InheritException` — flag classes inheriting from `Exception`
//! instead of `StandardError` or `RuntimeError`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/InheritException
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/InheritException cop with default style
//!   `standard_error`. The `EnforcedStyle` option is exported in the
//!   schema but runtime reads come from `Default` (v1 limitation).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Lint/InheritException` matcher `class_new_call?` is
// `(send (const {cbase nil?} :Class) :new $(const {cbase nil?} _))`.
// In Murphy `::Class` collapses to `Const{scope:None}`, so a single `nil?`
// scope covers bare and `::`-prefixed forms — equivalent to the prior
// `is_global_const` check. The argument capture (`$(const ...)`) stays
// hand-rolled below. The class-form `exception_class?` check
// (`is_exception_const`) is not a node matcher upstream, so it stays
// hand-rolled too.
def_node_matcher!(class_new_call, "(send (const nil? :Class) :new ...)");

#[derive(Default)]
pub struct InheritException;

#[derive(CopOptions)]
pub struct Options {
    #[option(name = "EnforcedStyle", 
        default = "standard_error",
        description = "Preferred base class: `standard_error` or `runtime_error`."
    )]
    pub enforced_style: String,
}

#[cop(
    name = "Lint/InheritException",
    description = "Inherit from StandardError or RuntimeError instead of Exception.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl InheritException {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { superclass, .. } = *cx.kind(node) else { return; };
        let Some(super_id) = superclass.get() else { return; };
        if !is_exception_const(cx, super_id) {
            return;
        }
        if has_preceding_exception_sibling(cx, node, super_id) {
            return;
        }
        let opts = cx.options_or_default::<Options>();
        let preferred = preferred_base_class(&opts);
        cx.emit_offense(
            cx.range(super_id),
            &format!("Inherit from `{preferred}` instead of `Exception`."),
            None,
        );
        cx.emit_edit(cx.range(super_id), preferred);
    }

    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send (const nil? :Class) :new ...)` — top-level `Class.new`.
        if !class_new_call(node, cx) {
            return;
        }
        let args = cx.call_arguments(node);
        if args.len() != 1 { return; }
        let first = args[0];
        if !is_exception_const(cx, first) {
            return;
        }
        let opts = cx.options_or_default::<Options>();
        let preferred = preferred_base_class(&opts);
        cx.emit_offense(
            cx.range(first),
            &format!("Inherit from `{preferred}` instead of `Exception`."),
            None,
        );
        cx.emit_edit(cx.range(first), preferred);
    }
}

fn is_exception_const(cx: &Cx<'_>, id: NodeId) -> bool {
    cx.is_global_const(id, "Exception")
}

fn has_preceding_exception_sibling(cx: &Cx<'_>, class_node: NodeId, superclass_id: NodeId) -> bool {
    let NodeKind::Const { scope, .. } = *cx.kind(superclass_id) else { return false; };
    if matches!(scope.get(), Some(s) if matches!(*cx.kind(s), NodeKind::Cbase)) {
        return false;
    }
    for sibling in cx.children(class_node) {
        if sibling == superclass_id { break; }
        if is_exception_const(cx, sibling) {
            return true;
        }
    }
    false
}

fn preferred_base_class(opts: &Options) -> &str {
    match opts.enforced_style.as_str() {
        "runtime_error" => "RuntimeError",
        _ => "StandardError",
    }
}

#[cfg(test)]
mod tests {
    use super::{InheritException, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_class_inheriting_exception() {
        test::<InheritException>().expect_offense(indoc! {r#"
            class C < Exception; end
                      ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
        "#});
    }

    #[test]
    fn ignores_class_inheriting_standard_error() {
        test::<InheritException>().expect_no_offenses("class C < StandardError; end\n");
    }

    #[test]
    fn ignores_class_with_no_superclass() {
        test::<InheritException>().expect_no_offenses("class C; end\n");
    }

    #[test]
    fn flags_class_runtime_error() {
        test::<InheritException>()
            .with_options(&Options { enforced_style: "runtime_error".into() })
            .expect_offense(indoc! {r#"
                class C < Exception; end
                          ^^^^^^^^^ Inherit from `RuntimeError` instead of `Exception`.
            "#});
    }

    #[test]
    fn autocorrects_to_runtime_error() {
        test::<InheritException>()
            .with_options(&Options { enforced_style: "runtime_error".into() })
            .expect_correction(
                indoc! {r#"
                    class C < Exception; end
                              ^^^^^^^^^ Inherit from `RuntimeError` instead of `Exception`.
                "#},
                "class C < RuntimeError; end\n",
            );
    }

    #[test]
    fn autocorrects_to_standard_error() {
        test::<InheritException>().expect_correction(
            indoc! {r#"
                class C < Exception; end
                          ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
            "#},
            "class C < StandardError; end\n",
        );
    }

    #[test]
    fn flags_class_new_exception() {
        test::<InheritException>().expect_offense(indoc! {r#"
            C = Class.new(Exception)
                          ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
        "#});
    }

    #[test]
    fn autocorrects_class_new() {
        test::<InheritException>().expect_correction(
            indoc! {r#"
                C = Class.new(Exception)
                              ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
            "#},
            "C = Class.new(StandardError)\n",
        );
    }

    // --- Boundary characterization (murphy-ft88.2): pin the exact node set
    // the hand-rolled `Class`-receiver check matches, so the
    // `(send (const nil? :Class) :new ...)` refactor can be proven
    // equivalent. The `$(const ...)` argument capture stays hand-rolled
    // below (as with batch-3 IoSelect). `::Class` collapses to
    // `Const{scope:None}` in Murphy, so a single `nil?` scope covers bare
    // + `::`-prefixed forms.

    #[test]
    fn boundary_flags_cbase_class_new() {
        test::<InheritException>().expect_offense(indoc! {r#"
            C = ::Class.new(Exception)
                            ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
        "#});
    }

    #[test]
    fn boundary_ignores_namespaced_class_new() {
        // `Foo::Class` has a non-nil const scope, so it is not flagged.
        test::<InheritException>().expect_no_offenses("C = Foo::Class.new(Exception)\n");
    }

    #[test]
    fn boundary_ignores_csend_class_new() {
        // `&.` is a `csend` node, not `send`; RuboCop's `(send ...)`
        // pattern does not match it, and `#[on_node(kind = "send")]`
        // never dispatches on it.
        test::<InheritException>().expect_no_offenses("C = Class&.new(Exception)\n");
    }
}
murphy_plugin_api::submit_cop!(InheritException);
