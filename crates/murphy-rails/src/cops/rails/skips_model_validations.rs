//! `Rails/SkipsModelValidations` — avoid methods that skip validations.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SkipsModelValidations
//! upstream_version_checked: 2.35.0
//! version_added: "0.47"
//! safe: false
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (send + csend): flags
//!   `ForbiddenMethods` (default: the 18-method upstream list) unless the
//!   method is also in `AllowedMethods`, the call has no arguments while
//!   the method requires one (`METHODS_WITH_ARGUMENTS`), the call is a
//!   benign `FileUtils.touch` / `::FileUtils.touch` or `touch` with a
//!   single boolean literal, or `insert`/`insert!` looks like
//!   `String#insert`/`Array#insert` (non-hash second arg) or carries a
//!   non-`:returning`/`:unique_by` hash key. Offense is the selector;
//!   no autocorrect. Unsafe upstream (`Safe: false`). Obsolete
//!   `Blacklist`/`Whitelist` keys are not supported.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Methods that must receive at least one argument to flag (upstream
/// `METHODS_WITH_ARGUMENTS`).
const METHODS_WITH_ARGUMENTS: &[&str] = &[
    "decrement!",
    "decrement_counter",
    "increment!",
    "increment_counter",
    "insert",
    "insert!",
    "insert_all",
    "insert_all!",
    "toggle!",
    "update_all",
    "update_attribute",
    "update_column",
    "update_columns",
    "update_counters",
    "upsert",
    "upsert_all",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SkipsModelValidations;

#[derive(CopOptions)]
pub struct SkipsModelValidationsOptions {
    #[option(
        name = "ForbiddenMethods",
        default = [
            "decrement!",
            "decrement_counter",
            "increment!",
            "increment_counter",
            "insert",
            "insert!",
            "insert_all",
            "insert_all!",
            "toggle!",
            "touch",
            "touch_all",
            "update_all",
            "update_attribute",
            "update_column",
            "update_columns",
            "update_counters",
            "upsert",
            "upsert_all"
        ],
        description = "Methods that skip validations and should be avoided."
    )]
    pub forbidden_methods: Vec<String>,
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods ignored by the cop, superseding ForbiddenMethods."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Rails/SkipsModelValidations",
    description = "Use methods that skip model validations with caution.",
    default_severity = "warning",
    default_enabled = true,
    options = SkipsModelValidationsOptions,
)]
impl SkipsModelValidations {
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
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let opts = cx.options_or_default::<SkipsModelValidationsOptions>();
    if opts.allowed_methods.iter().any(|m| m == &method) {
        return;
    }
    // An explicitly emptied `ForbiddenMethods` disables the cop; the
    // derive default otherwise supplies the upstream list.
    if opts.forbidden_methods.is_empty() {
        return;
    }
    if !opts.forbidden_methods.iter().any(|m| m == &method) {
        return;
    }
    // `allowed_method?`: methods requiring arguments called bare are fine.
    if METHODS_WITH_ARGUMENTS.contains(&method.as_str()) && cx.call_arguments(node).is_empty() {
        return;
    }
    if is_good_touch(cx, node, &method) {
        return;
    }
    if is_good_insert(cx, node, &method) {
        return;
    }
    cx.emit_offense(
        cx.selector(node),
        &format!("Avoid using `{method}` because it skips validations."),
        None,
    );
}

/// Upstream `good_touch?`: `FileUtils.touch` / `::FileUtils.touch`, or
/// `touch` with a single boolean literal argument.
fn is_good_touch(cx: &Cx<'_>, node: NodeId, method: &str) -> bool {
    if method != "touch" {
        return false;
    }
    if let Some(recv) = cx.call_receiver(node).get()
        && cx.const_name(recv).as_deref() == Some("FileUtils")
    {
        return true;
    }
    let args = cx.call_arguments(node);
    args.len() == 1
        && matches!(
            *cx.kind(args[0]),
            NodeKind::True_ | NodeKind::False_
        )
}

