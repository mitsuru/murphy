//! `Lint/UselessRescue` — Checks for useless `rescue`s which only reraise
//! rescued exceptions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessRescue
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: bare raise reraising, raise with
//!   exception variable, raise $!, raise $ERROR_INFO, multiple rescue blocks
//!   with last reraise, Thread#raise not flagged, modifier rescue not
//!   flagged, additional code before raise not flagged, different variable
//!   raise not flagged, exception variable used in ensure not flagged.
//! ```
//!
//! ## Matched shapes
//!
//! - `rescue; raise; end` — bare raise
//! - `rescue => e; raise e; end` — reraise exception variable
//! - `rescue; raise $!; end` — reraise global
//! - `rescue; raise $ERROR_INFO; end` — reraise error info
//!
//! ## Autocorrect
//!
//! None.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Useless `rescue` detected.";

#[derive(Default)]
pub struct UselessRescue;

#[cop(
    name = "Lint/UselessRescue",
    description = "Checks for useless `rescue`s which only reraise rescued exceptions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UselessRescue {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip modifier-form rescue (`foo rescue nil`) — no `end` keyword.
        if cx.loc(node).end_keyword() == Range::ZERO {
            return;
        }

        let NodeKind::Rescue { resbodies, .. } = *cx.kind(node) else {
            return;
        };

        let resbody_list = cx.list(resbodies);
        let Some(&last_resbody) = resbody_list.last() else {
            return;
        };

        if only_reraising(last_resbody, cx) {
            let r = cx.range(last_resbody);
            let source = cx.source();
            let line_end = source[r.start as usize..]
                .find('\n')
                .map(|i| r.start as usize + i)
                .unwrap_or(r.end as usize);
            let range = Range { start: r.start, end: line_end as u32 };
            cx.emit_offense(range, MSG, None);
        }
    }
}

fn only_reraising(resbody: NodeId, cx: &Cx<'_>) -> bool {
    if use_exception_variable_in_ensure(resbody, cx) {
        return false;
    }

    let NodeKind::Resbody { var, body, .. } = *cx.kind(resbody) else {
        return false;
    };

    let Some(body_id) = body.get() else {
        return false;
    };

    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(body_id)
    else {
        return false;
    };

    if receiver.get().is_some() {
        return false;
    }

    if cx.symbol_str(method) != "raise" {
        return false;
    }

    let args_list = cx.list(args);

    if args_list.is_empty() {
        return true;
    }

    if args_list.len() > 1 {
        return false;
    }

    let arg_id = args_list[0];
    match *cx.kind(arg_id) {
        NodeKind::Lvar(s) => {
            let exc_var_matches = var.get().is_some_and(|var_id| {
                matches!(*cx.kind(var_id), NodeKind::Lvasgn { name, .. } if cx.symbol_str(name) == cx.symbol_str(s))
            });
            exc_var_matches
        }
        NodeKind::Gvar(s) => {
            let name = cx.symbol_str(s);
            name == "$!" || name == "$ERROR_INFO"
        }
        _ => false,
    }
}

fn use_exception_variable_in_ensure(resbody: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Resbody { var, .. } = *cx.kind(resbody) else {
        return false;
    };

    let Some(var_id) = var.get() else {
        return false;
    };

    let NodeKind::Lvasgn { name, .. } = *cx.kind(var_id) else {
        return false;
    };
    let var_name = cx.symbol_str(name);

    for ancestor in cx.ancestors(resbody) {
        if let NodeKind::Ensure { ensure_, .. } = *cx.kind(ancestor) {
            if let Some(ensure_body) = ensure_.get() {
                // NOTE: This simple descendant walk does not account for
                // variable shadowing (e.g. block args or inner Lvasgn in the
                // ensure body with the same name). A more robust
                // implementation would walk ancestor chains from each Lvar to
                // check for intervening bindings. In practice the common case
                // (bare `ensure; do_something(e); end`) works correctly.
                for desc in cx.descendants(ensure_body) {
                    if let NodeKind::Lvar(s) = *cx.kind(desc) {
                        if cx.symbol_str(s) == var_name {
                            return true;
                        }
                    }
                }
            }
            return false;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::UselessRescue;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_simple_rescue_anon_reraises() {
        test::<UselessRescue>().expect_offense(indoc! {r#"
            def foo
              do_something
            rescue
            ^^^^^^ Useless `rescue` detected.
              raise
            end
        "#});
    }

    #[test]
    fn flags_rescue_reraises_exception_variable() {
        test::<UselessRescue>().expect_offense(indoc! {r#"
            def foo
              do_something
            rescue => e
            ^^^^^^^^^^^ Useless `rescue` detected.
              raise e
            end
        "#});
    }

    #[test]
    fn flags_rescue_reraises_global() {
        test::<UselessRescue>().expect_offense(indoc! {r#"
            def foo
              do_something
            rescue
            ^^^^^^ Useless `rescue` detected.
              raise $!
            end
        "#});
    }

    #[test]
    fn flags_rescue_reraises_error_info() {
        test::<UselessRescue>().expect_offense(indoc! {r#"
            def foo
              do_something
            rescue
            ^^^^^^ Useless `rescue` detected.
              raise $ERROR_INFO
            end
        "#});
    }

    #[test]
    fn accepts_rescue_with_additional_code() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
              do_something
            rescue
              do_cleanup
              raise e
            end
        "#});
    }

    #[test]
    fn accepts_rescue_different_variable() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
              do_something
            rescue => e
              raise x
            end
        "#});
    }

    #[test]
    fn flags_multiple_rescue_last_is_reraising() {
        test::<UselessRescue>().expect_offense(indoc! {r#"
            def foo
              do_something
            rescue ArgumentError
              # noop
            rescue
            ^^^^^^ Useless `rescue` detected.
              raise
            end
        "#});
    }

    #[test]
    fn accepts_multiple_rescue_not_last_reraises() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
              do_something
            rescue ArgumentError
              raise
            rescue
              # noop
            end
        "#});
    }

    #[test]
    fn accepts_thread_raise() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
              do_something
            rescue
              Thread.current.raise
            end
        "#});
    }

    #[test]
    fn accepts_modifier_rescue() {
        test::<UselessRescue>().expect_no_offenses("do_something rescue nil\n");
    }

    #[test]
    fn accepts_exception_variable_used_in_ensure() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
              do_something
            rescue => e
              raise
            ensure
              do_something(e)
            end
        "#});
    }

    #[test]
    fn accepts_empty_rescue_with_binding_and_empty_ensure() {
        test::<UselessRescue>().expect_no_offenses(indoc! {r#"
            def foo
            rescue => e
            ensure
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UselessRescue);
