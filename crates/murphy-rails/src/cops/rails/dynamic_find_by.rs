//! `Rails/DynamicFindBy` — flag dynamic `find_by_*` methods.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DynamicFindBy
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `METHOD_PATTERN /^find_by_(.+?)(!)?$/`,
//!   `IGNORED_ARGUMENT_TYPES [hash splat]`, bare-receiver
//!   `inherit_active_record_base?` gate (`ApplicationRecord` /
//!   `ActiveRecord::Base` ancestors), `AllowedMethods` / `AllowedReceivers` /
//!   `Whitelist` (deprecated) allow-lists, column-count and arg-type gates,
//!   whole-send offense with `Use \`static\` instead of dynamic \`method\``
//!   message, and autocorrect (selector → static + `kw: ` inserts).
//!   `on_csend` mirrors `alias on_csend on_send`.
//! ```
//!
//! ## Matched shapes
//!
//! - `User.find_by_name(name)` → `User.find_by(name: name)`.
//! - `User.find_by_name_and_email(a, b)` → `User.find_by(name: a, email: b)`.
//! - `User.find_by_email!(x)` → `User.find_by!(email: x)`.
//!
//! `User.find_by(name: name)`, `User.find_by_sql(q)`, and
//! `Gem::Specification.find_by_name(x)` do not flag.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DynamicFindBy;

#[derive(CopOptions)]
pub struct DynamicFindByOptions {
    #[option(
        name = "AllowedMethods",
        default = ["find_by_sql", "find_by_token_for"],
        description = "Methods ignored by the cop (dynamic finders that are allowed)."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedReceivers",
        default = ["Gem::Specification", "page"],
        description = "Receivers ignored by the cop (e.g. Gem::Specification, page)."
    )]
    pub allowed_receivers: Vec<String>,
    #[option(
        name = "Whitelist",
        default = ["find_by_sql", "find_by_token_for"],
        description = "Deprecated alias of AllowedMethods."
    )]
    pub whitelist: Vec<String>,
}

#[cop(
    name = "Rails/DynamicFindBy",
    description = "Use `find_by` instead of dynamic `find_by_*`.",
    default_severity = "warning",
    default_enabled = true,
    options = DynamicFindByOptions,
)]
impl DynamicFindBy {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let static_name = match static_method_name(&method) {
        Some(s) => s,
        None => return,
    };
    // Receiver gate: bare call only flags inside ActiveRecord models.
    // Upstream: `return if (receiver.nil? && !inherit_active_record_base?)`.
    let receiver_opt = cx.call_receiver(node).get();
    if receiver_opt.is_none() && !inherits_active_record(cx, node) {
        return;
    }
    if allowed_invocation(cx, node, &method, receiver_opt) {
        return;
    }
    let columns = match column_keywords(&method) {
        Some(c) => c,
        None => return,
    };
    let args = cx.call_arguments(node);
    if columns.len() != args.len() {
        return;
    }
    for &arg in args {
        if matches!(*cx.kind(arg), NodeKind::Hash(_) | NodeKind::Splat(_)) {
            return;
        }
    }
    let msg = format!("Use `{static_name}` instead of dynamic `{method}`.");
    cx.emit_offense(cx.range(node), &msg, None);
    // Autocorrect: selector → static, `kw: ` inserts before each arg.
    cx.emit_edit(cx.selector(node), static_name);
    for (kw, &arg) in columns.iter().zip(args.iter()) {
        let pos = cx.range(arg).start;
        cx.emit_edit(Range { start: pos, end: pos }, kw);
    }
}

/// Upstream `METHOD_PATTERN = /^find_by_(.+?)(!)?$/`.
/// Returns `find_by!` when trailing `!`, else `find_by`.
fn static_method_name(method: &str) -> Option<&'static str> {
    if !method.starts_with("find_by_") {
        return None;
    }
    let rest = &method["find_by_".len()..];
    if rest.is_empty() {
        return None;
    }
    if let Some(stripped) = rest.strip_suffix('!') {
        if stripped.is_empty() {
            return None;
        }
        Some("find_by!")
    } else {
        Some("find_by")
    }
}

/// Upstream `column_keywords`: `method[METHOD_PATTERN, 1].split('_and_')`
/// mapped to `"#{kw}: "`.
fn column_keywords(method: &str) -> Option<Vec<String>> {
    if !method.starts_with("find_by_") {
        return None;
    }
    let mut rest = &method["find_by_".len()..];
    if let Some(stripped) = rest.strip_suffix('!') {
        rest = stripped;
    }
    if rest.is_empty() {
        return None;
    }
    Some(rest.split("_and_").map(|kw| format!("{kw}: ")).collect())
}

fn allowed_invocation(
    cx: &Cx<'_>,
    _node: NodeId,
    method: &str,
    receiver_opt: Option<NodeId>,
) -> bool {
    let opts = cx.options_or_default::<DynamicFindByOptions>();
    if opts.allowed_methods.contains(&method.to_owned()) {
        return true;
    }
    if opts.whitelist.contains(&method.to_owned()) {
        return true;
    }
    if let Some(recv) = receiver_opt {
        let src = cx.raw_source(cx.range(recv)).to_owned();
        if opts.allowed_receivers.contains(&src) {
            return true;
        }
    }
    false
}

