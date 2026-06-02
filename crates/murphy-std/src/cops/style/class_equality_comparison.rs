//! `Style/ClassEqualityComparison` — enforces `instance_of?` instead of class
//! comparison for equality.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassEqualityComparison
//! upstream_version_checked: 1.86.2
//! version_added: "0.93"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   AllowedMethods (default: ["==", "equal?", "eql?"]) and AllowedPatterns
//!   (default: []) are both implemented. AllowedPatterns uses simple substring
//!   match (RuboCop uses Regexp), which is a known v1 limitation.
//!   Autocorrect is emitted but marked unsafe -- the constant named in the
//!   replacement may not exist at the call site (e.g. var.class.name == 'Foo'
//!   to var.instance_of?(Foo) requires Foo to be in scope). Murphy has no
//!   cop-level safety metadata knob in v1.
//!   No autocorrect is emitted when the comparison argument is a variable or
//!   a call (unknown type), matching RuboCop's behavior.
//!   String argument inside a module/class context is prefixed with `::` to
//!   ensure the constant lookup is absolute.
//!   `on_csend` (safe-navigation `var&.class`) is not handled -- RuboCop also
//!   only subscribes `on_send`, so this is not a gap.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `==`, `equal?`, or `eql?` whose receiver is one of:
//!
//! - `recv.class` -- the receiver is a `.class` call
//! - `recv.class.name` / `recv.class.to_s` / `recv.class.inspect` -- the
//!   receiver is a `.name`/`.to_s`/`.inspect` call on a `.class` call
//!
//! The offense range spans from the `.class` selector to the end of the whole
//! comparison node.
//!
//! ## Autocorrect
//!
//! Replaces the offense range with `instance_of?(<class_argument>)`:
//!
//! - `var.class == Date` to `var.instance_of?(Date)` (class_argument = "Date")
//! - `var.class.name == 'Date'` to `var.instance_of?(Date)` (stripped quotes)
//! - `var.class.name == Date.name` to `var.instance_of?(Date)` (receiver source)
//! - `var.class.name == class_name` -- no autocorrect (variable, unknown type)
//!
//! When the comparison argument cannot be determined (variable or call), the
//! offense message omits the parenthesized class argument.
//!
//! ## AllowedMethods and AllowedPatterns
//!
//! When the node is inside a `def` or `defs` method definition whose name
//! appears in `AllowedMethods`, or matches any pattern in `AllowedPatterns`
//! (substring), the offense is suppressed.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_WITH_CLASS: &str =
    "Use `instance_of?{class_argument}` instead of comparing classes.";
const MSG_WITHOUT_CLASS: &str = "Use `instance_of?` instead of comparing classes.";

/// Method names that, when called on `.class`, indicate a string-name comparison.
const CLASS_NAME_METHODS: &[&str] = &["name", "to_s", "inspect"];

#[derive(Default)]
pub struct ClassEqualityComparison;

#[derive(CopOptions)]
pub struct ClassEqualityComparisonOptions {
    #[option(
        name = "AllowedMethods",
        default = ["==", "equal?", "eql?"],
        description = "Method names that are exempt from this cop (typically equality methods)."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Patterns (substring match) for method names to exempt."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Style/ClassEqualityComparison",
    description = "Enforces `instance_of?` instead of class comparison for equality.",
    default_severity = "warning",
    default_enabled = true,
    options = ClassEqualityComparisonOptions,
)]
impl ClassEqualityComparison {
    #[on_node(kind = "send", methods = ["==", "equal?", "eql?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        args,
        ..
    } = *cx.kind(node)
    else {
        return;
    };

    // Require exactly one argument.
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let class_node = arg_list[0];

    // Skip dstr (interpolated string) arguments -- no offense.
    if matches!(cx.kind(class_node), NodeKind::Dstr(_)) {
        return;
    }

    // The comparison receiver must be present.
    let Some(recv_id) = receiver.get() else {
        return;
    };

    // Determine if the receiver is a `.class` call (direct or via name method).
    // Shape A: `recv.class` -- receiver is the `.class` Send.
    // Shape B: `recv.class.name/to_s/inspect` -- receiver is a name-method Send
    //          whose own receiver is the `.class` Send.
    let (class_send_id, via_name_method) = match classify_receiver(recv_id, cx) {
        Some(v) => v,
        None => return,
    };

    // AllowedMethods / AllowedPatterns: check if the node is inside a def/defs
    // whose method name is in the allowed list.
    if is_allowed_context(node, cx) {
        return;
    }

    // Compute the offense range: from the `.class` selector start to the end
    // of the comparison node. This trims the receiver prefix (`var.`).
    let class_selector_start = cx.loc(class_send_id).name.start;
    let node_end = cx.range(node).end;
    let offense_range = Range {
        start: class_selector_start,
        end: node_end,
    };

    // Compute the class name for the autocorrect / message.
    let class_name = resolve_class_name(class_node, node, via_name_method, cx);

    if let Some(name) = &class_name {
        let class_argument = format!("({name})");
        let msg = MSG_WITH_CLASS.replace("{class_argument}", &class_argument);
        cx.emit_offense(offense_range, &msg, None);
        let replacement = format!("instance_of?{class_argument}");
        cx.emit_edit(offense_range, &replacement);
    } else {
        cx.emit_offense(offense_range, MSG_WITHOUT_CLASS, None);
        // No autocorrect -- type is unknown.
    }
}

