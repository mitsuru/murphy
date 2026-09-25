//! `Rails/TimeZone` — use `Time.zone` instead of bare `Time`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/TimeZone
//! upstream_version_checked: 2.35.0
//! version_added: "0.30"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 with default config (`EnforcedStyle:
//!   flexible`): `on_const` for core `Time`/`::Time` with the
//!   `method_send?` gate plus `RESTRICT_ON_SEND = %i[to_time]` for
//!   `String#to_time`. Dangerous (`now local new parse at`),
//!   good (`zone zone_default find_zone find_zone!` plus `current`
//!   and the accepted chain in flexible), ISO-8601 zone-suffix and
//!   `in:`-offset exemptions, `localtime` messaging, and the
//!   insert-`.zone` / `current`→`now` / `new`→`parse|local|now`
//!   rewrites with redundant `.in_time_zone` removal are implemented.
//!   Autocorrect is unsafe upstream (`SafeAutoCorrect: false`).
//!   `strict` style is supported via cop options.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TimeZone;

#[derive(CopOptions)]
pub struct TimeZoneOptions {
    #[option(
        name = "EnforcedStyle",
        default = "flexible",
        description = "Whether `in_time_zone` chains are allowed (flexible) or only `Time.zone` (strict)."
    )]
    pub enforced_style: TimeZoneStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum TimeZoneStyle {
    #[option(value = "flexible")]
    Flexible,
    #[option(value = "strict")]
    Strict,
}

const GOOD_METHODS: &[&str] = &["zone", "zone_default", "find_zone", "find_zone!"];
const DANGEROUS_METHODS: &[&str] = &["now", "local", "new", "parse", "at"];
const ACCEPTED_METHODS: &[&str] = &[
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

const MSG_TMPL: &str = "Do not use `%<current>s` without zone. Use `%<prefer>s` instead.";
const MSG_ACCEPTABLE_TMPL: &str =
    "Do not use `%<current>s` without zone. Use one of %<prefer>s instead.";
const MSG_LOCALTIME: &str = "Do not use `Time.localtime` without offset or zone.";
const MSG_STRING_TO_TIME: &str =
    "Do not use `String#to_time` without zone. Use `Time.zone.parse` instead.";

#[cop(
    name = "Rails/TimeZone",
    description = "Checks for the use of Time methods without zone.",
    default_severity = "warning",
    default_enabled = true,
    options = TimeZoneOptions,
)]
impl TimeZone {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check_const(node, cx);
    }

    #[on_node(kind = "send", methods = ["to_time"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_to_time(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_to_time(node, cx);
    }
}

fn is_strict(cx: &Cx<'_>) -> bool {
    cx.options_or_default::<TimeZoneOptions>().enforced_style == TimeZoneStyle::Strict
}

fn good_methods(strict: bool) -> Vec<&'static str> {
    if strict {
        GOOD_METHODS.to_vec()
    } else {
        let mut v = GOOD_METHODS.to_vec();
        v.push("current");
        v.extend_from_slice(ACCEPTED_METHODS);
        v
    }
}

fn check_const(node: NodeId, cx: &Cx<'_>) {
    // Only core `Time` / `::Time`.
    if !cx.is_global_const(node, "Time") {
        return;
    }
    // Upstream `method_send?`: parent is a send whose receiver is this const.
    let Some(parent) = cx.parent(node).get() else {
        return;
    };
    if !matches!(*cx.kind(parent), NodeKind::Send { .. }) {
        return;
    }
    let is_recv = match *cx.kind(parent) {
        NodeKind::Send { receiver, .. } => receiver.get() == Some(node),
        _ => false,
    };
    if !is_recv {
        return;
    }
    check_time_node(parent, cx);
}

