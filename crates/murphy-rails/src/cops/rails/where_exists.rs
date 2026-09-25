//! `Rails/WhereExists` — prefer `exists?` over `where(...).exists?` (or vice versa).
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereExists
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! version_changed: "2.10"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_csend` with
//!   RESTRICT_ON_SEND [:exists?] and `EnforcedStyle: exists` (default) /
//!   `where`. Exists style flags `where(...).exists?` (no-arg `exists?`)
//!   with convertible where args (multi-arg or single Hash/Array) and
//!   corrects `where(...).exists?` -> `exists?(...)` (multi-arg wrapped in
//!   `[...]`). Where style flags single non-splat `exists?(arg)` with
//!   Hash/Array arg and corrects to `where(...).exists?` preserving
//!   `.`/`&.` via the call operator. Disabled upstream by default
//!   (`Enabled: pending`, `SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereExists;

#[derive(CopOptions)]
pub struct WhereExistsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "exists",
        description = "Whether to enforce `exists?` or `where(...).exists?` style."
    )]
    pub enforced_style: WhereExistsStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum WhereExistsStyle {
    #[option(value = "exists")]
    Exists,
    #[option(value = "where")]
    Where,
}

#[cop(
    name = "Rails/WhereExists",
    description = "Prefer `exists?(...)` over `where(...).exists?`.",
    default_severity = "warning",
    default_enabled = false,
    options = WhereExistsOptions,
)]
impl WhereExists {
    #[on_node(kind = "send", methods = ["exists?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<WhereExistsOptions>();
    if opts.enforced_style == WhereExistsStyle::Exists {
        check_exists_style(node, cx);
    } else {
        check_where_style(node, cx);
    }
}

fn check_exists_style(node: NodeId, cx: &Cx<'_>) {
    // Must be `exists?` with no args (outer).
    if cx.method_name(node) != Some("exists?") {
        return;
    }
    if !cx.call_arguments(node).is_empty() {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(recv) != Some("where") {
        return;
    }
    let where_args = cx.call_arguments(recv);
    if !is_convertible(where_args, cx) {
        return;
    }
    // Range: inner `where` selector start to outer `exists?` selector end
    // (== node end since outer has no args).
    let range = Range {
        start: cx.selector(recv).start,
        end: cx.selector(node).end,
    };
    let good = build_exists_good(cx, where_args);
    let bad_src = cx.raw_source(range).to_owned();
    let msg = format!("Prefer `{good}` over `{bad_src}`.");
    cx.emit_offense(range, &msg, None);
    cx.emit_edit(range, &good);
}

fn check_where_style(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("exists?") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let arg = args[0];
    // `$!splat_type?` — single non-splat arg.
    if matches!(*cx.kind(arg), NodeKind::Splat(_)) {
        return;
    }
    if !is_convertible(&[arg], cx) {
        // For single arg, convertible means Hash or Array.
        return;
    }
    let range = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    let dot_src = cx
        .call_operator_loc(node)
        .map(|r| cx.raw_source(r).to_owned())
        .unwrap_or_else(|| ".".to_owned());
    let arg_src = cx.raw_source(cx.range(arg)).to_owned();
    let good = format!("where({arg_src}){dot_src}exists?");
    let bad_src = cx.raw_source(range).to_owned();
    let msg = format!("Prefer `{good}` over `{bad_src}`.");
    cx.emit_offense(range, &msg, None);
    cx.emit_edit(range, &good);
}

fn is_convertible(args: &[NodeId], cx: &Cx<'_>) -> bool {
    if args.is_empty() {
        return false;
    }
    if args.len() > 1 {
        return true;
    }
    matches!(
        *cx.kind(args[0]),
        NodeKind::Hash(_) | NodeKind::Array(_)
    )
}

fn build_exists_good(cx: &Cx<'_>, where_args: &[NodeId]) -> String {
    if where_args.len() > 1 {
        let parts: Vec<String> = where_args
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_owned())
            .collect();
        format!("exists?([{}])", parts.join(", "))
    } else {
        let src = cx.raw_source(cx.range(where_args[0])).to_owned();
        format!("exists?({src})")
    }
}