/// Classify the receiver of a `==`/`equal?`/`eql?` call.
///
/// Returns `Some((class_send_id, via_name_method))` when the receiver is a
/// `.class` or `.class.NAME` chain, or `None` otherwise.
///
/// - `via_name_method = false`: receiver is exactly `recv.class`
/// - `via_name_method = true`:  receiver is `recv.class.name/to_s/inspect`
fn classify_receiver(recv_id: NodeId, cx: &Cx<'_>) -> Option<(NodeId, bool)> {
    match cx.kind(recv_id) {
        NodeKind::Send {
            receiver: inner_recv,
            method,
            ..
        } => {
            let method_name = cx.symbol_str(*method);
            if method_name == "class" {
                // Shape A: `recv.class`
                inner_recv.get()?; // guard: must have a receiver
                return Some((recv_id, false));
            }
            if CLASS_NAME_METHODS.contains(&method_name) {
                // Shape B: potentially `recv.class.name/to_s/inspect`
                if let Some(name_recv_id) = inner_recv.get()
                    && let NodeKind::Send {
                        receiver: class_recv,
                        method: class_method,
                        ..
                    } = cx.kind(name_recv_id)
                    && cx.symbol_str(*class_method) == "class"
                    && class_recv.get().is_some()
                {
                    return Some((name_recv_id, true));
                }
            }
            None
        }
        _ => None,
    }
}

/// Returns `true` when the comparison node is inside a def/defs method
/// whose name is in AllowedMethods or matches any AllowedPatterns entry.
fn is_allowed_context(node: NodeId, cx: &Cx<'_>) -> bool {
    let opts = cx.options_or_default::<ClassEqualityComparisonOptions>();
    if opts.allowed_methods.is_empty() && opts.allowed_patterns.is_empty() {
        return false;
    }

    // Find the nearest enclosing def or defs.
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => {
                let method_name = cx.symbol_str(*name);
                if opts.allowed_methods.iter().any(|m| m == method_name) {
                    return true;
                }
                if opts
                    .allowed_patterns
                    .iter()
                    .any(|p| method_name.contains(p.as_str()))
                {
                    return true;
                }
                // Only the first (nearest) def/defs is checked.
                return false;
            }
            _ => {}
        }
    }
    false
}