fn check_to_time(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(node) != Some("to_time") {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !matches!(*cx.kind(recv), NodeKind::Str(_)) {
        return;
    }
    if has_zone_suffix_node(cx, recv) {
        return;
    }
    let is_csend = matches!(*cx.kind(node), NodeKind::Csend { .. });
    cx.emit_offense(cx.loc(node).name, MSG_STRING_TO_TIME, None);
    if !is_csend {
        let recv_src = cx.raw_source(cx.range(recv));
        cx.emit_edit(cx.range(node), &format!("Time.zone.parse({recv_src})"));
    }
}

fn check_time_node(node: NodeId, cx: &Cx<'_>) {
    // Upstream `return if attach_timezone_specifier?(node.first_argument)`.
    if let Some(first) = cx.call_arguments(node).first().copied()
        && has_zone_suffix_node(cx, first)
    {
        return;
    }
    let chain = method_chain(cx, node);
    if not_danger_chain(cx, &chain) {
        return;
    }
    let strict = is_strict(cx);
    if !strict && chain.iter().any(|m| m == "localtime") {
        check_localtime(node, cx);
        return;
    }
    let dangerous: Vec<&str> = chain
        .iter()
        .filter(|m| DANGEROUS_METHODS.contains(&m.as_str()))
        .map(|m| m.as_str())
        .collect();
    if dangerous.is_empty() {
        return;
    }
    let method_name = dangerous.join(".");
    if offset_provided(cx, node) {
        return;
    }
    let msg = build_message(cx, &method_name, node, strict);
    cx.emit_offense(cx.loc(node).name, &msg, None);
    autocorrect(node, cx, &chain, strict);
}

/// Upstream `extract_method_chain`: offense send plus each ancestor send
/// whose receiver chain roots at global `Time`.
fn method_chain(cx: &Cx<'_>, node: NodeId) -> Vec<String> {
    let mut chain = vec![];
    if matches!(*cx.kind(node), NodeKind::Send { .. })
        && let Some(m) = cx.method_name(node)
    {
        // Offense node itself is always a Time-child send.
        chain.push(m.to_owned());
    }
    for anc in cx.ancestors(node) {
        if !matches!(*cx.kind(anc), NodeKind::Send { .. }) {
            continue;
        }
        if !receiver_roots_at_time(cx, anc) {
            continue;
        }
        if let Some(m) = cx.method_name(anc) {
            chain.push(m.to_owned());
        }
        if chain.len() > 32 {
            break;
        }
    }
    chain
}

/// Upstream `method_from_time_class?`: the send's receiver chain roots at
/// global `Time`.
fn receiver_roots_at_time(cx: &Cx<'_>, send: NodeId) -> bool {
    let Some(recv) = cx.call_receiver(send).get() else {
        return false;
    };
    roots_at_time(cx, recv)
}

fn roots_at_time(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Const { .. } => cx.is_global_const(id, "Time"),
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            match cx.call_receiver(id).get() {
                Some(r) => roots_at_time(cx, r),
                None => false,
            }
        }
        _ => false,
    }
}

fn not_danger_chain(cx: &Cx<'_>, chain: &[String]) -> bool {
    let strict = is_strict(cx);
    let good = good_methods(strict);
    if !chain.iter().any(|m| DANGEROUS_METHODS.contains(&m.as_str())) {
        return true;
    }
    chain.iter().any(|m| good.contains(&m.as_str()))
}

fn check_localtime(node: NodeId, cx: &Cx<'_>) {
    // Find the `localtime` send in the offense-plus-ancestors chain.
    let mut localtime_node: Option<NodeId> = None;
    if cx.method_name(node) == Some("localtime") {
        localtime_node = Some(node);
    } else {
        for anc in cx.ancestors(node) {
            if !matches!(*cx.kind(anc), NodeKind::Send { .. }) {
                continue;
            }
            if !receiver_roots_at_time(cx, anc) {
                continue;
            }
            if cx.method_name(anc) == Some("localtime") {
                localtime_node = Some(anc);
                break;
            }
        }
    }
    let Some(lt) = localtime_node else {
        return;
    };
    if !cx.call_arguments(lt).is_empty() {
        return;
    }
    cx.emit_offense(cx.loc(node).name, MSG_LOCALTIME, None);
    let chain = method_chain(cx, node);
    autocorrect(node, cx, &chain, is_strict(cx));
}

/// `Time.new` (7 args or `in:`), `Time.at`/`Time.now` (`in:`) are safe.
fn offset_provided(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    match method {
        "new" => {
            if cx.call_arguments(node).len() == 7 {
                return true;
            }
            offset_option_provided(cx, node)
        }
        "at" | "now" => offset_option_provided(cx, node),
        _ => false,
    }
}

fn offset_option_provided(cx: &Cx<'_>, node: NodeId) -> bool {
    let args = cx.call_arguments(node);
    let Some(&last) = args.last() else {
        return false;
    };
    if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
        return false;
    }
    for pair in cx.hash_pairs(last) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        if !matches!(*cx.kind(key), NodeKind::Sym(_)) {
            continue;
        }
        if cx.method_name(key).is_some() {
            continue;
        }
        // Pair key sym `:in` with non-nil value.
        let NodeKind::Sym(sym) = *cx.kind(key) else {
            continue;
        };
        if cx.symbol_str(sym) != "in" {
            continue;
        }
        if matches!(*cx.kind(value), NodeKind::Nil) {
            continue;
        }
        return true;
    }
    false
}

