//! `Rails/Date` — flag `Date.today` etc. without zone and `to_time` on Date objects.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Date
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 with default config (EnforcedStyle flexible,
//!   AllowToTime true): flexible flags only `Date.today` (strict flags
//!   today/current/yesterday/tomorrow), `to_time` is allowed by default,
//!   `to_time_in_current_zone` always flags as deprecated in favour of
//!   `in_time_zone`. Method-chain extraction walks Send/Csend ancestors,
//!   `Time.zone` receiver rewrite for the Date branch, and the zone-suffixed
//!   string / single-arg `to_time` exemptions. The `good_methods`
//!   (TimeZone ACCEPTED_METHODS) safe-chain gate is implemented. Not gated:
//!   `Style/Date` interplay. `AllowToTime: false` and `strict` are supported
//!   via cop options.
//! ```
//!
//! ## Matched shapes (default flexible)
//!
//! - `Date.today` → `Time.zone.today` (offense on `today` selector).
//! - `Date.current` (flexible) → no offense; strict → offense.
//! - `date.to_time_in_current_zone` → `date.in_time_zone` (deprecated).
//!
//! `date.to_time` passes by default (`AllowToTime: true`).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Date;

#[derive(CopOptions)]
pub struct DateOptions {
    #[option(
        name = "EnforcedStyle",
        default = "flexible",
        description = "Whether to flag only `today` (flexible) or all of today/current/yesterday/tomorrow (strict)."
    )]
    pub enforced_style: DateStyle,
    #[option(
        name = "AllowToTime",
        default = true,
        description = "Whether to allow `to_time` on Date objects."
    )]
    pub allow_to_time: bool,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    #[option(value = "flexible")]
    Flexible,
    #[option(value = "strict")]
    Strict,
}

#[cop(
    name = "Rails/Date",
    description = "Do not use `Date.today` without zone. Use `Time.zone.today` instead.",
    default_severity = "warning",
    default_enabled = true,
    options = DateOptions,
)]
impl Date {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Const { scope, name } = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(name) != "Date" {
            return;
        }
        // Only core `Date` / `::Date`.
        if let Some(scope_id) = scope.get()
            && !matches!(*cx.kind(scope_id), NodeKind::Cbase) {
                return;
            }
        // `method_send?`: parent is a send whose receiver is this const.
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        let is_recv = match *cx.kind(parent) {
            NodeKind::Send { receiver, .. } => receiver.get() == Some(node),
            NodeKind::Csend { receiver, .. } => receiver == node,
            _ => false,
        };
        if !is_recv {
            return;
        }
        check_date_node(parent, cx);
    }

    // Mirrors upstream `RESTRICT_ON_SEND = %i[to_time to_time_in_current_zone]`
    // plus `alias on_csend on_send`.
    #[on_node(kind = "send", methods = ["to_time", "to_time_in_current_zone"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send_node(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_send_node(node, cx);
    }
}

fn opts(cx: &Cx<'_>) -> (bool, bool) {
    let o = cx.options_or_default::<DateOptions>();
    (o.enforced_style == DateStyle::Strict, o.allow_to_time)
}

fn bad_days(strict: bool) -> &'static [&'static str] {
    if strict {
        &["today", "current", "yesterday", "tomorrow"]
    } else {
        &["today"]
    }
}

/// Upstream `extract_method_chain`: node + each ancestor send method names.
fn method_chain(cx: &Cx<'_>, node: NodeId) -> Vec<String> {
    let mut chain = vec![];
    if let Some(m) = cx.method_name(node) {
        chain.push(m.to_owned());
    }
    for anc in cx.ancestors(node) {
        match *cx.kind(anc) {
            NodeKind::Send { .. } | NodeKind::Csend { .. } => {
                if let Some(m) = cx.method_name(anc) {
                    chain.push(m.to_owned());
                }
            }
            _ => {}
        }
        // Upstream walks all send ancestors, not just the direct receiver
        // chain; the full-ancestor walk is the conservative approximation.
        // Cap to avoid runaway on huge files (chain depth is tiny).
        if chain.len() > 16 {
            break;
        }
    }
    chain
}

fn check_date_node(node: NodeId, cx: &Cx<'_>) {
    let (strict, _) = opts(cx);
    let chain = method_chain(cx, node);
    let bad = bad_days(strict);
    let hit: Vec<&str> = chain
        .iter()
        .filter(|m| bad.contains(&m.as_str()))
        .map(|m| m.as_str())
        .collect();
    if hit.is_empty() {
        return;
    }
    let method_called = hit.join(".");
    let day = if method_called == "current" {
        "today".to_owned()
    } else {
        method_called.clone()
    };
    let msg = format!("Do not use `Date.{method_called}` without zone. Use `Time.zone.{day}` instead.");
    cx.emit_offense(cx.loc(node).name, &msg, None);
    // Autocorrect: replace `Date` receiver with `Time.zone`.
    if let Some(recv) = cx.call_receiver(node).get() {
        cx.emit_edit(cx.range(recv), "Time.zone");
    }
}