/// Resolve the class name string used in the autocorrect replacement.
///
/// Returns `Some(name)` when a concrete class name can be determined,
/// or `None` when the type is unknown (variable or call), suppressing
/// autocorrect.
fn resolve_class_name(
    class_node: NodeId,
    comparison_node: NodeId,
    via_name_method: bool,
    cx: &Cx<'_>,
) -> Option<String> {
    if via_name_method {
        // The class argument is one of:
        // (a) `Foo.name` / `Foo.to_s` / `Foo.inspect` -- use the receiver source
        // (b) a string literal -- strip quotes, possibly prepend `::`
        // (c) a variable or call -- unknown, return None
        match cx.kind(class_node) {
            NodeKind::Send {
                receiver: arg_recv,
                method: arg_method,
                ..
            } => {
                let method_name = cx.symbol_str(*arg_method);
                if CLASS_NAME_METHODS.contains(&method_name) {
                    // Shape (a): e.g. `Date.name` -- use `Date`
                    let recv_id = arg_recv.get()?;
                    let recv_src = cx.raw_source(cx.range(recv_id));
                    return Some(recv_src.to_string());
                }
                // Other send or bare call: unknown type.
                None
            }
            NodeKind::Csend {
                receiver: arg_recv,
                method: arg_method,
                ..
            } => {
                let method_name = cx.symbol_str(*arg_method);
                if CLASS_NAME_METHODS.contains(&method_name) {
                    let recv_src = cx.raw_source(cx.range(*arg_recv));
                    return Some(recv_src.to_string());
                }
                None
            }
            NodeKind::Str(sid) => {
                // Shape (b): strip quotes by using the interned string value.
                let value = cx.string_str(*sid);
                // Prepend `::` if the comparison is inside a class or module.
                let needs_cbase = cx.ancestors(comparison_node).any(|a| {
                    matches!(cx.kind(a), NodeKind::Class { .. } | NodeKind::Module { .. })
                });
                if needs_cbase {
                    Some(format!("::{value}"))
                } else {
                    Some(value.to_string())
                }
            }
            // Lvar, Ivar, Cvar, Gvar: unknown type.
            NodeKind::Lvar(_) | NodeKind::Ivar(_) | NodeKind::Cvar(_) | NodeKind::Gvar(_) => None,
            // Any other call/send: return None (unknown type).
            _ => {
                if is_call_or_variable(class_node, cx) {
                    None
                } else {
                    // Literal or constant (e.g. Const): use raw source.
                    Some(cx.raw_source(cx.range(class_node)).to_string())
                }
            }
        }
    } else {
        // Shape A: `recv.class == <something>` -- use the raw source of the arg.
        // RuboCop uses `class_node.source` for all non-name-method shapes.
        Some(cx.raw_source(cx.range(class_node)).to_string())
    }
}