fn build_message(cx: &Cx<'_>, method_name: &str, node: NodeId, strict: bool) -> String {
    if strict {
        let safe = safe_method(cx, method_name, node);
        MSG_TMPL
            .replace("%<current>s", &format!("Time.{method_name}"))
            .replace("%<prefer>s", &format!("Time.zone.{safe}"))
    } else {
        let safe = safe_method(cx, method_name, node);
        let mut acceptable = vec![
            format!("`Time.zone.{safe}`"),
            "`Time.current`".to_owned(),
        ];
        for am in ACCEPTED_METHODS {
            acceptable.push(format!("`Time.{method_name}.{am}`"));
        }
        MSG_ACCEPTABLE_TMPL
            .replace("%<current>s", &format!("Time.{method_name}"))
            .replace("%<prefer>s", &acceptable.join(", "))
    }
}

fn safe_method(cx: &Cx<'_>, method_name: &str, node: NodeId) -> String {
    if method_name == "new" || method_name == "current" {
        replacement(cx, node)
    } else {
        method_name.to_owned()
    }
}

fn replacement(cx: &Cx<'_>, node: NodeId) -> String {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return "now".to_owned();
    }
    if matches!(*cx.kind(args[0]), NodeKind::Str(_)) {
        "parse".to_owned()
    } else {
        "local".to_owned()
    }
}

fn autocorrect(node: NodeId, cx: &Cx<'_>, chain: &[String], strict: bool) {
    // Insert `.zone` after the `Time` const receiver.
    if let Some(recv) = cx.call_receiver(node).get()
        && matches!(*cx.kind(recv), NodeKind::Const { .. })
    {
        let src = cx.raw_source(cx.range(recv)).to_owned();
        cx.emit_edit(cx.range(recv), &format!("{src}.zone"));
    }
    if let Some(method) = cx.method_name(node).map(|s| s.to_owned()) {
        if method == "current" {
            cx.emit_edit(cx.loc(node).name, "now");
        } else if method == "new" {
            let rep = replacement(cx, node);
            cx.emit_edit(cx.loc(node).name, &rep);
        }
    }
    // Strict `DateTime` → `Time` (no-op for the Time-only path, kept for parity).
    let _ = strict;
    // Remove redundant `.in_time_zone` ancestors.
    if !chain.iter().any(|m| m == "in_time_zone" || m == "zone") {
        return;
    }
    // Walk offense node plus ancestors; remove each `.in_time_zone` selector.
    let mut to_remove: Vec<NodeId> = vec![];
    if cx.method_name(node) == Some("in_time_zone") {
        to_remove.push(node);
    }
    for anc in cx.ancestors(node) {
        if !matches!(*cx.kind(anc), NodeKind::Send { .. }) {
            continue;
        }
        if cx.method_name(anc) != Some("in_time_zone") {
            continue;
        }
        to_remove.push(anc);
    }
    for id in to_remove {
        if let Some(dot) = cx.call_operator_loc(id) {
            cx.emit_edit(
                Range {
                    start: dot.start,
                    end: cx.loc(id).name.end,
                },
                "",
            );
        } else {
            cx.emit_edit(cx.loc(id).name, "");
        }
    }
}

/// Upstream `attach_timezone_specifier?` / `TIMEZONE_SPECIFIER`.
fn has_zone_suffix_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Str(sid) => has_zone_suffix(cx.string_str(sid)),
        NodeKind::Sym(sym) => has_zone_suffix(cx.symbol_str(sym)),
        _ => false,
    }
}

