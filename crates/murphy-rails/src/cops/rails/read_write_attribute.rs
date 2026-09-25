//! `Rails/ReadWriteAttribute` — prefer `self[:attr]` over `read_attribute`/`write_attribute`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ReadWriteAttribute
//! upstream_version_checked: 2.35.0
//! version_added: "0.20"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [:read_attribute, :write_attribute] (send only, no csend), bare
//!   receiver, exact arities (1 / 2), and the shadowing-method exemption
//!   (a `def` ancestor whose name matches the attribute, `foo=` for
//!   writes). Offense is the whole node; autocorrect rewrites to
//!   `self[attr]` / `self[attr] = val`. Multi-line messages use the
//!   generic `self[:attr]` / `self[:attr] = val` spellings. Upstream
//!   Include gating (`app/models`) has no file-path infrastructure in
//!   Murphy yet, so the cop fires in all files.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReadWriteAttribute;

#[cop(
    name = "Rails/ReadWriteAttribute",
    description = "Checks for read_attribute(:attr) and write_attribute(:attr, val).",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ReadWriteAttribute {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["read_attribute", "write_attribute"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let is_read = method == "read_attribute";
    let is_write = method == "write_attribute";
    if !is_read && !is_write {
        return;
    }
    // Upstream patterns require a bare (`nil?`) receiver.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    // `(send nil? :read_attribute _)` / `(send nil? :write_attribute _ _)`.
    if is_read && args.len() != 1 {
        return;
    }
    if is_write && args.len() != 2 {
        return;
    }
    if within_shadowing_method(cx, node, args[0], is_write) {
        return;
    }
    let replacement = if is_read {
        format!("self[{}]", cx.raw_source(cx.range(args[0])))
    } else {
        format!(
            "self[{}] = {}",
            cx.raw_source(cx.range(args[0])),
            cx.raw_source(cx.range(args[1]))
        )
    };
    // Upstream uses the concrete replacement for single-line nodes and a
    // generic `self[:attr]` / `self[:attr] = val` spelling for multi-line.
    let prefer = if cx.raw_source(cx.range(node)).contains('\n') {
        if is_read {
            "self[:attr]".to_owned()
        } else {
            "self[:attr] = val".to_owned()
        }
    } else {
        replacement.clone()
    };
    cx.emit_offense(
        cx.range(node),
        &format!("Prefer `{prefer}`."),
        None,
    );
    cx.emit_edit(cx.range(node), &replacement);
}

/// Upstream `within_shadowing_method?`: the first argument names a `def`
/// ancestor (with `=` appended for writes), in which case the explicit
/// reader/writer avoids infinite recursion and must be kept.
fn within_shadowing_method(cx: &Cx<'_>, node: NodeId, first_arg: NodeId, is_write: bool) -> bool {
    let attr_name = match *cx.kind(first_arg) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(id) => cx.string_str(id).to_owned(),
        _ => return false,
    };
    let wanted = if is_write {
        format!("{attr_name}=")
    } else {
        attr_name
    };
    for anc in cx.ancestors(node) {
        if let NodeKind::Def { name, .. } = *cx.kind(anc)
            && cx.symbol_str(name) == wanted
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ReadWriteAttribute;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_read_attribute() {
        test::<ReadWriteAttribute>().expect_correction(
            indoc! {r#"
                x = read_attribute(:attr)
                    ^^^^^^^^^^^^^^^^^^^^^ Prefer `self[:attr]`.
            "#},
            "x = self[:attr]\n",
        );
    }

    #[test]
    fn flags_write_attribute() {
        test::<ReadWriteAttribute>().expect_correction(
            indoc! {r#"
                write_attribute(:attr, val)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `self[:attr] = val`.
            "#},
            "self[:attr] = val\n",
        );
    }

    #[test]
    fn allows_receiver() {
        test::<ReadWriteAttribute>().expect_no_offenses("x = obj.read_attribute(:attr)\n");
    }

    #[test]
    fn allows_wrong_arity() {
        test::<ReadWriteAttribute>().expect_no_offenses("x = read_attribute(:a, :b)\n");
    }

    #[test]
    fn allows_shadowing_reader() {
        test::<ReadWriteAttribute>().expect_no_offenses(indoc! {r#"
            def foo
              bar || read_attribute(:foo)
            end
        "#});
    }

    #[test]
    fn allows_shadowing_writer() {
        test::<ReadWriteAttribute>().expect_no_offenses(indoc! {r#"
            def foo=(val)
              write_attribute(:foo, val)
            end
        "#});
    }

    #[test]
    fn flags_non_shadowing_def() {
        test::<ReadWriteAttribute>().expect_correction(
            indoc! {r#"
                def bar
                  read_attribute(:foo)
                  ^^^^^^^^^^^^^^^^^^^^ Prefer `self[:foo]`.
                end
            "#},
            "def bar\n  self[:foo]\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(ReadWriteAttribute);
