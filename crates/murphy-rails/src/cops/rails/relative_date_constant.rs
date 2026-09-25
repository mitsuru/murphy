//! `Rails/RelativeDateConstant` — do not assign relative dates to constants.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RelativeDateConstant
//! upstream_version_checked: 2.35.0
//! version_added: "0.48"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `on_casgn` / `on_masgn` / `on_or_asgn`
//!   with nested zero-arg relative-date (`since from_now after ago until
//!   before yesterday tomorrow`) search that skips block bodies, whole-node
//!   (casgn/or_asgn) or name-to-value (masgn) offenses, and casgn-only
//!   `def self.*` autocorrect for top-level (scope-nil) constants.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

const RELATIVE_DATE_METHODS: &[&str] = &[
    "since",
    "from_now",
    "after",
    "ago",
    "until",
    "before",
    "yesterday",
    "tomorrow",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RelativeDateConstant;

#[cop(
    name = "Rails/RelativeDateConstant",
    description = "Do not assign relative date to constants.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RelativeDateConstant {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_casgn(node, cx);
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_masgn(node, cx);
    }

    #[on_node(kind = "or_asgn")]
    fn check_orasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_orasgn(node, cx);
    }
}

fn is_relative_date_send(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return None;
    }
    let method = cx.method_name(id)?.to_owned();
    if !RELATIVE_DATE_METHODS.contains(&method.as_str()) {
        return None;
    }
    if !cx.call_arguments(id).is_empty() {
        return None;
    }
    Some(method)
}