fn has_zone_suffix(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // `/([A-Za-z]|[+-]\d{2}:?\d{2})\z/` — trailing letter or offset.
    if s.chars().last().is_some_and(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let b = s.as_bytes();
    // `[+-]DD:DD` (6) or `[+-]DDDD` (5) at end.
    if b.len() >= 6 {
        let t = &b[b.len() - 6..];
        if (t[0] == b'+' || t[0] == b'-')
            && t[1].is_ascii_digit()
            && t[2].is_ascii_digit()
            && t[3] == b':'
            && t[4].is_ascii_digit()
            && t[5].is_ascii_digit()
        {
            return true;
        }
    }
    if b.len() >= 5 {
        let t = &b[b.len() - 5..];
        if (t[0] == b'+' || t[0] == b'-')
            && t.iter().skip(1).all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{TimeZone, TimeZoneOptions, TimeZoneStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn strict_opts() -> TimeZoneOptions {
        TimeZoneOptions {
            enforced_style: TimeZoneStyle::Strict,
        }
    }

    #[test]
    fn flags_time_now() {
        test::<TimeZone>().expect_offense(indoc! {r#"
            Time.now
                 ^^^ Do not use `Time.now` without zone. Use one of `Time.zone.now`, `Time.current`, `Time.now.in_time_zone`, `Time.now.utc`, `Time.now.getlocal`, `Time.now.xmlschema`, `Time.now.iso8601`, `Time.now.jisx0301`, `Time.now.rfc3339`, `Time.now.httpdate`, `Time.now.to_i`, `Time.now.to_f` instead.
        "#});
    }

    #[test]
    fn flags_time_parse() {
        test::<TimeZone>().expect_offense(indoc! {r#"
            Time.parse('2015-03-02T19:05:37')
                 ^^^^^ Do not use `Time.parse` without zone. Use one of `Time.zone.parse`, `Time.current`, `Time.parse.in_time_zone`, `Time.parse.utc`, `Time.parse.getlocal`, `Time.parse.xmlschema`, `Time.parse.iso8601`, `Time.parse.jisx0301`, `Time.parse.rfc3339`, `Time.parse.httpdate`, `Time.parse.to_i`, `Time.parse.to_f` instead.
        "#});
    }

    #[test]
    fn allows_time_zone_now() {
        test::<TimeZone>().expect_no_offenses("Time.zone.now\n");
    }

    #[test]
    fn allows_time_current_flexible() {
        test::<TimeZone>().expect_no_offenses("Time.current\n");
    }

    #[test]
    fn allows_time_current_strict() {
        // `Time.current` is zone-aware; it never flags (no dangerous method).
        test::<TimeZone>()
            .with_options(&strict_opts())
            .expect_no_offenses("Time.current\n");
    }

    #[test]
    fn flags_time_at_in_time_zone_strict() {
        test::<TimeZone>()
            .with_options(&strict_opts())
            .expect_offense(indoc! {r#"
                Time.at(timestamp).in_time_zone
                     ^^ Do not use `Time.at` without zone. Use `Time.zone.at` instead.
            "#});
    }

    #[test]
    fn allows_time_at_in_time_zone_flexible() {
        test::<TimeZone>().expect_no_offenses("Time.at(timestamp).in_time_zone\n");
    }

    #[test]
    fn allows_iso8601_with_zone() {
        test::<TimeZone>().expect_no_offenses("Time.parse('2015-03-02T19:05:37Z')\n");
    }

    #[test]
    fn allows_time_with_offset_option() {
        test::<TimeZone>().expect_no_offenses("Time.new(1988, 3, 15, 3, 0, 0, '-05:00')\n");
    }

    #[test]
    fn flags_string_to_time() {
        test::<TimeZone>().expect_offense(indoc! {r#"
            '2015-03-02T19:05:37'.to_time
                                  ^^^^^^^ Do not use `String#to_time` without zone. Use `Time.zone.parse` instead.
        "#});
    }

    #[test]
    fn allows_string_to_time_with_zone() {
        test::<TimeZone>().expect_no_offenses("'2015-03-02T19:05:37Z'.to_time\n");
    }

    #[test]
    fn does_not_flag_namespaced_time() {
        test::<TimeZone>().expect_no_offenses("Foo::Time.now\n");
    }

    #[test]
    fn corrects_time_now() {
        test::<TimeZone>()
            .expect_correction(
                indoc! {r#"
                    Time.now
                         ^^^ Do not use `Time.now` without zone. Use one of `Time.zone.now`, `Time.current`, `Time.now.in_time_zone`, `Time.now.utc`, `Time.now.getlocal`, `Time.now.xmlschema`, `Time.now.iso8601`, `Time.now.jisx0301`, `Time.now.rfc3339`, `Time.now.httpdate`, `Time.now.to_i`, `Time.now.to_f` instead.
                "#},
                "Time.zone.now\n",
            )
            .expect_no_offenses("Time.zone.now\n");
    }

    #[test]
    fn flags_time_now_in_time_zone_strict() {
        // Flexible allows `in_time_zone`; strict flags the `Time.now` part.
        test::<TimeZone>()
            .with_options(&strict_opts())
            .expect_offense(indoc! {r#"
                Time.now.in_time_zone
                     ^^^ Do not use `Time.now` without zone. Use `Time.zone.now` instead.
            "#});
    }

    #[test]
    fn allows_time_now_in_time_zone_flexible() {
        test::<TimeZone>().expect_no_offenses("Time.now.in_time_zone\n");
    }

    #[test]
    fn corrects_string_to_time() {
        test::<TimeZone>()
            .expect_correction(
                indoc! {r#"
                    '2015-03-02T19:05:37'.to_time
                                          ^^^^^^^ Do not use `String#to_time` without zone. Use `Time.zone.parse` instead.
                "#},
                "Time.zone.parse('2015-03-02T19:05:37')\n",
            )
            .expect_no_offenses("Time.zone.parse('2015-03-02T19:05:37')\n");
    }
}
murphy_plugin_api::submit_cop!(TimeZone);