/// Upstream `inherit_active_record_base?`: any `class` ancestor whose
/// superclass is `ApplicationRecord` or `ActiveRecord::Base`.
fn inherits_active_record(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        let NodeKind::Class { superclass, .. } = *cx.kind(anc) else {
            continue;
        };
        let Some(super_id) = superclass.get() else {
            continue;
        };
        let name = cx.const_name(super_id).unwrap_or_default();
        if name == "ApplicationRecord" || name == "ActiveRecord::Base" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{DynamicFindBy, DynamicFindByOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_find_by_name() {
        test::<DynamicFindBy>().expect_offense(indoc! {r#"
            User.find_by_name(name)
            ^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name`.
        "#});
    }

    #[test]
    fn flags_find_by_name_and_email() {
        test::<DynamicFindBy>().expect_offense(indoc! {r#"
            User.find_by_name_and_email(a, b)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name_and_email`.
        "#});
    }

    #[test]
    fn flags_bang() {
        test::<DynamicFindBy>().expect_offense(indoc! {r#"
            User.find_by_email!(x)
            ^^^^^^^^^^^^^^^^^^^^^^ Use `find_by!` instead of dynamic `find_by_email!`.
        "#});
    }

    #[test]
    fn does_not_flag_static() {
        test::<DynamicFindBy>().expect_no_offenses("User.find_by(name: name)\n");
    }

    #[test]
    fn does_not_flag_allowed_method() {
        test::<DynamicFindBy>().expect_no_offenses("User.find_by_sql(q)\n");
    }

    #[test]
    fn does_not_flag_allowed_receiver_gem() {
        test::<DynamicFindBy>()
            .expect_no_offenses("Gem::Specification.find_by_name('backend')\n");
    }

    #[test]
    fn does_not_flag_allowed_receiver_page() {
        test::<DynamicFindBy>().expect_no_offenses("page.find_by_id('a')\n");
    }

    #[test]
    fn does_not_flag_bare_outside_ar() {
        test::<DynamicFindBy>().expect_no_offenses("find_by_name(name)\n");
    }

    #[test]
    fn flags_bare_inside_ar() {
        test::<DynamicFindBy>().expect_offense(indoc! {r#"
            class Foo < ApplicationRecord
              find_by_name(name)
              ^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name`.
            end
        "#});
    }

    #[test]
    fn does_not_flag_hash_arg() {
        test::<DynamicFindBy>().expect_no_offenses("User.find_by_name(name: name)\n");
    }

    #[test]
    fn does_not_flag_splat_arg() {
        test::<DynamicFindBy>().expect_no_offenses("User.find_by_name(*args)\n");
    }

    #[test]
    fn does_not_flag_count_mismatch() {
        test::<DynamicFindBy>()
            .expect_no_offenses("User.find_by_name_and_email(a)\n");
    }

    #[test]
    fn does_not_flag_plain_find_by() {
        test::<DynamicFindBy>().expect_no_offenses("User.find_by_sql(users_sql)\n");
    }

    #[test]
    fn flags_csend() {
        test::<DynamicFindBy>().expect_offense(indoc! {r#"
            User&.find_by_name(name)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name`.
        "#});
    }

    #[test]
    fn corrects_single() {
        test::<DynamicFindBy>()
            .expect_correction(
                indoc! {r#"
                    User.find_by_name(name)
                    ^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name`.
                "#},
                "User.find_by(name: name)\n",
            )
            .expect_no_offenses("User.find_by(name: name)\n");
    }

    #[test]
    fn corrects_multi() {
        test::<DynamicFindBy>()
            .expect_correction(
                indoc! {r#"
                    User.find_by_name_and_email(a, b)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of dynamic `find_by_name_and_email`.
                "#},
                "User.find_by(name: a, email: b)\n",
            )
            .expect_no_offenses("User.find_by(name: a, email: b)\n");
    }

    #[test]
    fn corrects_bang() {
        test::<DynamicFindBy>()
            .expect_correction(
                indoc! {r#"
                    User.find_by_email!(x)
                    ^^^^^^^^^^^^^^^^^^^^^^ Use `find_by!` instead of dynamic `find_by_email!`.
                "#},
                "User.find_by!(email: x)\n",
            )
            .expect_no_offenses("User.find_by!(email: x)\n");
    }

    #[test]
    fn custom_allowed_method() {
        let opts = DynamicFindByOptions {
            allowed_methods: vec!["find_by_name".to_owned()],
            ..Default::default()
        };
        test::<DynamicFindBy>()
            .with_options(&opts)
            .expect_no_offenses("User.find_by_name(name)\n");
    }

    #[test]
    fn custom_allowed_receiver() {
        let opts = DynamicFindByOptions {
            allowed_receivers: vec!["User".to_owned()],
            ..Default::default()
        };
        test::<DynamicFindBy>()
            .with_options(&opts)
            .expect_no_offenses("User.find_by_name(name)\n");
    }
}
murphy_plugin_api::submit_cop!(DynamicFindBy);
