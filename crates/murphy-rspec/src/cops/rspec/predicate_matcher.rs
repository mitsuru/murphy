//! `RSpec/PredicateMatcher` — prefer predicate matchers over predicate calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/PredicateMatcher
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `PredicateMatcher` with `EnforcedStyle: inflected`
//!   (default) / `explicit` and `Strict: true` (default).
//!   Inflected flags `expect(foo.bar?).to <boolean>` where the actual is a
//!   non-bare `?` send (or a `Block` wrapping one) and the matcher is
//!   truthy/falsy (`Strict: true`) or truthy/falsy plus `be`/`eq`/`eql`/
//!   `equal` with a boolean literal (`Strict: false`); `respond_to?` with
//!   more than one arg stays clean via `cannot_replace_predicate?`.
//!   The message is `Prefer using `%<matcher>s` matcher over
//!   `%<predicate>s`.` with `to_predicate_matcher` (`is_a?` → `be_a`,
//!   `instance_of?` → `be_an_instance_of`, `include?`/`respond_to?` strip
//!   `?`, `exist?`/`exists?` → `exist`, `has_*?` → `have_*`, else
//!   `be_*`). Explicit flags `expect(foo).to be_*` / `have_*` / `include`
//!   (single arg only) / `respond_to` outside the built-in
//!   (`be_truthy` / `be_falsey` / `be_falsy` / `have_attributes` /
//!   `have_received` / `be_between` / `be_within`) plus
//!   `AllowedExplicitMatchers` allow-list, with
//!   `Prefer using `%<predicate>s` over `%<matcher>s` matcher.` and
//!   `to_predicate_method` (`be_a` → `is_a?`, etc).
//!   Detection is at parity; autocorrect (rewrite between spellings) is
//!   not ported in this batch — same convention as `RSpec/BeEmpty`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` (`to` / `to_not` / `not_to`), default inflected:
//!
//! - `expect(foo.empty?).to be_truthy` — flagged (→ `be_empty`).
//! - `expect(foo.empty?).to be(true)` with `Strict: false` — flagged.
//! - `expect(foo.empty?).to be(true)` with `Strict: true` — clean.
//! - `expect(foo.respond_to?(:a, true)).to be_truthy` — multi-arg, clean.
//! - `expect(foo).to be_empty` with `EnforcedStyle: explicit` — flagged.
//! - `expect(foo).to be_truthy` with explicit — built-in, clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites between `foo.bar?` and `be_bar`. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct PredicateMatcher;

#[derive(CopOptions)]
pub struct PredicateMatcherOptions {
    #[option(
        name = "EnforcedStyle",
        default = "inflected",
        description = "Whether to enforce inflected or explicit predicate matcher style."
    )]
    pub enforced_style: PredicateMatcherStyle,
    #[option(
        name = "Strict",
        default = true,
        description = "Whether to check boolean matchers strictly."
    )]
    pub strict: bool,
    #[option(
        name = "AllowedExplicitMatchers",
        default = [],
        description = "Additional matchers allowed in explicit style."
    )]
    pub allowed_explicit_matchers: Vec<String>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum PredicateMatcherStyle {
    #[option(value = "inflected")]
    Inflected,
    #[option(value = "explicit")]
    Explicit,
}

#[cop(
    name = "RSpec/PredicateMatcher",
    description = "Prefer using predicate matcher over using predicate method directly.",
    default_severity = "warning",
    default_enabled = true,
    options = PredicateMatcherOptions
)]
impl PredicateMatcher {
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<PredicateMatcherOptions>();
        if opts.enforced_style == PredicateMatcherStyle::Inflected {
            check_inflected(cx, node, opts.strict);
        } else {
            check_explicit(cx, node, &opts.allowed_explicit_matchers);
        }
    }
}

fn check_inflected(cx: &Cx<'_>, node: NodeId, strict: bool) {
    let NodeKind::Send {
        receiver,
        args, ..
    } = *cx.kind(node)
    else {
        return;
    };
    let Some(expect_id) = receiver.get() else {
        return;
    };
    let NodeKind::Send {
        receiver: e_recv,
        method: e_method,
        args: e_args,
    } = *cx.kind(expect_id)
    else {
        return;
    };
    if e_recv != OptNodeId::NONE || cx.symbol_str(e_method) != "expect" {
        return;
    }
    let e_arg_ids = cx.list(e_args);
    if e_arg_ids.len() != 1 {
        return;
    }
    let Some(pred_id) = predicate_send_in_actual(cx, e_arg_ids[0]) else {
        return;
    };
    if is_multi_arg_respond_to(cx, pred_id) {
        return;
    }
    let to_args = cx.list(args);
    if to_args.is_empty() {
        return;
    }
    if !is_boolean_matcher(cx, to_args[0], strict) {
        return;
    }
    let pred_name = predicate_name(cx, pred_id).unwrap_or_default();
    let matcher = to_predicate_matcher(&pred_name);
    cx.emit_offense(
        cx.range(node),
        &format!("Prefer using `{matcher}` matcher over `{pred_name}`."),
        None,
    );
}