#[cfg(test)]
mod tests {
    use super::{WhereExists, WhereExistsOptions, WhereExistsStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn exists_style() -> WhereExistsOptions {
        WhereExistsOptions {
            enforced_style: WhereExistsStyle::Exists,
        }
    }

    fn where_style() -> WhereExistsOptions {
        WhereExistsOptions {
            enforced_style: WhereExistsStyle::Where,
        }
    }

    #[test]
    fn exists_flags_where_hash() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    User.where(name: 'john').exists?
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(name: 'john')` over `where(name: 'john').exists?`.
                "#},
                "User.exists?(name: 'john')\n",
            );
    }

    #[test]
    fn exists_flags_where_csend() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    User.where(name: 'john')&.exists?
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(name: 'john')` over `where(name: 'john')&.exists?`.
                "#},
                "User.exists?(name: 'john')\n",
            );
    }

    #[test]
    fn exists_flags_csend_where_csend_exists() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    User&.where(name: 'john')&.exists?
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(name: 'john')` over `where(name: 'john')&.exists?`.
                "#},
                "User&.exists?(name: 'john')\n",
            );
    }

    #[test]
    fn exists_flags_where_array() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    User.where(['name = ?', 'john']).exists?
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(['name = ?', 'john'])` over `where(['name = ?', 'john']).exists?`.
                "#},
                "User.exists?(['name = ?', 'john'])\n",
            );
    }

    #[test]
    fn exists_flags_where_multi_arg() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    User.where('name = ?', 'john').exists?
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(['name = ?', 'john'])` over `where('name = ?', 'john').exists?`.
                "#},
                "User.exists?(['name = ?', 'john'])\n",
            );
    }

    #[test]
    fn exists_flags_bare_where_exists() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    where('name = ?', 'john').exists?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(['name = ?', 'john'])` over `where('name = ?', 'john').exists?`.
                "#},
                "exists?(['name = ?', 'john'])\n",
            );
    }

    #[test]
    fn exists_flags_association() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_correction(
                indoc! {r#"
                    user.posts.where(published: true).exists?
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `exists?(published: true)` over `where(published: true).exists?`.
                "#},
                "user.posts.exists?(published: true)\n",
            );
    }

    #[test]
    fn exists_no_offense_string_arg() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_no_offenses("User.where(\"name = 'john'\").exists?\n");
    }

    #[test]
    fn exists_no_offense_bare_exists() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_no_offenses("User.exists?(name: 'john')\n");
    }

    #[test]
    fn exists_no_offense_no_args() {
        test::<WhereExists>()
            .with_options(&exists_style())
            .expect_no_offenses("User.exists?\n");
    }

    #[test]
    fn where_flags_exists_hash() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_correction(
                indoc! {r#"
                    User.exists?(name: 'john')
                         ^^^^^^^^^^^^^^^^^^^^^ Prefer `where(name: 'john').exists?` over `exists?(name: 'john')`.
                "#},
                "User.where(name: 'john').exists?\n",
            );
    }

    #[test]
    fn where_flags_exists_csend() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_correction(
                indoc! {r#"
                    User&.exists?(name: 'john')
                          ^^^^^^^^^^^^^^^^^^^^^ Prefer `where(name: 'john')&.exists?` over `exists?(name: 'john')`.
                "#},
                "User&.where(name: 'john')&.exists?\n",
            );
    }

    #[test]
    fn where_flags_exists_array() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_correction(
                indoc! {r#"
                    User.exists?(['name = ?', 'john'])
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `where(['name = ?', 'john']).exists?` over `exists?(['name = ?', 'john'])`.
                "#},
                "User.where(['name = ?', 'john']).exists?\n",
            );
    }

    #[test]
    fn where_no_offense_multi_arg() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_no_offenses("User.exists?('name = ?', 'john')\n");
    }

    #[test]
    fn where_no_offense_splat() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_no_offenses("User.exists?(*conditions)\n");
    }

    #[test]
    fn where_flags_bare_exists_array() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_correction(
                indoc! {r#"
                    exists?(['name = ?', 'john'])
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `where(['name = ?', 'john']).exists?` over `exists?(['name = ?', 'john'])`.
                "#},
                "where(['name = ?', 'john']).exists?\n",
            );
    }

    #[test]
    fn where_flags_association() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_correction(
                indoc! {r#"
                    user.posts.exists?(published: true)
                               ^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `where(published: true).exists?` over `exists?(published: true)`.
                "#},
                "user.posts.where(published: true).exists?\n",
            );
    }

    #[test]
    fn where_no_offense_where_exists() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_no_offenses("User.where(name: 'john').exists?\n");
    }

    #[test]
    fn where_no_offense_no_args() {
        test::<WhereExists>()
            .with_options(&where_style())
            .expect_no_offenses("User.exists?\n");
    }
}

murphy_plugin_api::submit_cop!(WhereExists);