/// Returns `true` when the node is a variable or call (type unknown at
/// compile time for autocorrect purposes).
fn is_call_or_variable(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Lvar(_)
            | NodeKind::Ivar(_)
            | NodeKind::Cvar(_)
            | NodeKind::Gvar(_)
            | NodeKind::Send { .. }
            | NodeKind::Csend { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::ClassEqualityComparison;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic shapes: ==, equal?, eql? ---

    #[test]
    fn flags_and_corrects_class_eq_eq() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class == Date
                    ^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_equal_predicate() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.equal?(Date)
                    ^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_eql_predicate() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.eql?(Date)
                    ^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    // --- Class#name shapes ---

    #[test]
    fn flags_and_corrects_class_name_single_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.name == 'Date'
                    ^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_name_double_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.name == "Date"
                    ^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_name_vs_module_name() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.name == Date.name
                    ^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn does_not_flag_class_name_interpolated_string() {
        test::<ClassEqualityComparison>()
            .expect_no_offenses(r#"var.class.name == "String#{interpolation}""#);
    }

    #[test]
    fn flags_no_correction_for_local_variable() {
        test::<ClassEqualityComparison>()
            .expect_offense(indoc! {r#"
                class_name = 'Model'
                var.class.name == class_name
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?` instead of comparing classes.
            "#})
            .expect_no_corrections(indoc! {r#"
                class_name = 'Model'
                var.class.name == class_name
            "#});
    }

    #[test]
    fn flags_no_correction_for_instance_variable() {
        test::<ClassEqualityComparison>()
            .expect_offense(indoc! {r#"
                var.class.name == @class_name
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?` instead of comparing classes.
            "#})
            .expect_no_corrections(indoc! {r#"
                var.class.name == @class_name
            "#});
    }

    #[test]
    fn flags_no_correction_for_method_call() {
        // Bare method call (Send with no receiver) -- unknown type.
        test::<ClassEqualityComparison>()
            .expect_offense(indoc! {r#"
                var.class.name == class_name
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?` instead of comparing classes.
            "#})
            .expect_no_corrections(indoc! {r#"
                var.class.name == class_name
            "#});
    }

    #[test]
    fn flags_no_correction_for_safe_navigation_method_call() {
        test::<ClassEqualityComparison>()
            .expect_offense(indoc! {r#"
                var.class.name == obj&.class_name
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?` instead of comparing classes.
            "#})
            .expect_no_corrections(indoc! {r#"
                var.class.name == obj&.class_name
            "#});
    }

    // --- Class#to_s ---

    #[test]
    fn flags_and_corrects_class_to_s_single_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.to_s == 'Date'
                    ^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_to_s_double_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.to_s == "Date"
                    ^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_to_s_vs_module_name() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.to_s == Date.to_s
                    ^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    // --- Class#inspect ---

    #[test]
    fn flags_and_corrects_class_inspect_single_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.inspect == 'Date'
                    ^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_inspect_double_quote() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.inspect == "Date"
                    ^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_inspect_vs_module_name() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                var.class.inspect == Date.inspect
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Date)` instead of comparing classes.
            "#},
            "var.instance_of?(Date)\n",
        );
    }

    // --- Negative: already correct ---

    #[test]
    fn accepts_instance_of_predicate() {
        test::<ClassEqualityComparison>()
            .expect_no_offenses("var.instance_of?(Date)\n");
    }

    // --- AllowedMethods ---

    #[test]
    fn respects_allowed_methods() {
        use super::ClassEqualityComparisonOptions;
        test::<ClassEqualityComparison>()
            .with_options(&ClassEqualityComparisonOptions {
                allowed_methods: vec!["==".to_string()],
                allowed_patterns: vec![],
            })
            .expect_no_offenses(indoc! {r#"
                def ==(other)
                  self.class == other.class &&
                    name == other.name
                end
            "#});
    }

    // Default AllowedMethods includes ==, equal?, eql?.
    #[test]
    fn default_allowed_methods_skip_eq_in_def() {
        test::<ClassEqualityComparison>().expect_no_offenses(indoc! {r#"
            def ==(other)
              self.class == other.class && name == other.name
            end
        "#});
    }

    #[test]
    fn default_allowed_methods_skip_equal_pred_in_def() {
        test::<ClassEqualityComparison>().expect_no_offenses(indoc! {r#"
            def equal?(other)
              self.class.equal?(other.class) && name.equal?(other.name)
            end
        "#});
    }

    #[test]
    fn default_allowed_methods_skip_eql_pred_in_def() {
        test::<ClassEqualityComparison>().expect_no_offenses(indoc! {r#"
            def eql?(other)
              self.class.eql?(other.class) && name.eql?(other.name)
            end
        "#});
    }

    // --- AllowedPatterns ---

    #[test]
    fn respects_allowed_patterns() {
        use super::ClassEqualityComparisonOptions;
        test::<ClassEqualityComparison>()
            .with_options(&ClassEqualityComparisonOptions {
                allowed_methods: vec![],
                allowed_patterns: vec!["equal".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                def equal?(other)
                  self.class == other.class &&
                    name == other.name
                end
            "#});
    }

    // --- Module context: string arg gets :: prefix ---

    #[test]
    fn flags_and_corrects_string_in_module_context() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                module Foo
                  def bar?(value)
                    bar.class.name == 'Bar'
                        ^^^^^^^^^^^^^^^^^^^ Use `instance_of?(::Bar)` instead of comparing classes.
                  end

                  class Bar
                  end
                end
            "#},
            indoc! {r#"
                module Foo
                  def bar?(value)
                    bar.instance_of?(::Bar)
                  end

                  class Bar
                  end
                end
            "#},
        );
    }

    // --- Module context: const arg does not get :: prefix ---

    #[test]
    fn flags_and_corrects_const_in_module_context() {
        test::<ClassEqualityComparison>().expect_correction(
            indoc! {r#"
                module Foo
                  def bar?(value)
                    bar.class.name == Model
                        ^^^^^^^^^^^^^^^^^^^ Use `instance_of?(Model)` instead of comparing classes.
                  end
                end
            "#},
            indoc! {r#"
                module Foo
                  def bar?(value)
                    bar.instance_of?(Model)
                  end
                end
            "#},
        );
    }
}

murphy_plugin_api::submit_cop!(ClassEqualityComparison);