/// The predicate `Send` inside an `expect(...)` actual, if any.
///
/// Mirrors `predicate_in_actual?`: the actual is either a bare `Send`
/// with a non-nil receiver ending in `?`, or a `Block` wrapping such a
/// `Send` (e.g. `foo.all? { |x| ... }`).
fn predicate_send_in_actual(cx: &Cx<'_>, actual: NodeId) -> Option<NodeId> {
    match *cx.kind(actual) {
        NodeKind::Send {
            receiver, method, ..
        } => {
            if receiver == OptNodeId::NONE {
                return None;
            }
            if !cx.symbol_str(method).ends_with('?') {
                return None;
            }
            Some(actual)
        }
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                return None;
            };
            if receiver == OptNodeId::NONE {
                return None;
            }
            if !cx.symbol_str(method).ends_with('?') {
                return None;
            }
            Some(call)
        }
        _ => None,
    }
}

fn predicate_name(cx: &Cx<'_>, pred: NodeId) -> Option<String> {
    let NodeKind::Send { method, .. } = *cx.kind(pred) else {
        return None;
    };
    Some(cx.symbol_str(method).to_owned())
}

fn is_multi_arg_respond_to(cx: &Cx<'_>, pred: NodeId) -> bool {
    let NodeKind::Send { method, args, .. } = *cx.kind(pred) else {
        return false;
    };
    if cx.symbol_str(method) != "respond_to?" {
        return false;
    }
    cx.list(args).len() > 1
}

fn is_boolean_matcher(cx: &Cx<'_>, matcher: NodeId, strict: bool) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(matcher)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    let name = cx.symbol_str(method);
    if matches!(
        name,
        "be_truthy"
            | "be_falsey"
            | "be_falsy"
            | "a_truthy_value"
            | "a_falsey_value"
            | "a_falsy_value"
    ) {
        return true;
    }
    if strict {
        return false;
    }
    if matches!(name, "be" | "eq" | "eql" | "equal") {
        let arg_ids = cx.list(args);
        if arg_ids.len() != 1 {
            return false;
        }
        return matches!(
            *cx.kind(arg_ids[0]),
            NodeKind::True_ | NodeKind::False_
        );
    }
    false
}

fn to_predicate_matcher(name: &str) -> String {
    match name {
        "is_a?" => "be_a".to_owned(),
        "instance_of?" => "be_an_instance_of".to_owned(),
        "include?" | "respond_to?" => name.trim_end_matches('?').to_owned(),
        "exist?" | "exists?" => "exist".to_owned(),
        _ if name.starts_with("has_") && name.ends_with('?') => {
            format!("have_{}", &name[4..name.len() - 1])
        }
        _ if name.ends_with('?') => {
            format!("be_{}", &name[..name.len() - 1])
        }
        _ => format!("be_{name}"),
    }
}

fn check_explicit(cx: &Cx<'_>, node: NodeId, allowed: &[String]) {
    let NodeKind::Send {
        receiver,
        args, ..
    } = *cx.kind(node)
    else {
        return;
    };
    let Some(expect_id) = receiver.get() else {
        return;
    };
    let NodeKind::Send {
        receiver: e_recv,
        method: e_method,
        args: e_args,
    } = *cx.kind(expect_id)
    else {
        return;
    };
    if e_recv != OptNodeId::NONE || cx.symbol_str(e_method) != "expect" {
        return;
    }
    let e_arg_ids = cx.list(e_args);
    if e_arg_ids.is_empty() {
        return;
    }
    if matches!(*cx.kind(e_arg_ids[0]), NodeKind::Nil) {
        return;
    }
    let to_args = cx.list(args);
    if to_args.is_empty() {
        return;
    }
    let matcher_id = to_args[0];
    // Block form `expect(foo).to be_all { }`: the matcher send is the call
    // of a wrapping block. Accept both plain and block-wrapped matchers.
    let matcher_send = if matches!(*cx.kind(matcher_id), NodeKind::Block { .. }) {
        let NodeKind::Block { call, .. } = *cx.kind(matcher_id) else {
            return;
        };
        call
    } else {
        matcher_id
    };
    let NodeKind::Send {
        receiver: m_recv,
        method: m_method,
        args: m_args,
    } = *cx.kind(matcher_send)
    else {
        return;
    };
    if m_recv != OptNodeId::NONE {
        return;
    }
    let m_name = cx.symbol_str(m_method).to_owned();
    if !is_predicate_matcher_name(&m_name) {
        return;
    }
    if is_builtin_explicit_matcher(&m_name) || allowed.iter().any(|a| a == &m_name) {
        return;
    }
    if m_name == "include" && cx.list(m_args).len() != 1 {
        return;
    }
    let pred = to_predicate_method(&m_name);
    cx.emit_offense(
        cx.range(node),
        &format!("Prefer using `{pred}` over `{m_name}` matcher."),
        None,
    );
}