fn check_send_node(node: NodeId, cx: &Cx<'_>) {
    let (strict, allow_to_time) = opts(cx);
    let Some(method) = cx.method_name(node) else {
        return;
    };
    if method != "to_time" && method != "to_time_in_current_zone" {
        return;
    }
    // Receiver must be present.
    let has_recv = match *cx.kind(node) {
        NodeKind::Send { receiver, .. } => receiver.get().is_some(),
        NodeKind::Csend { .. } => true,
        _ => false,
    };
    if !has_recv {
        return;
    }
    if allow_to_time && method == "to_time" {
        return;
    }
    if safe_chain(cx, node, strict) || safe_to_time(cx, node) {
        return;
    }
    // Deprecated `to_time_in_current_zone` → `in_time_zone`.
    if method == "to_time_in_current_zone" {
        cx.emit_offense(
            cx.loc(node).name,
            "`to_time_in_current_zone` is deprecated. Use `in_time_zone` instead.",
            None,
        );
        cx.emit_edit(cx.loc(node).name, "in_time_zone");
    }
    cx.emit_offense(
        cx.loc(node).name,
        &format!("Do not use `{method}` on Date objects, because they know nothing about the time zone in use."),
        None,
    );
}

/// `(chain & bad_methods).empty? || !(chain & good_methods).empty?` → safe.
fn safe_chain(cx: &Cx<'_>, node: NodeId, strict: bool) -> bool {
    let chain = method_chain(cx, node);
    let bad = ["to_time", "to_time_in_current_zone"];
    if !chain.iter().any(|m| bad.contains(&m.as_str())) {
        return true;
    }
    if strict {
        return false;
    }
    let good = [
        "in_time_zone",
        "utc",
        "getlocal",
        "xmlschema",
        "iso8601",
        "jisx0301",
        "rfc3339",
        "httpdate",
        "to_i",
        "to_f",
    ];
    chain.iter().any(|m| good.contains(&m.as_str()))
}

/// Zone-suffixed string receiver or single-arg `to_time(arg)` is safe.
fn safe_to_time(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    if method != "to_time" {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    if let NodeKind::Str(sid) = *cx.kind(recv) {
        let s = cx.string_str(sid);
        if has_zone_suffix(s) {
            return true;
        }
    }
    cx.call_arguments(node).len() == 1
}

fn has_zone_suffix(s: &str) -> bool {
    // `/([+-][\d:]+|\dZ)\z/` — trailing offset or `NZ`-style zone.
    if s.ends_with('Z') && s.len() >= 2 && s[..s.len() - 1].chars().last().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }
    let bytes = s.as_bytes();
    let i = bytes.len();
    // Scan trailing [0-9:] run after a + / - sign.
    let mut j = i;
    while j > 0 && (bytes[j - 1].is_ascii_digit() || bytes[j - 1] == b':') {
        j -= 1;
    }
    if j > 0 && j < i && (bytes[j - 1] == b'+' || bytes[j - 1] == b'-') {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Date, DateOptions, DateStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_date_today() {
        test::<Date>().expect_offense(indoc! {r#"
            Date.today
                 ^^^^^ Do not use `Date.today` without zone. Use `Time.zone.today` instead.
        "#});
    }

    #[test]
    fn does_not_flag_date_current_flexible() {
        test::<Date>().expect_no_offenses("Date.current\n");
    }

    #[test]
    fn flags_date_current_strict() {
        let opts = DateOptions {
            enforced_style: DateStyle::Strict,
            allow_to_time: true,
        };
        test::<Date>().with_options(&opts).expect_offense(indoc! {r#"
            Date.current
                 ^^^^^^^ Do not use `Date.current` without zone. Use `Time.zone.today` instead.
        "#});
    }

    #[test]
    fn does_not_flag_time_zone_today() {
        test::<Date>().expect_no_offenses("Time.zone.today\n");
    }

    #[test]
    fn does_not_flag_to_time_by_default() {
        test::<Date>().expect_no_offenses("date.to_time\n");
    }

    #[test]
    fn flags_to_time_when_disallowed() {
        let opts = DateOptions {
            enforced_style: DateStyle::Flexible,
            allow_to_time: false,
        };
        test::<Date>().with_options(&opts).expect_offense(indoc! {r#"
            date.to_time
                 ^^^^^^^ Do not use `to_time` on Date objects, because they know nothing about the time zone in use.
        "#});
    }

    #[test]
    fn flags_deprecated_to_time_in_current_zone() {
        test::<Date>().expect_offense(indoc! {r#"
            date.to_time_in_current_zone
                 ^^^^^^^^^^^^^^^^^^^^^^^ `to_time_in_current_zone` is deprecated. Use `in_time_zone` instead.
                 ^^^^^^^^^^^^^^^^^^^^^^^ Do not use `to_time_in_current_zone` on Date objects, because they know nothing about the time zone in use.
        "#});
    }

    #[test]
    fn does_not_flag_namespaced_date() {
        test::<Date>().expect_no_offenses("Foo::Date.today\n");
    }

    #[test]
    fn corrects_date_today() {
        test::<Date>()
            .expect_correction(
                indoc! {r#"
                    Date.today
                         ^^^^^ Do not use `Date.today` without zone. Use `Time.zone.today` instead.
                "#},
                "Time.zone.today\n",
            )
            .expect_no_offenses("Time.zone.today\n");
    }
}
murphy_plugin_api::submit_cop!(Date);
