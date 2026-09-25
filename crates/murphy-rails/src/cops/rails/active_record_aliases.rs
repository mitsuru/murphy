//! `Rails/ActiveRecordAliases` — flag `update_attributes` in favour of `update`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActiveRecordAliases
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND gating, non-empty-args
//!   guard, selector-only offense range, and selector-rename autocorrect.
//!   Both Send and Csend dispatch (alias on_csend on_send upstream).
//! ```
//!
//! ## Matched shapes
//!
//! `Send`/`Csend` with method `update_attributes`/`update_attributes!`
//! and at least one argument. Receiver shape is unconstrained (upstream
//! has no receiver gate).
//!
//! - `book.update_attributes(author: "Alice")` → `book.update(author: "Alice")`
//! - `book&.update_attributes(author: "Alice")` → `book&.update(author: "Alice")`
//! - `book.update_attributes!(author: "Bob")` → `book.update!(author: "Bob")`
//!
//! Bare `update_attributes` with zero args (e.g. as a bare argument in
//! `user.update(update_attributes)`) does not flag — mirrors upstream's
//! `return if node.arguments.empty?`.
//!
//! ## Autocorrect
//!
//! Rename the selector only (`loc.name`): `update_attributes` → `update`,
//! `update_attributes!` → `update!`. Arguments and receiver pass through
//! untouched.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActiveRecordAliases;

#[cop(
    name = "Rails/ActiveRecordAliases",
    description = "Use `update` instead of `update_attributes`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ActiveRecordAliases {
    // Mirrors upstream `RESTRICT_ON_SEND = [:update_attributes, :update_attributes!]`
    // for Send dispatch. Csend cannot use `methods` filtering (macro restriction),
    // so it uses a bare kind subscription with a manual method check.
    #[on_node(kind = "send", methods = ["update_attributes", "update_attributes!"])]
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
        Some(m) => m,
        None => return,
    };
    let prefer = match method {
        "update_attributes" => "update",
        "update_attributes!" => "update!",
        _ => return,
    };
    // Upstream: `return if node.arguments.empty?`.
    if cx.call_arguments(node).is_empty() {
        return;
    }
    // Defensive: dispatcher guarantees Send/Csend, but keep let-else
    // insurance against future kind aliasing.
    match *cx.kind(node) {
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {}
        _ => return,
    }
    let msg = format!("Use `{prefer}` instead of `{method}`.");
    cx.emit_offense(cx.loc(node).name, &msg, None);
    cx.emit_edit(cx.loc(node).name, prefer);
}

#[cfg(test)]
mod tests {
    use super::ActiveRecordAliases;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_update_attributes() {
        test::<ActiveRecordAliases>().expect_offense(indoc! {r#"
            book.update_attributes(author: "Alice")
                 ^^^^^^^^^^^^^^^^^ Use `update` instead of `update_attributes`.
        "#});
    }

    #[test]
    fn flags_update_attributes_bang() {
        test::<ActiveRecordAliases>().expect_offense(indoc! {r#"
            book.update_attributes!(author: "Bob")
                 ^^^^^^^^^^^^^^^^^^ Use `update!` instead of `update_attributes!`.
        "#});
    }

    #[test]
    fn flags_csend_update_attributes() {
        test::<ActiveRecordAliases>().expect_offense(indoc! {r#"
            book&.update_attributes(author: "Alice")
                  ^^^^^^^^^^^^^^^^^ Use `update` instead of `update_attributes`.
        "#});
    }

    #[test]
    fn does_not_flag_bare_update_attributes_without_args() {
        // `update_attributes` as a bare zero-arg call (inner arg) has no
        // arguments, so it is excluded — mirrors upstream's empty-args guard.
        test::<ActiveRecordAliases>().expect_no_offenses("user.update(update_attributes)\n");
    }

    #[test]
    fn does_not_flag_update() {
        test::<ActiveRecordAliases>().expect_no_offenses("book.update(author: \"Alice\")\n");
    }

    #[test]
    fn does_not_flag_update_bang() {
        test::<ActiveRecordAliases>().expect_no_offenses("book.update!(author: \"Bob\")\n");
    }

    #[test]
    fn corrects_update_attributes_to_update() {
        test::<ActiveRecordAliases>()
            .expect_correction(
                indoc! {r#"
                    book.update_attributes(author: "Alice")
                         ^^^^^^^^^^^^^^^^^ Use `update` instead of `update_attributes`.
                "#},
                "book.update(author: \"Alice\")\n",
            )
            .expect_no_offenses("book.update(author: \"Alice\")\n");
    }

    #[test]
    fn corrects_update_attributes_bang_to_update_bang() {
        test::<ActiveRecordAliases>()
            .expect_correction(
                indoc! {r#"
                    book.update_attributes!(author: "Bob")
                         ^^^^^^^^^^^^^^^^^^ Use `update!` instead of `update_attributes!`.
                "#},
                "book.update!(author: \"Bob\")\n",
            )
            .expect_no_offenses("book.update!(author: \"Bob\")\n");
    }

    #[test]
    fn corrects_csend_update_attributes() {
        test::<ActiveRecordAliases>()
            .expect_correction(
                indoc! {r#"
                    book&.update_attributes(author: "Alice")
                          ^^^^^^^^^^^^^^^^^ Use `update` instead of `update_attributes`.
                "#},
                "book&.update(author: \"Alice\")\n",
            )
            .expect_no_offenses("book&.update(author: \"Alice\")\n");
    }
}
murphy_plugin_api::submit_cop!(ActiveRecordAliases);
