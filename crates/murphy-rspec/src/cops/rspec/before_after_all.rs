//! `RSpec/BeforeAfterAll` — avoid `before(:all)` / `after(:context)`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/BeforeAfterAll
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `before_or_after_all` (`$(send _ {:before :after}
//!   (sym {:all :context}))`) with `RESTRICT_ON_SEND = [:before, :after]`.
//!   Upstream matches any receiver (`_`); so does this port —
//!   `RSpec.before(:all)` is still the RSpec hook. The offense range is the
//!   hook `Send` (trimmed to exclude a wrapping block via
//!   `send_without_block_range`, since Murphy's `Send` range covers the
//!   block while RuboCop's does not). The message interpolates the hook
//!   source (`before(:all)`) per upstream `format(MSG, hook: hook.source)`.
//!   File scoping (`spec_helper` / `rails_helper` / `support/**` Excludes)
//!   is layered via the pack `config/default.yml`. No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["before", "after"]`. Flags when
//! the first positional arg is `:all` or `:context`:
//!
//! - `before(:all) { }` — flagged.
//! - `after(:context) { }` — flagged.
//! - `before(:each) { }` — each-example hook, not flagged.
//! - `before { }` — implicit each hook, not flagged.
//! - `around(:all) { }` — different selector, not flagged (dispatched only
//!   on `before` / `after`).
//!
//! ## File scoping
//!
//! `spec/spec_helper.rb`, `spec/rails_helper.rb`, and `spec/support/**`
//! are excluded via the pack bundled defaults (mirrors upstream
//! `config/default.yml`).
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; switching to `:each` needs human
//! judgement about shared state.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::send_without_block_range;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BeforeAfterAll;

#[cop(
    name = "RSpec/BeforeAfterAll",
    description = "Check that before/after(:all/:context) is not used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BeforeAfterAll {
    #[on_node(kind = "send", methods = ["before", "after"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        let NodeKind::Sym(sym) = *cx.kind(first) else {
            return;
        };
        let scope = cx.symbol_str(sym);
        if scope != "all" && scope != "context" {
            return;
        }
        let hook_src = cx.raw_source(send_without_block_range(cx, node));
        cx.emit_offense(
            send_without_block_range(cx, node),
            &format!(
                "Beware of using `{hook_src}` as it may cause state to leak between tests. \
                If you are using `rspec-rails`, and `use_transactional_fixtures` is enabled, \
                then records created in `{hook_src}` are not automatically rolled back."
            ),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::BeforeAfterAll;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_before_all() {
        test::<BeforeAfterAll>().expect_offense(indoc! {r#"
                before(:all) { }
                ^^^^^^^^^^^^ Beware of using `before(:all)` as it may cause state to leak between tests. If you are using `rspec-rails`, and `use_transactional_fixtures` is enabled, then records created in `before(:all)` are not automatically rolled back.
            "#});
    }

    #[test]
    fn flags_after_context() {
        test::<BeforeAfterAll>().expect_offense(indoc! {r#"
                after(:context) { }
                ^^^^^^^^^^^^^^^ Beware of using `after(:context)` as it may cause state to leak between tests. If you are using `rspec-rails`, and `use_transactional_fixtures` is enabled, then records created in `after(:context)` are not automatically rolled back.
            "#});
    }

    #[test]
    fn flags_after_all_do_end() {
        test::<BeforeAfterAll>().expect_offense(indoc! {r#"
                after(:all) do
                ^^^^^^^^^^^ Beware of using `after(:all)` as it may cause state to leak between tests. If you are using `rspec-rails`, and `use_transactional_fixtures` is enabled, then records created in `after(:all)` are not automatically rolled back.
                end
            "#});
    }

    #[test]
    fn does_not_flag_before_each() {
        test::<BeforeAfterAll>().expect_no_offenses(indoc! {r#"
                before(:each) { }
            "#});
    }

    #[test]
    fn does_not_flag_bare_before() {
        test::<BeforeAfterAll>().expect_no_offenses(indoc! {r#"
                before { }
            "#});
    }

    #[test]
    fn does_not_flag_around_all() {
        // `around` is a different selector; this cop only watches before/after.
        test::<BeforeAfterAll>().expect_no_offenses(indoc! {r#"
                around(:all) { }
            "#});
    }
}

murphy_plugin_api::submit_cop!(BeforeAfterAll);
