//! `Rails/I18nLazyLookup` — use lazy lookup in controllers.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/I18nLazyLookup
//! upstream_version_checked: 2.35.0
//! version_added: "2.14"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [translate t] with bare
//!   receivers, first-arg Sym/Str gating, EnforcedStyle lazy/explicit,
//!   controller (class *Controller) + public def action scoping with
//!   `private`/`protected` section and modifier handling, scoped-key
//!   computation (underscore + tr), lazy flags only when key == scoped key,
//!   explicit flags any `.key`. Offense is the key node, autocorrect rewrites
//!   to `.last` / scoped key with single quotes. Upstream Include
//!   (`**/app/controllers/**/*.rb`) has no file-path infrastructure in
//!   Murphy (audit murphy-4gd.1.15).
//! ```
//!
//! Checks for places where I18n "lazy" lookup can be used.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct I18nLazyLookup;

#[derive(CopOptions)]
pub struct I18nLazyLookupOptions {
    #[option(
        name = "EnforcedStyle",
        default = "lazy",
        description = "Whether to prefer lazy (`.key`) or explicit (`controller.action.key`) lookup."
    )]
    pub enforced_style: I18nLazyLookupStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum I18nLazyLookupStyle {
    #[option(value = "lazy")]
    Lazy,
    #[option(value = "explicit")]
    Explicit,
}

#[cop(
    name = "Rails/I18nLazyLookup",
    description = "Checks for places where I18n \"lazy\" lookup can be used.",
    default_severity = "warning",
    default_enabled = false,
    options = I18nLazyLookupOptions,
)]
impl I18nLazyLookup {
    #[on_node(kind = "send", methods = ["t", "translate"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `(send nil? ...)` — `I18n.t` never flags.
        if receiver.get().is_some() {
            return;
        }
        let m = cx.symbol_str(method);
        if m != "t" && m != "translate" {
            return;
        }
        let args = cx.call_arguments(node);
        let Some(&first) = args.first() else {
            return;
        };
        let Some(key) = sym_or_str_value(cx, first) else {
            return;
        };
        let opts = cx.options_or_default::<I18nLazyLookupOptions>();
        let style = opts.enforced_style;
        match style {
            I18nLazyLookupStyle::Lazy => {
                if key.starts_with('.') {
                    return;
                }
                let Some((controller, action)) = controller_and_action(cx, node) else {
                    return;
                };
                let scoped = scoped_key(cx, controller, action, &key);
                if key != scoped {
                    return;
                }
                let last = key.rsplit('.').next().unwrap_or(&key);
                cx.emit_offense(
                    cx.range(first),
                    "Use lazy lookup for the text used in controllers.",
                    None,
                );
                cx.emit_edit(cx.range(first), &format!("'.{last}'"));
            }
            I18nLazyLookupStyle::Explicit => {
                if !key.starts_with('.') {
                    return;
                }
                let Some((controller, action)) = controller_and_action(cx, node) else {
                    return;
                };
                let scoped = scoped_key(cx, controller, action, &key);
                cx.emit_offense(
                    cx.range(first),
                    "Use explicit lookup for the text used in controllers.",
                    None,
                );
                cx.emit_edit(cx.range(first), &format!("'{scoped}'"));
            }
        }
    }
}

fn sym_or_str_value(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Str(s) => Some(cx.string_str(s).to_owned()),
        _ => None,
    }
}

/// Upstream `controller_and_action`: nearest def must be public, nearest
/// class must end with Controller.
fn controller_and_action(cx: &Cx<'_>, node: NodeId) -> Option<(NodeId, NodeId)> {
    let mut action = None;
    let mut controller = None;
    for anc in cx.ancestors(node) {
        if action.is_none() {
            if let NodeKind::Def { .. } = *cx.kind(anc) {
                action = Some(anc);
                continue;
            }
            // `defs` (def self.action) is not an action.
            if matches!(*cx.kind(anc), NodeKind::Defs { .. }) {
                return None;
            }
        } else if controller.is_none()
            && matches!(*cx.kind(anc), NodeKind::Class { .. })
        {
            // A module between def and class does not break the search;
            // upstream uses each_ancestor(:def).first and (:class).first
            // independently, so a non-class ancestor simply continues scanning.
            controller = Some(anc);
            break;
        }
    }
    let action = action?;
    let controller = controller?;
    // Must be public.
    if is_private_or_protected(cx, action) {
        return None;
    }
    // Controller name must end with Controller.
    let full = full_controller_const(cx, controller)?;
    let short = full.rsplit("::").next().unwrap_or(&full);
    if !short.ends_with("Controller") {
        return None;
    }
    Some((controller, action))
}

fn full_controller_const(cx: &Cx<'_>, class_node: NodeId) -> Option<String> {
    let NodeKind::Class { name, .. } = *cx.kind(class_node) else {
        return None;
    };
    let own = cx.const_name(name)?;
    if own.contains("::") {
        return Some(own);
    }
    // Prepend enclosing modules.
    let mut mods: Vec<String> = Vec::new();
    for anc in cx.ancestors(class_node) {
        if let NodeKind::Module { name: mname, .. } = *cx.kind(anc)
            && let Some(n) = cx.const_name(mname)
        {
            mods.push(n);
        }
    }
    // Ancestors are inner-out; reverse to outer-inner.
    mods.reverse();
    if mods.is_empty() {
        Some(own)
    } else {
        mods.push(own);
        Some(mods.join("::"))
    }
}

