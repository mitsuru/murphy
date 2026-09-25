//! `Rails/ContentTag` — flag legacy `tag(:p)` in favour of `tag.p`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ContentTag
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:tag] gating, bare
//!   receiver, fewer-than-3-args gate, first-argument allow-list (variable,
//!   send, const, splat, non-tag-name str/sym, valueless nodes), whole-node
//!   offense range with selector-to-end correction to `tag.<name>(rest...)`,
//!   and the nested-tag ancestor suppression (approximated: any ancestor
//!   bare `tag` send that would itself register an offense suppresses the
//!   inner one; upstream tracks actually-corrected ancestors). Not gated:
//!   `minimum_target_rails_version 5.1`. `underscore` is a local
//!   reimplementation of ActiveSupport underscore (namespace, camel-case,
//!   dash handling).
//! ```
//!
//! ## Matched shapes
//!
//! Bare `Send` with method `tag`, fewer than 3 arguments, whose first
//! argument is a tag-name `Sym`/`Str` (e.g. `:p`, `"my-tag"`) or a value
//! node without an allow-list shape (e.g. an `Int`):
//!
//! - `tag(:p)` → `tag.p`
//! - `tag(:br, class: 'classname')` → `tag.br(class: 'classname')`
//!
//! `tag(name)` (variable), `tag(foo.bar)` (send), `tag(FOO)` (const),
//! `tag(*args)` (splat), and `tag(name, class: 'x')` pass through —
//! `tag(name, ...)` with a variable first argument is simpler than
//! `tag.public_send(name)`.
//!
//! ## Autocorrect
//!
//! Replace the selector-to-end range with `tag.<preferred>(rest...)`; the
//! remaining arguments pass through verbatim.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ContentTag;

#[cop(
    name = "Rails/ContentTag",
    description = "Use `tag.p` instead of `tag(:p)`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ContentTag {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[tag]`.
    #[on_node(kind = "send", methods = ["tag"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream: `return unless node.receiver.nil?`.
        if receiver.get().is_some() {
            return;
        }
        let args = cx.call_arguments(node);
        // Upstream: `return if node.arguments.count >= 3`.
        if args.len() >= 3 {
            return;
        }
        let Some(&first) = args.first() else {
            return;
        };
        if is_allowed_argument(cx, first) {
            return;
        }
        if has_offense_ancestor(cx, node) {
            return;
        }
        let value_src = match *cx.kind(first) {
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
            NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
            _ => cx.raw_source(cx.range(first)).to_owned(),
        };
        let preferred = underscore(&value_src);
        let current = cx.raw_source(cx.range(first));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `tag.{preferred}` instead of `tag({current})`."),
            None,
        );
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_owned())
            .collect();
        cx.emit_edit(
            correction_range(cx, node),
            &format!("tag.{preferred}({})", rest.join(", ")),
        );
    }
}

/// Mirrors upstream `allowed_argument?`: variables, sends, consts, splats,
/// non-tag-name str/sym, and nodes without a `value` pass through.
fn is_allowed_argument(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        // `variable?` — lvar/ivar/cvar/gvar.
        NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_) => true,
        // `send_type?`.
        NodeKind::Send { .. } => true,
        // `const_type?`.
        NodeKind::Const { .. } => true,
        // `splat_type?`.
        NodeKind::Splat(_) => true,
        NodeKind::Sym(sym) => !is_tag_name(cx.symbol_str(sym)),
        NodeKind::Str(sid) => !is_tag_name(cx.string_str(sid)),
        // `!argument.respond_to?(:value)` — nodes without scalar values
        // (array, hash, dstr, nil, true/false, ...) pass through.
        NodeKind::Int(_)
        | NodeKind::Float(_) => false,
        _ => true,
    }
}

/// Upstream `allowed_name?` (inverted): str/sym values that do NOT look like
/// tag names are allowed. Note the upstream character class excludes `_`:
/// `tag(:foo_bar)` passes while `tag("my-tag")` flags.
fn is_tag_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '-') {
        return false;
    }
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Approximate ActiveSupport `underscore` for tag names: namespace
/// separators, camel-case boundaries, and dashes.
fn underscore(value: &str) -> String {
    let namespaced = value.replace("::", "/");
    let mut result = String::with_capacity(namespaced.len() + 4);
    let bytes = namespaced.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_uppercase() {
            let prev_lower_or_digit = i > 0
                && (bytes[i - 1].is_ascii_lowercase() || bytes[i - 1].is_ascii_digit());
            let next_lower = i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase();
            let acronym_boundary = i > 0
                && bytes[i - 1].is_ascii_uppercase()
                && next_lower;
            if i > 0 && (prev_lower_or_digit || acronym_boundary) {
                result.push('_');
            }
            result.push(b.to_ascii_lowercase() as char);
        } else if b == b'-' {
            result.push('_');
        } else {
            result.push(b as char);
        }
        i += 1;
    }
    result
}

