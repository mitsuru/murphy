//! `Rails/SafeNavigation` — use `&.` instead of `try!` (or `try` with `ConvertTry`).
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SafeNavigation
//! upstream_version_checked: 2.35.0
//! version_added: "0.43"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: bare-receiver and explicit-receiver
//!   `try!` (plus `try` when `ConvertTry: true`, default false) whose
//!   first argument is a sym literal whose value contains a word char
//!   (upstream `/\w+[=!?]?/`, unanchored — so `:"foo-bar"` flags and
//!   corrects to `&."foo-bar"`). `try!` on a `&.` receiver never matches
//!   (upstream `send` pattern), and non-sym dispatches (`name`, `"bar"`,
//!   `0`) never flag. Offense covers the `try!` send only (a trailing
//!   block is preserved); correction replaces the `.try!(...)` span with
//!   `&.method[(args)]` (`&.` plus ` = value` for setter names, `self`
//!   prefix for bare calls). Gated on target Ruby >= 2.3 like upstream's
//!   `minimum_target_ruby_version`.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, RubyVersion, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SafeNavigation;

#[derive(CopOptions)]
pub struct SafeNavigationOptions {
    #[option(
        name = "ConvertTry",
        default = false,
        description = "Whether to convert `try` as well as `try!`."
    )]
    pub convert_try: bool,
}