fn scoped_key(cx: &Cx<'_>, controller: NodeId, action: NodeId, key: &str) -> String {
    let full = full_controller_const(cx, controller).unwrap_or_default();
    let path = underscore(full.strip_suffix("Controller").unwrap_or(&full)).replace('/', ".");
    let action_name = cx.method_name(action).unwrap_or_default();
    let last = key.rsplit('.').next().unwrap_or(key);
    // If key starts with '.', last still works (".key" -> "key").
    let last = last.trim_start_matches('.');
    // For lazy keys like "foo.action.key", last is "key"; for ".key", last is "key".
    format!("{path}.{action_name}.{last}")
}

fn underscore(s: &str) -> String {
    // ActiveSupport underscore approximation.
    let r = s.replace("::", "/");
    let mut out = String::with_capacity(r.len() + 4);
    let chars: Vec<char> = r.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied();
                let boundary = prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase()
                        && next.is_some_and(|n| n.is_ascii_lowercase()));
                if boundary {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '-' {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    // Collapse possible double handling — keep simple.
    let _ = r;
    out
}

/// `private def` / `protected def` modifier plus bare-section visibility.
/// Mirrors `Rails/Delegate` helper.
fn is_private_or_protected(cx: &Cx<'_>, def_node: NodeId) -> bool {
    for anc in cx.ancestors(def_node) {
        if let NodeKind::Send { method, .. } = *cx.kind(anc) {
            let m = cx.symbol_str(method);
            if (m == "private" || m == "protected") && reaches_def(cx, anc, def_node) {
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
    if let Some(vis) = section_visibility(cx, def_node) {
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
    use super::{I18nLazyLookup, I18nLazyLookupOptions, I18nLazyLookupStyle};
    use murphy_plugin_api::test_support::test;

    #[test]
    fn flags_scoped_key_lazy() {
        test::<I18nLazyLookup>().expect_offense(
            "class FooController\n  def action\n    t 'foo.action.key'\n      ^^^^^^^^^^^^^^^^ Use lazy lookup for the text used in controllers.\n    translate 'foo.action.key'\n              ^^^^^^^^^^^^^^^^ Use lazy lookup for the text used in controllers.\n  end\nend\n",
        );
    }

    #[test]
    fn corrects_to_lazy() {
        test::<I18nLazyLookup>().expect_correction(
            "class FooController\n  def action\n    t 'foo.action.key'\n      ^^^^^^^^^^^^^^^^ Use lazy lookup for the text used in controllers.\n  end\nend\n",
            "class FooController\n  def action\n    t '.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_i18n_scoped() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooController\n  def action\n    I18n.t 'foo.action.key'\n    I18n.translate 'foo.action.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_non_controller() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooService\n  def do_something\n    t 'foo_service.do_something.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_private_action() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooController\n  private\n  def action\n    t 'foo.action.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_unscoped_key() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooController\n  def action\n    t 'one.two.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_already_lazy() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooController\n  def action\n    t '.key'\n  end\nend\n",
        );
    }

    #[test]
    fn does_not_flag_non_string_keys() {
        test::<I18nLazyLookup>().expect_no_offenses(
            "class FooController\n  def action\n    t ['foo.action.key']\n    t key\n  end\nend\n",
        );
    }

    #[test]
    fn handles_scoped_controllers() {
        test::<I18nLazyLookup>().expect_offense(
            "module Bar\n  class FooController\n    def action\n      t 'bar.foo.action.key'\n        ^^^^^^^^^^^^^^^^^^^^ Use lazy lookup for the text used in controllers.\n      t 'foo.action.key'\n    end\n  end\nend\n",
        );
    }

    #[test]
    fn explicit_flags_lazy() {
        let opts = I18nLazyLookupOptions {
            enforced_style: I18nLazyLookupStyle::Explicit,
        };
        test::<I18nLazyLookup>()
            .with_options(&opts)
            .expect_offense(
                "class FooController\n  def action\n    t '.key'\n      ^^^^^^ Use explicit lookup for the text used in controllers.\n    translate '.key'\n              ^^^^^^ Use explicit lookup for the text used in controllers.\n  end\nend\n",
            );
    }

    #[test]
    fn explicit_corrects_to_scoped() {
        let opts = I18nLazyLookupOptions {
            enforced_style: I18nLazyLookupStyle::Explicit,
        };
        test::<I18nLazyLookup>().with_options(&opts).expect_correction(
            "class FooController\n  def action\n    t '.key'\n      ^^^^^^ Use explicit lookup for the text used in controllers.\n  end\nend\n",
            "class FooController\n  def action\n    t 'foo.action.key'\n  end\nend\n",
        );
    }

    #[test]
    fn explicit_does_not_flag_scoped() {
        let opts = I18nLazyLookupOptions {
            enforced_style: I18nLazyLookupStyle::Explicit,
        };
        test::<I18nLazyLookup>()
            .with_options(&opts)
            .expect_no_offenses("class FooController\n  def action\n    t 'foo.action.key'\n  end\nend\n");
    }

    #[test]
    fn explicit_handles_scoped_controllers() {
        let opts = I18nLazyLookupOptions {
            enforced_style: I18nLazyLookupStyle::Explicit,
        };
        test::<I18nLazyLookup>().with_options(&opts).expect_correction(
            "module Bar\n  class FooController\n    def action\n      t '.key'\n        ^^^^^^ Use explicit lookup for the text used in controllers.\n      t 'foo.action.key'\n    end\n  end\nend\n",
            "module Bar\n  class FooController\n    def action\n      t 'bar.foo.action.key'\n      t 'foo.action.key'\n    end\n  end\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(I18nLazyLookup);
