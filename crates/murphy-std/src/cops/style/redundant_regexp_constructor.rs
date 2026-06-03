//! `Style/RedundantRegexpConstructor` — flags `Regexp.new(/re/)` and
//! `Regexp.compile(/re/)` where the argument is already a regexp literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantRegexpConstructor
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers the primary case: Regexp.new(/re/) and Regexp.compile(/re/).
//!   Autocorrect preserves the inner regexp literal verbatim (including
//!   %r{} delimiters if used) rather than normalising to /…/ as RuboCop does.
//!   The cop is disabled by default (Enabled: pending in RuboCop's default.yml).
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `new` or `compile` whose receiver is the constant
//! `Regexp` (with `nil` or `cbase` scope — i.e. not `Foo::Regexp`), and which
//! have exactly one argument that is a `regexp` literal.
//!
//! ## Autocorrect
//!
//! Two surgical edits per autocorrect-pattern.md:
//! 1. Delete `Regexp.new(` — the bytes from the send's start to the inner
//!    regexp literal's start.
//! 2. Delete the closing `)` — the bytes from the inner literal's end to the
//!    send's end.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Remove the redundant `Regexp.%<method>s`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantRegexpConstructor;

#[cop(
    name = "Style/RedundantRegexpConstructor",
    description = "Checks for the instantiation of regexp using redundant `Regexp.new` or `Regexp.compile`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantRegexpConstructor {
    #[on_node(kind = "send", methods = ["new", "compile"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // Receiver must be `Regexp` constant with nil or cbase scope.
    let Some(recv_id) = receiver.get() else {
        return;
    };
    let NodeKind::Const { scope, name } = *cx.kind(recv_id) else {
        return;
    };
    if cx.symbol_str(name) != "Regexp" {
        return;
    }
    // Allow nil scope (bare `Regexp`) and cbase scope (`::Regexp`), but reject
    // namespaced constants like `Foo::Regexp`.
    if let Some(scope_id) = scope.get()
        && !matches!(cx.kind(scope_id), NodeKind::Cbase) {
            return;
        }

    // Exactly one argument that is a regexp literal.
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let arg_id = arg_list[0];
    if !matches!(cx.kind(arg_id), NodeKind::Regexp { .. }) {
        return;
    }

    let method_name = cx.symbol_str(method);
    let node_range = cx.range(node);
    let arg_range = cx.range(arg_id);

    let msg = MSG.replace("%<method>s", method_name);
    cx.emit_offense(node_range, &msg, None);

    // Autocorrect: two non-overlapping surgical edits.
    // Edit 1: delete `Regexp.new(` — from node start to arg start.
    cx.emit_edit(
        Range {
            start: node_range.start,
            end: arg_range.start,
        },
        "",
    );
    // Edit 2: delete the closing `)` — from arg end to node end.
    cx.emit_edit(
        Range {
            start: arg_range.end,
            end: node_range.end,
        },
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::RedundantRegexpConstructor;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_regexp_new_with_regexp_literal() {
        test::<RedundantRegexpConstructor>().expect_correction(
            indoc! {"
                Regexp.new(/regexp/)
                ^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Regexp.new`.
            "},
            "/regexp/\n",
        );
    }

    #[test]
    fn flags_regexp_compile_with_regexp_literal() {
        test::<RedundantRegexpConstructor>().expect_correction(
            indoc! {"
                Regexp.compile(/regexp/)
                ^^^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Regexp.compile`.
            "},
            "/regexp/\n",
        );
    }

    #[test]
    fn flags_regexp_new_preserving_flags() {
        test::<RedundantRegexpConstructor>().expect_correction(
            indoc! {"
                Regexp.new(/pat/ix)
                ^^^^^^^^^^^^^^^^^^^ Remove the redundant `Regexp.new`.
            "},
            "/pat/ix\n",
        );
    }

    #[test]
    fn flags_regexp_new_with_percent_r_delimiter() {
        // The %r{} delimiter is preserved verbatim (Murphy diverges from RuboCop
        // which normalises to /.../ -- documented in murphy-parity notes).
        test::<RedundantRegexpConstructor>().expect_correction(
            indoc! {"
                Regexp.new(%r{foo/bar})
                ^^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Regexp.new`.
            "},
            "%r{foo/bar}\n",
        );
    }

    #[test]
    fn flags_cbase_scoped_regexp() {
        // ::Regexp.new(/re/) is also redundant.
        test::<RedundantRegexpConstructor>().expect_correction(
            indoc! {"
                ::Regexp.new(/re/)
                ^^^^^^^^^^^^^^^^^^ Remove the redundant `Regexp.new`.
            "},
            "/re/\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_regexp_new_with_string_arg() {
        test::<RedundantRegexpConstructor>().expect_no_offenses("Regexp.new('regexp')\n");
    }

    #[test]
    fn accepts_regexp_new_with_no_args() {
        // Degenerate case -- not matched.
        test::<RedundantRegexpConstructor>().expect_no_offenses("Regexp.new\n");
    }

    #[test]
    fn accepts_regexp_new_with_two_args() {
        // Extra argument (e.g. Regexp::IGNORECASE) -- not matched.
        test::<RedundantRegexpConstructor>()
            .expect_no_offenses("Regexp.new(/re/, Regexp::IGNORECASE)\n");
    }

    #[test]
    fn accepts_namespaced_regexp() {
        // `Foo::Regexp.new(/re/)` -- not `Regexp` at the top level.
        test::<RedundantRegexpConstructor>()
            .expect_no_offenses("Foo::Regexp.new(/regexp/)\n");
    }

    #[test]
    fn accepts_regexp_compile_with_string() {
        test::<RedundantRegexpConstructor>().expect_no_offenses("Regexp.compile('regexp')\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantRegexpConstructor);