fn is_any_block(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

fn find_relative_dates(cx: &Cx<'_>, id: NodeId, out: &mut Vec<String>) {
    if is_any_block(cx, id) {
        return;
    }
    for child in cx.children(id) {
        find_relative_dates(cx, child, out);
    }
    if let Some(m) = is_relative_date_send(cx, id) {
        out.push(m);
    }
}

fn check_casgn(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Casgn { scope, name: _, value } = *cx.kind(node) else {
        return;
    };
    let Some(val) = value.get() else {
        return;
    };
    let mut found = Vec::new();
    find_relative_dates(cx, val, &mut found);
    let Some(method) = found.into_iter().next() else {
        return;
    };
    cx.emit_offense(
        cx.range(node),
        &format!("Do not assign `{method}` to constants as it will be evaluated only once."),
        None,
    );
    if scope != OptNodeId::NONE {
        return;
    }
    if let Some(replacement) = casgn_correction(cx, node) {
        cx.emit_edit(cx.range(node), &replacement);
    }
}

fn casgn_correction(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let NodeKind::Casgn { scope, name, value } = *cx.kind(node) else {
        return None;
    };
    if scope != OptNodeId::NONE {
        return None;
    }
    let val = value.get()?;
    let const_name = cx.symbol_str(name).to_owned();
    let value_src = cx.raw_source(cx.range(val)).to_owned();
    let start = cx.range(node).start as usize;
    let src = cx.source();
    let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = start - line_start;
    let indent = " ".repeat(column);
    let method_part = const_name.to_lowercase();
    Some(format!(
        "def self.{method_part}\n{indent}{indent}{value_src}\n{indent}end"
    ))
}

fn check_masgn(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Masgn { lhs, rhs } = *cx.kind(node) else {
        return;
    };
    if !matches!(*cx.kind(rhs), NodeKind::Array(_)) {
        return;
    }
    let NodeKind::Mlhs(targets) = *cx.kind(lhs) else {
        return;
    };
    let lhs_ids = cx.list(targets).to_vec();
    let NodeKind::Array(values) = *cx.kind(rhs) else {
        return;
    };
    let rhs_ids = cx.list(values).to_vec();
    for (target, val) in lhs_ids.iter().zip(rhs_ids.iter()) {
        if !matches!(*cx.kind(*target), NodeKind::Casgn { .. }) {
            continue;
        }
        let mut found = Vec::new();
        find_relative_dates(cx, *val, &mut found);
        let Some(method) = found.into_iter().next() else {
            continue;
        };
        let range = Range {
            start: cx.range(*target).start,
            end: cx.range(*val).end,
        };
        cx.emit_offense(
            range,
            &format!("Do not assign `{method}` to constants as it will be evaluated only once."),
            None,
        );
    }
}

fn check_orasgn(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::OrAsgn { target, value } = *cx.kind(node) else {
        return;
    };
    if !matches!(*cx.kind(target), NodeKind::Casgn { .. }) {
        return;
    }
    let Some(method) = is_relative_date_send(cx, value) else {
        // Upstream `relative_date_or_assignment` only matches direct
        // `(send _ $METHODS)` on the rhs, not nested. No offense otherwise.
        return;
    };
    cx.emit_offense(
        cx.range(node),
        &format!("Do not assign `{method}` to constants as it will be evaluated only once."),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::RelativeDateConstant;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn allows_method_with_args() {
        test::<RelativeDateConstant>().expect_no_offenses(indoc! {r#"
            class SomeClass
              EXPIRED_AT = 1.week.since(base)
            end
        "#});
    }

    #[test]
    fn allows_lambda() {
        test::<RelativeDateConstant>().expect_no_offenses(indoc! {r#"
            class SomeClass
              EXPIRED_AT = -> { 1.year.ago }
            end
        "#});
    }

    #[test]
    fn allows_proc() {
        test::<RelativeDateConstant>().expect_no_offenses(indoc! {r#"
            class SomeClass
              EXPIRED_AT = Proc.new { 1.year.ago }
            end
        "#});
    }

    #[test]
    fn allows_nested_proc_hash() {
        test::<RelativeDateConstant>().expect_no_offenses(indoc! {r#"
            class SomeClass
              EXPIRIES = {
                yearly: Proc.new { 1.year.ago },
                monthly: Proc.new { 1.month.ago }
              }
            end
        "#});
    }

    #[test]
    fn flags_since() {
        test::<RelativeDateConstant>().expect_correction(
            indoc! {r#"
                class SomeClass
                  EXPIRED_AT = 1.week.since
                  ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `since` to constants as it will be evaluated only once.
                end
            "#},
            "class SomeClass\n  def self.expired_at\n    1.week.since\n  end\nend\n",
        );
    }

    #[test]
    fn flags_yesterday() {
        test::<RelativeDateConstant>().expect_correction(
            indoc! {r#"
                class SomeClass
                  RECENT_DATE = Date.yesterday
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `yesterday` to constants as it will be evaluated only once.
                end
            "#},
            "class SomeClass\n  def self.recent_date\n    Date.yesterday\n  end\nend\n",
        );
    }

    #[test]
    fn flags_tomorrow() {
        test::<RelativeDateConstant>().expect_correction(
            indoc! {r#"
                class SomeClass
                  FUTURE_DATE = Time.zone.tomorrow
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `tomorrow` to constants as it will be evaluated only once.
                end
            "#},
            "class SomeClass\n  def self.future_date\n    Time.zone.tomorrow\n  end\nend\n",
        );
    }

    #[test]
    fn flags_chained_after() {
        test::<RelativeDateConstant>().expect_correction(
            indoc! {r#"
                class SomeClass
                  START_DATE = 2.weeks.ago.to_date
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `ago` to constants as it will be evaluated only once.
                end
            "#},
            "class SomeClass\n  def self.start_date\n    2.weeks.ago.to_date\n  end\nend\n",
        );
    }

    #[test]
    fn flags_range() {
        test::<RelativeDateConstant>().expect_offense(indoc! {r#"
            class SomeClass
              TRIAL_PERIOD = DateTime.current..1.day.since
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `since` to constants as it will be evaluated only once.
            end
        "#});
    }

    #[test]
    fn flags_or_asgn() {
        test::<RelativeDateConstant>().expect_offense(indoc! {r#"
            class SomeClass
              EXPIRED_AT ||= 1.week.since
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `since` to constants as it will be evaluated only once.
            end
        "#});
    }

    #[test]
    fn flags_masgn() {
        test::<RelativeDateConstant>().expect_offense(indoc! {r#"
            class SomeClass
              START, A, x = 2.weeks.ago, 1.week.since, 5
              ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `ago` to constants as it will be evaluated only once.
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not assign `since` to constants as it will be evaluated only once.
            end
        "#});
    }

    #[test]
    fn allows_masgn_splat() {
        test::<RelativeDateConstant>().expect_no_offenses(indoc! {r#"
            class SomeClass
              FOO, BAR = *do_something
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(RelativeDateConstant);
