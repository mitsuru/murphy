//! `Lint/UriRegexp` — Checks for calls to `URI.regexp` which is obsolete.
//!
//! The correct replacement is `URI::DEFAULT_PARSER.make_regexp` (Ruby 3.3 or lower)
//! or `URI::RFC2396_PARSER.make_regexp` (Ruby 3.4 or higher).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UriRegexp
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags all `URI.regexp` and `::URI.regexp` calls. No autocorrect
//!   (the correct replacement depends on Ruby version and the fix is
//!   not safe to automate mechanically).
//! ```
//!
//! ## Matched shapes
//!
//! - `URI.regexp(...)` — with or without arguments
//! - `::URI.regexp(...)` — cbase form
//!
//! ## No autocorrect
//!
//! The replacement depends on the target Ruby version
//! (`URI::DEFAULT_PARSER.make_regexp` vs `URI::RFC2396_PARSER.make_regexp`)
//! and is not safe to automate.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// RuboCop parity: `Lint/UriRegexp` matcher is
// `(send (const {nil? cbase} :URI) :regexp ...)`. In Murphy `::URI`
// collapses to `Const{scope:None}`, so a single `nil?` scope covers bare and
// `::`-prefixed forms — equivalent to the prior `is_global_const` check.
def_node_matcher!(uri_regexp, "(send (const nil? :URI) :regexp ...)");

const MSG: &str = "`URI.regexp` is obsolete and should not be used. Instead, use `URI::DEFAULT_PARSER.make_regexp` (Ruby <= 3.3) or `URI::RFC2396_PARSER.make_regexp` (Ruby >= 3.4).";

#[derive(Default)]
pub struct UriRegexp;

#[cop(
    name = "Lint/UriRegexp",
    description = "Checks for calls to `URI.regexp` which is obsolete.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UriRegexp {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send (const nil? :URI) :regexp ...)` — top-level `URI.regexp`.
        if !uri_regexp(node, cx) {
            return;
        }
        cx.emit_offense(cx.node(node).loc.name, MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::UriRegexp;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_uri_regexp_with_argument() {
        test::<UriRegexp>().expect_offense(indoc! {r#"
            URI.regexp('http://example.com')
                ^^^^^^ `URI.regexp` is obsolete and should not be used. Instead, use `URI::DEFAULT_PARSER.make_regexp` (Ruby <= 3.3) or `URI::RFC2396_PARSER.make_regexp` (Ruby >= 3.4).
        "#});
    }

    #[test]
    fn flags_uri_regexp_without_argument() {
        test::<UriRegexp>().expect_offense(indoc! {r#"
            URI.regexp
                ^^^^^^ `URI.regexp` is obsolete and should not be used. Instead, use `URI::DEFAULT_PARSER.make_regexp` (Ruby <= 3.3) or `URI::RFC2396_PARSER.make_regexp` (Ruby >= 3.4).
        "#});
    }

    #[test]
    fn flags_cbase_uri_regexp() {
        test::<UriRegexp>().expect_offense(indoc! {r#"
            ::URI.regexp('http://example.com')
                  ^^^^^^ `URI.regexp` is obsolete and should not be used. Instead, use `URI::DEFAULT_PARSER.make_regexp` (Ruby <= 3.3) or `URI::RFC2396_PARSER.make_regexp` (Ruby >= 3.4).
        "#});
    }

    #[test]
    fn flags_uri_regexp_with_array_argument() {
        test::<UriRegexp>().expect_offense(indoc! {r#"
            URI.regexp(['http', 'https'])
                ^^^^^^ `URI.regexp` is obsolete and should not be used. Instead, use `URI::DEFAULT_PARSER.make_regexp` (Ruby <= 3.3) or `URI::RFC2396_PARSER.make_regexp` (Ruby >= 3.4).
        "#});
    }

    #[test]
    fn does_not_flag_regexp_without_uri_receiver() {
        test::<UriRegexp>().expect_no_offenses(indoc! {"
            regexp('http://example.com')
        "});
    }

    #[test]
    fn does_not_flag_regexp_with_variable_receiver() {
        test::<UriRegexp>().expect_no_offenses(indoc! {"
            m.regexp('http://example.com')
        "});
    }

    #[test]
    fn does_not_flag_other_method_on_uri() {
        test::<UriRegexp>().expect_no_offenses(indoc! {"
            URI.parse('http://example.com')
        "});
    }

    // --- Boundary characterization (murphy-ft88): pin the exact node set
    // the hand-rolled `is_global_const` predicate matches, so the
    // `(send (const nil? :URI) :regexp ...)` refactor can be proven
    // equivalent.

    #[test]
    fn boundary_ignores_namespaced_uri() {
        // `Foo::URI` has a non-nil const scope, so it is not flagged.
        test::<UriRegexp>().expect_no_offenses("Foo::URI.regexp('x')\n");
    }

    #[test]
    fn boundary_ignores_csend() {
        // `&.` is a `csend` node, not `send`; RuboCop's `(send ...)`
        // pattern does not match it.
        test::<UriRegexp>().expect_no_offenses("URI&.regexp('x')\n");
    }
}

murphy_plugin_api::submit_cop!(UriRegexp);
