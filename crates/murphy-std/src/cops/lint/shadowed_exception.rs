//! `Lint/ShadowedException` — avoid rescuing broad exceptions before narrow ones.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ShadowedException
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-brw4]
//! notes: >
//!   Covers modifier rescue exclusion, empty rescue as StandardError, same-rescue
//!   ancestor pairs, duplicate exceptions, and ordered/misordered multiple rescue
//!   groups for common built-in exception classes. v1 cannot constant-resolve
//!   arbitrary user exception classes or compare platform-specific SystemCallError
//!   Errno values, so unknown constants are conservatively ignored except when
//!   shadowed by an earlier Exception group.
//! ```
//!
//! ## Matched shapes
//!
//! - `rescue Exception; ...; rescue StandardError` — broad rescue shadows later narrow rescue
//! - `rescue StandardError, RuntimeError` — broad and narrow exceptions in one group
//! - duplicate exception names in one group
//!
//! ## Autocorrect
//!
//! None.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Do not shadow rescued Exceptions.";

#[derive(Default)]
pub struct ShadowedException;

#[cop(
    name = "Lint/ShadowedException",
    description = "Avoid rescuing a higher level exception before a lower level exception.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ShadowedException {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.loc(node).end_keyword() == Range::ZERO {
            return;
        }
        let NodeKind::Rescue { resbodies, .. } = *cx.kind(node) else {
            return;
        };
        let rescues = cx.list(resbodies);
        let groups = rescues
            .iter()
            .map(|&resbody| exception_group(resbody, cx))
            .collect::<Vec<_>>();

        for (&resbody, group) in rescues.iter().zip(groups.iter()) {
            if contains_multiple_levels(group) {
                cx.emit_offense(rescue_line_range(resbody, cx), MSG, None);
                return;
            }
        }

        for idx in 0..groups.len() {
            if groups[idx + 1..].iter().any(|later| shadows_later(&groups[idx], later)) {
                cx.emit_offense(rescue_line_range(rescues[idx], cx), MSG, None);
                return;
            }
        }
    }
}

fn rescue_line_range(resbody: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(resbody);
    let source = cx.source();
    let line_end = source[r.start as usize..]
        .find('\n')
        .map(|offset| r.start as usize + offset)
        .unwrap_or(r.end as usize);
    Range {
        start: r.start,
        end: line_end as u32,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExceptionClass {
    Known(&'static str),
    Unknown,
    Splat,
}

fn exception_group(resbody: NodeId, cx: &Cx<'_>) -> Vec<ExceptionClass> {
    let NodeKind::Resbody { exceptions, .. } = *cx.kind(resbody) else {
        return Vec::new();
    };
    let exceptions = cx.list(exceptions);
    if exceptions.is_empty() {
        return vec![ExceptionClass::Known("StandardError")];
    }
    exceptions
        .iter()
        .map(|&node| match *cx.kind(node) {
            NodeKind::Splat(_) => ExceptionClass::Splat,
            _ => const_name(node, cx)
                .and_then(known_exception_name)
                .map(ExceptionClass::Known)
                .unwrap_or(ExceptionClass::Unknown),
        })
        .collect()
}

fn const_name(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return None;
    };
    let name = cx.symbol_str(name);
    match scope.get() {
        None => Some(name.to_string()),
        Some(scope) if matches!(cx.kind(scope), NodeKind::Cbase) => Some(name.to_string()),
        Some(scope) => const_name(scope, cx).map(|prefix| format!("{prefix}::{name}")),
    }
}

fn known_exception_name(name: String) -> Option<&'static str> {
    match name.as_str() {
        "Exception" => Some("Exception"),
        "StandardError" => Some("StandardError"),
        "RuntimeError" => Some("RuntimeError"),
        "NameError" => Some("NameError"),
        "NoMethodError" => Some("NoMethodError"),
        "ZeroDivisionError" => Some("ZeroDivisionError"),
        "ArgumentError" => Some("ArgumentError"),
        "SystemCallError" => Some("SystemCallError"),
        "Interrupt" => Some("Interrupt"),
        "SignalException" => Some("SignalException"),
        name if name.starts_with("Errno::") => Some("SystemCallError"),
        _ => None,
    }
}

fn contains_multiple_levels(group: &[ExceptionClass]) -> bool {
    for i in 0..group.len() {
        for j in i + 1..group.len() {
            if comparable_shadow(group[i], group[j]) || comparable_shadow(group[j], group[i]) {
                return true;
            }
        }
    }
    false
}

fn shadows_later(earlier: &[ExceptionClass], later: &[ExceptionClass]) -> bool {
    earlier.iter().any(|&a| {
        later
            .iter()
            .any(|&b| comparable_shadow(a, b) || matches!(a, ExceptionClass::Known("Exception")))
    })
}

fn comparable_shadow(a: ExceptionClass, b: ExceptionClass) -> bool {
    match (a, b) {
        (ExceptionClass::Known(a), ExceptionClass::Known(b)) => a == b || is_ancestor(a, b),
        _ => false,
    }
}

fn is_ancestor(a: &str, b: &str) -> bool {
    match a {
        "Exception" => b != "Exception",
        "StandardError" => matches!(
            b,
            "RuntimeError"
                | "NameError"
                | "NoMethodError"
                | "ZeroDivisionError"
                | "ArgumentError"
                | "SystemCallError"
        ),
        "NameError" => b == "NoMethodError",
        "SignalException" => b == "Interrupt",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ShadowedException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_shadowed_exception_in_later_rescue() {
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue Exception
            ^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              handle_exception
            rescue StandardError
              handle_standard_error
            end
        "#});
    }

    #[test]
    fn flags_shadowed_exception_with_intervening_unknown_rescue() {
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue StandardError
            ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              handle_standard_error
            rescue UnknownException
              handle_unknown
            rescue RuntimeError
              handle_runtime_error
            end
        "#});
    }

    #[test]
    fn flags_multiple_levels_or_duplicates_in_same_rescue() {
        test::<ShadowedException>()
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError, NameError
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  foo
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue NameError, NameError
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  foo
                end
            "#});
    }

    #[test]
    fn flags_top_level_prefixed_and_system_call_error_shadowing() {
        test::<ShadowedException>()
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue ::StandardError
                ^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue RuntimeError
                  handle_runtime_error
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue SystemCallError
                  handle_system_call_error
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue Errno::ENOENT
                  handle_errno
                end
            "#});
    }

    #[test]
    fn accepts_narrow_before_broad_single_rescue_modifier_and_unknowns() {
        test::<ShadowedException>()
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue StandardError
                  handle_standard_error
                rescue Exception
                  handle_exception
                end
            "#})
            .expect_no_offenses("foo rescue nil\n")
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue UnknownException
                  handle_unknown
                rescue StandardError
                  handle_standard_error
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ShadowedException);