/// Upstream `good_insert?` for `insert`/`insert!`: a non-hash second
/// argument (`String#insert`/`Array#insert` shape), or a hash containing a
/// key other than `:returning`/`:unique_by`.
fn is_good_insert(cx: &Cx<'_>, node: NodeId, method: &str) -> bool {
    if method != "insert" && method != "insert!" {
        return false;
    }
    let args = cx.call_arguments(node);
    if args.len() < 2 {
        return false;
    }
    let second = args[1];
    if !matches!(*cx.kind(second), NodeKind::Hash(_)) {
        return true;
    }
    for pair in cx.hash_pairs(second) {
        let Some(key) = cx.pair_key(pair).get() else {
            continue;
        };
        if !matches!(*cx.kind(key), NodeKind::Sym(_)) {
            continue;
        }
        let NodeKind::Sym(sym) = *cx.kind(key) else {
            continue;
        };
        let name = cx.symbol_str(sym);
        if name != "returning" && name != "unique_by" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{SkipsModelValidations, SkipsModelValidationsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn forbidden() -> SkipsModelValidationsOptions {
        SkipsModelValidationsOptions {
            forbidden_methods: [
                "decrement!",
                "decrement_counter",
                "increment!",
                "increment_counter",
                "insert",
                "insert!",
                "insert_all",
                "insert_all!",
                "toggle!",
                "touch",
                "touch_all",
                "update_all",
                "update_attribute",
                "update_column",
                "update_columns",
                "update_counters",
                "upsert",
                "upsert_all",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            allowed_methods: Vec::new(),
        }
    }

    #[test]
    fn flags_with_default_options() {
        // The derive default carries the upstream ForbiddenMethods list.
        test::<SkipsModelValidations>().expect_offense(
            indoc! {r#"
                user.update_attribute(:website, 'example.com')
                     ^^^^^^^^^^^^^^^^ Avoid using `update_attribute` because it skips validations.
            "#},
        );
    }

    #[test]
    fn flags_update_attribute() {
        test::<SkipsModelValidations>().with_options(&forbidden()).expect_offense(
            indoc! {r#"
                user.update_attribute(:website, 'example.com')
                     ^^^^^^^^^^^^^^^^ Avoid using `update_attribute` because it skips validations.
            "#},
        );
    }

    #[test]
    fn flags_update_attribute_csend() {
        test::<SkipsModelValidations>().with_options(&forbidden()).expect_offense(
            indoc! {r#"
                user&.update_attribute(:website, 'example.com')
                      ^^^^^^^^^^^^^^^^ Avoid using `update_attribute` because it skips validations.
            "#},
        );
    }

    #[test]
    fn flags_bare_touch() {
        test::<SkipsModelValidations>().with_options(&forbidden()).expect_offense(
            indoc! {r#"
                User.touch(:attr)
                     ^^^^^ Avoid using `touch` because it skips validations.
            "#},
        );
    }

    #[test]
    fn allows_method_requiring_arguments_called_bare() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("User.update_attribute
");
    }

    #[test]
    fn allows_non_forbidden_method() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("user.update(website: 'example.com')
");
    }

    #[test]
    fn allows_file_utils_touch() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("FileUtils.touch('file')
");
    }

    #[test]
    fn allows_cbase_file_utils_touch() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("::FileUtils.touch('file')
");
    }

    #[test]
    fn allows_touch_with_boolean() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("belongs_to(:user).touch(true)
");
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("belongs_to(:user).touch(false)
");
    }

    #[test]
    fn allows_string_like_insert() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("string.insert(0, 'b')
");
    }

    #[test]
    fn allows_insert_with_other_key() {
        test::<SkipsModelValidations>()
            .with_options(&forbidden())
            .expect_no_offenses("insert(attributes, something_else: true)
");
    }

    #[test]
    fn flags_insert_with_returning() {
        test::<SkipsModelValidations>().with_options(&forbidden()).expect_offense(
            indoc! {r#"
                insert(attributes, returning: false)
                ^^^^^^ Avoid using `insert` because it skips validations.
            "#},
        );
    }

    #[test]
    fn flags_toggle_bang_but_allows_allowed_method() {
        let opts = SkipsModelValidationsOptions {
            forbidden_methods: vec!["toggle!".to_string(), "touch".to_string()],
            allowed_methods: vec!["touch".to_string()],
        };
        test::<SkipsModelValidations>().with_options(&opts).expect_offense(
            indoc! {r#"
                user.toggle!(:active)
                     ^^^^^^^ Avoid using `toggle!` because it skips validations.
            "#},
        );
        test::<SkipsModelValidations>()
            .with_options(&opts)
            .expect_no_offenses("User.touch(:attr)
");
    }
}
murphy_plugin_api::submit_cop!(SkipsModelValidations);