#[cop(
    name = "Rails/SafeNavigation",
    description = "Use Ruby's safe navigation operator (`&.`) instead of `try!`.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "2.3",
    options = SafeNavigationOptions,
)]
impl SafeNavigation {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[try try!]`.
    #[on_node(kind = "send", methods = ["try", "try!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `minimum_target_ruby_version 2.3`; unset resolves to newest (fires).
    let ruby_ok = cx
        .target_ruby_version()
        .is_none_or(|v| v >= RubyVersion::new(2, 3));
    if !ruby_ok {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let method = cx.method_name(node);
    if method != Some("try") && method != Some("try!") {
        return;
    }
    let try_name = method.unwrap_or("try").to_owned();
    if try_name == "try" && !cx.options_or_default::<SafeNavigationOptions>().convert_try {
        return;
    }
    let args = cx.call_arguments(node);
    let Some(&dispatch) = args.first() else {
        return;
    };
    // Upstream `dispatch.sym_type?`.
    let NodeKind::Sym(sym) = *cx.kind(dispatch) else {
        return;
    };
    // Upstream `dispatch.value.match?(/\w+[=!?]?/)` — unanchored, so any
    // word char anywhere in the value qualifies.
    if !cx.symbol_str(sym).chars().any(is_word_char) {
        return;
    }
    // The arena send range covers an attached block (`foo.try!(:bar) { }`);
    // upstream reports and corrects the send alone.
    let send_end = send_only_end(cx, node, args);
    let send_range = murphy_plugin_api::Range {
        start: cx.range(node).start,
        end: send_end,
    };
    cx.emit_offense(
        send_range,
        &format!("Use safe navigation (`&.`) instead of `{try_name}`."),
        None,
    );
    // Upstream `method = method_node.source[1..]`: the sym source minus `:`.
    let dispatch_src = cx.raw_source(cx.range(dispatch)).to_owned();
    let bare = dispatch_src.strip_prefix(':').unwrap_or(&dispatch_src);
    let params: Vec<String> = args[1..]
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .collect();
    let new_call = replacement(bare, &params);
    if let Some(recv) = cx.call_receiver(node).get() {
        // Upstream replaces from the dot (`.try!...` span) to the send end.
        let dot_start = cx.loc(node).dot().start;
        let dot_start = if dot_start == 0 {
            cx.range(recv).end
        } else {
            dot_start
        };
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: dot_start,
                end: send_end,
            },
            &new_call,
        );
    } else {
        cx.emit_edit(send_range, &format!("self{new_call}"));
    }
}


/// Send-only end: the arena send range covers an attached block, while
/// upstream's `node.source_range` (and hence offense + correction) excludes
/// it. Mirrors `Rails/FreezeTime`'s recomputation.
fn send_only_end(cx: &Cx<'_>, node: NodeId, args: &[NodeId]) -> u32 {
    if cx.block_node(node).get().is_none() {
        return cx.range(node).end;
    }
    if cx.is_parenthesized(node) {
        if let Some(paren_end) = matching_close_paren(cx, node) {
            return paren_end;
        }
    } else if let Some(&last) = args.last() {
        return cx.range(last).end;
    } else {
        return cx.selector(node).end;
    }
    cx.range(node).end
}

/// Closing paren matching the call's open paren. Depth-counted so nested
/// calls (`File.read(Rails.root.join('a'))`) resolve to the outer `)`.
fn matching_close_paren(cx: &Cx<'_>, node: NodeId) -> Option<u32> {
    let toks = cx.tokens_in(cx.range(node));
    let sel_end = cx.selector(node).end;
    let open = toks
        .iter()
        .position(|t| t.kind == SourceTokenKind::LeftParen && t.range.start >= sel_end)?;
    let mut depth = 0i32;
    for tok in &toks[open..] {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    None
}

/// Upstream `replacement`: setter names take ` = value`, arg-less calls take
/// the bare method, otherwise the args are parenthesized.
fn replacement(method: &str, params: &[String]) -> String {
    if let Some(base) = method.strip_suffix('=') {
        format!("&.{base} = {}", params.join(", "))
    } else if params.is_empty() {
        format!("&.{method}")
    } else {
        format!("&.{method}({})", params.join(", "))
    }
}

/// Ruby `\w` word character (Unicode-aware, matching upstream semantics).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::{SafeNavigation, SafeNavigationOptions};
    use murphy_plugin_api::test_support::{indoc, test, test_with_options};

    #[test]
    fn flags_try_bang() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:bar)
                ^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_try_bang_with_args() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:bar, baz)
                ^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.bar(baz)\n",
        );
    }

    #[test]
    fn flags_try_bang_with_block() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:bar) { |e| e.baz }
                ^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.bar { |e| e.baz }\n",
        );
    }

    #[test]
    fn flags_setter() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:foo=, bar)
                ^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.foo = bar\n",
        );
    }

    #[test]
    fn flags_predicate_and_bang_names() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:bar?)
                ^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.bar?\n",
        );
    }

    #[test]
    fn flags_bare_try_bang() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                try!(:bar)
                ^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "self&.bar\n",
        );
    }

    #[test]
    fn flags_quoted_symbol_dispatch() {
        test::<SafeNavigation>().expect_correction(
            indoc! {r#"
                foo.try!(:"foo-bar")
                ^^^^^^^^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try!`.
            "#},
            "foo&.\"foo-bar\"\n",
        );
    }

    #[test]
    fn allows_plain_try_by_default() {
        test::<SafeNavigation>().expect_no_offenses("foo.try(:bar)\n");
    }

    #[test]
    fn flags_plain_try_when_convert_try() {
        let opts = SafeNavigationOptions { convert_try: true };
        test_with_options::<SafeNavigation>(&opts).expect_correction(
            indoc! {r#"
                foo.try(:bar)
                ^^^^^^^^^^^^^ Use safe navigation (`&.`) instead of `try`.
            "#},
            "foo&.bar\n",
        );
    }

    #[test]
    fn allows_operator_dispatch() {
        test::<SafeNavigation>().expect_no_offenses("foo.try!(:[], 0)\n");
    }

    #[test]
    fn allows_csend_receiver() {
        test::<SafeNavigation>().expect_no_offenses("foo&.try!(:bar)\n");
    }

    #[test]
    fn allows_non_sym_dispatch() {
        test::<SafeNavigation>().expect_no_offenses("foo.try!(name)\n");
        test::<SafeNavigation>().expect_no_offenses("foo.try('bar')\n");
    }

    #[test]
    fn gated_below_ruby_2_3() {
        test::<SafeNavigation>()
            .with_target_ruby_version(2, 2)
            .expect_no_offenses("foo.try!(:bar)\n");
    }
}
murphy_plugin_api::submit_cop!(SafeNavigation);