fn is_predicate_matcher_name(name: &str) -> bool {
    if (name.starts_with("be_") || name.starts_with("have_")) && !name.ends_with('?') {
        return true;
    }
    matches!(name, "include" | "respond_to")
}

fn is_builtin_explicit_matcher(name: &str) -> bool {
    matches!(
        name,
        "be_truthy"
            | "be_falsey"
            | "be_falsy"
            | "have_attributes"
            | "have_received"
            | "be_between"
            | "be_within"
    )
}

fn to_predicate_method(matcher: &str) -> String {
    match matcher {
        "be_a" | "be_an" | "be_a_kind_of" | "a_kind_of" | "be_kind_of" => "is_a?".to_owned(),
        "be_an_instance_of" | "be_instance_of" | "an_instance_of" => "instance_of?".to_owned(),
        "include" => "include?".to_owned(),
        "respond_to" => "respond_to?".to_owned(),
        _ if matcher.starts_with("have_") => {
            format!("has_{}?", &matcher["have_".len()..])
        }
        _ if matcher.starts_with("be_") => {
            format!("{}?", &matcher["be_".len()..])
        }
        _ => format!("{matcher}?"),
    }
}

#[cfg(test)]
mod tests {
    use super::{PredicateMatcher, PredicateMatcherOptions, PredicateMatcherStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn explicit() -> PredicateMatcherOptions {
        PredicateMatcherOptions {
            enforced_style: PredicateMatcherStyle::Explicit,
            strict: true,
            allowed_explicit_matchers: Vec::new(),
        }
    }

    fn non_strict() -> PredicateMatcherOptions {
        PredicateMatcherOptions {
            enforced_style: PredicateMatcherStyle::Inflected,
            strict: false,
            allowed_explicit_matchers: Vec::new(),
        }
    }

    #[test]
    fn flags_inflected_predicate() {
        test::<PredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo.empty?).to be_truthy
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using `be_empty` matcher over `empty?`.
            "#});
    }

    #[test]
    fn flags_inflected_is_a() {
        test::<PredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo.is_a?(Array)).to be_truthy
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using `be_a` matcher over `is_a?`.
            "#});
    }

    #[test]
    fn flags_inflected_has_key() {
        test::<PredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo.has_key?("a")).to be_truthy
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using `have_key` matcher over `has_key?`.
            "#});
    }

    #[test]
    fn allows_strict_be_true() {
        test::<PredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo.empty?).to be(true)
            "#});
    }

    #[test]
    fn flags_non_strict_be_true() {
        test::<PredicateMatcher>()
            .with_options(&non_strict())
            .expect_offense(indoc! {r#"
                expect(foo.empty?).to be(true)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using `be_empty` matcher over `empty?`.
            "#});
    }

    #[test]
    fn allows_multi_arg_respond_to() {
        test::<PredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo.respond_to?(:bar, true)).to be_truthy
            "#});
    }

    #[test]
    fn allows_non_predicate() {
        test::<PredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo.something).to be(true)
            "#});
    }

    #[test]
    fn flags_explicit_matcher() {
        test::<PredicateMatcher>()
            .with_options(&explicit())
            .expect_offense(indoc! {r#"
                expect(foo).to be_empty
                ^^^^^^^^^^^^^^^^^^^^^^^ Prefer using `empty?` over `be_empty` matcher.
            "#});
    }

    #[test]
    fn allows_explicit_builtin() {
        test::<PredicateMatcher>()
            .with_options(&explicit())
            .expect_no_offenses(indoc! {r#"
                expect(foo).to be_truthy
            "#});
    }
}

murphy_plugin_api::submit_cop!(PredicateMatcher);