/// Mirrors upstream `corrected_ancestor?`: suppress when an ancestor `tag`
/// send would itself register an offense (upstream tracks the corrected set;
/// any offense-shaped ancestor is the conservative approximation).
fn has_offense_ancestor(cx: &Cx<'_>, node: NodeId) -> bool {
    for ancestor in cx.ancestors(node) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(ancestor) else {
            continue;
        };
        if cx.symbol_str(method) != "tag" || receiver.get().is_some() {
            continue;
        }
        let args = cx.call_arguments(ancestor);
        if args.len() >= 3 || args.is_empty() {
            continue;
        }
        if !is_allowed_argument(cx, args[0]) {
            return true;
        }
    }
    false
}

/// Upstream `correction_range`: selector begin through send end.
fn correction_range(cx: &Cx<'_>, node: NodeId) -> Range {
    Range {
        start: cx.loc(node).name.start,
        end: cx.range(node).end,
    }
}

#[cfg(test)]
mod tests {
    use super::ContentTag;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_tag_sym() {
        test::<ContentTag>().expect_offense(indoc! {r#"
            tag(:p)
            ^^^^^^^ Use `tag.p` instead of `tag(:p)`.
        "#});
    }

    #[test]
    fn flags_tag_sym_with_options() {
        test::<ContentTag>().expect_offense(indoc! {r#"
            tag(:br, class: 'classname')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `tag.br` instead of `tag(:br)`.
        "#});
    }

    #[test]
    fn flags_tag_dashed_string() {
        test::<ContentTag>().expect_offense(indoc! {r#"
            tag("my-tag")
            ^^^^^^^^^^^^^ Use `tag.my_tag` instead of `tag("my-tag")`.
        "#});
    }

    #[test]
    fn does_not_flag_variable_first_arg() {
        test::<ContentTag>().expect_no_offenses("tag(name, class: 'classname')\n");
    }

    #[test]
    fn does_not_flag_send_first_arg() {
        test::<ContentTag>().expect_no_offenses("tag(foo.bar)\n");
    }

    #[test]
    fn does_not_flag_const_first_arg() {
        test::<ContentTag>().expect_no_offenses("tag(FOO)\n");
    }

    #[test]
    fn does_not_flag_splat_first_arg() {
        test::<ContentTag>().expect_no_offenses("tag(*args)\n");
    }

    #[test]
    fn does_not_flag_three_args() {
        test::<ContentTag>().expect_no_offenses("tag(:p, :div, class: 'x')\n");
    }

    #[test]
    fn does_not_flag_good_form() {
        test::<ContentTag>().expect_no_offenses("tag.p\n");
        test::<ContentTag>().expect_no_offenses("tag.br(class: 'classname')\n");
    }

    #[test]
    fn flags_tag_camel_sym() {
        test::<ContentTag>().expect_offense(indoc! {r#"
            tag(:FooBar)
            ^^^^^^^^^^^^ Use `tag.foo_bar` instead of `tag(:FooBar)`.
        "#});
    }

    #[test]
    fn does_not_flag_underscore_sym() {
        // Upstream tag-name class excludes `_`.
        test::<ContentTag>().expect_no_offenses("tag(:foo_bar)\n");
    }

    #[test]
    fn corrects_tag_sym() {
        test::<ContentTag>()
            .expect_correction(
                indoc! {r#"
                    tag(:p)
                    ^^^^^^^ Use `tag.p` instead of `tag(:p)`.
                "#},
                "tag.p()\n",
            )
            .expect_no_offenses("tag.p()\n");
    }

    #[test]
    fn corrects_tag_camel_sym() {
        test::<ContentTag>()
            .expect_correction(
                indoc! {r#"
                    tag(:FooBar)
                    ^^^^^^^^^^^^ Use `tag.foo_bar` instead of `tag(:FooBar)`.
                "#},
                "tag.foo_bar()\n",
            )
            .expect_no_offenses("tag.foo_bar()\n");
    }

    #[test]
    fn corrects_tag_sym_with_options() {
        test::<ContentTag>()
            .expect_correction(
                indoc! {r#"
                    tag(:br, class: 'classname')
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `tag.br` instead of `tag(:br)`.
                "#},
                "tag.br(class: 'classname')\n",
            )
            .expect_no_offenses("tag.br(class: 'classname')\n");
    }
}
murphy_plugin_api::submit_cop!(ContentTag);
