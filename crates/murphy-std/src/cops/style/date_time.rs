//! `Style/DateTime` — prefers `Time` over `DateTime`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DateTime
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Disabled by default (Enabled: false in RuboCop's default.yml), matching
//!   RuboCop behaviour.
//!   Flags DateTime.method_name calls (except historical dates), and
//!   something.to_datetime calls (unless AllowCoercion: true).
//!   Historical dates (second argument is a Date constant like Date::ENGLAND)
//!   are accepted per RuboCop's historic_date? matcher.
//!   Autocorrect only applies to the class case (replaces DateTime with Time);
//!   to_datetime calls have no autocorrect (different semantics).
//!   SafeAutoCorrect: false is noted in parity; DateTime and Time have subtle
//!   differences in handling of timezones and DST.
//!   Const nodes have loc.name = Range::ZERO in Murphy (push uses Range::ZERO);
//!   the DateTime name token is found via token scan within the const range.
//! ```
//!
//! ## Matched shapes
//!
//! - `DateTime.method_name(...)` — any call on the `DateTime` constant
//! - `something.to_datetime` — coercion calls (when `AllowCoercion: false`)
//!
//! ## Skip conditions
//!
//! - Historical dates: second arg is `Date::CONSTANT` (e.g. `Date::ENGLAND`)
//!
//! ## Autocorrect
//!
//! Class case: replace the `DateTime` name token in the receiver const with `Time`.
//! Coercion case: no autocorrect.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const CLASS_MSG: &str = "Prefer `Time` over `DateTime`.";
const COERCION_MSG: &str = "Do not use `#to_datetime`.";

#[derive(Default)]
pub struct DateTime;

/// Configuration options for Style/DateTime.
#[derive(Default, Debug)]
pub struct DateTimeOptions {
    /// When true, `something.to_datetime` is accepted.
    pub allow_coercion: bool,
}

impl CopOptions for DateTimeOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, murphy_plugin_api::ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(murphy_plugin_api::ConfigError::parse)?;
        let obj = value
            .as_object()
            .ok_or_else(murphy_plugin_api::ConfigError::not_an_object)?;

        let allow_coercion = match obj.get("AllowCoercion") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| murphy_plugin_api::ConfigError::type_mismatch("AllowCoercion", "bool"))?,
        };

        Ok(Self { allow_coercion })
    }

    fn to_config_json(&self) -> String {
        format!(r#"{{"AllowCoercion": {}}}"#, self.allow_coercion)
    }
}

#[cop(
    name = "Style/DateTime",
    description = "Use Time over DateTime.",
    default_severity = "warning",
    default_enabled = false,
    options = DateTimeOptions,
)]
impl DateTime {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns true if `node` is a call on the `DateTime` constant
/// (e.g. `DateTime.now`, `DateTime.iso8601(...)`).
fn is_date_time_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_global_const(recv, "DateTime")
}

/// Returns true if `node` is a `to_datetime` call (e.g. `something.to_datetime`).
fn is_to_datetime(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(node) == Some("to_datetime")
}

/// Returns true if the call has a second argument that is a constant rooted in
/// `Date` — e.g. `Date::ENGLAND`, `Date::JULIAN`. This is RuboCop's
/// `historic_date?` check.
fn is_historic_date(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    let Some(&second_arg) = args.get(1) else {
        return false;
    };
    // Match: (const (const {nil? cbase} :Date) :SOMETHING)
    let NodeKind::Const { scope, .. } = *cx.kind(second_arg) else {
        return false;
    };
    let Some(scope_id) = scope.get() else {
        return false;
    };
    cx.is_global_const(scope_id, "Date")
}

/// Find the token spelling `DateTime` within the receiver const node's range.
/// Const nodes have `loc.name = Range::ZERO` in Murphy, so we scan tokens.
fn find_datetime_token(recv: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let recv_range = cx.range(recv);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < recv_range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < recv_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"DateTime"
        })
        .map(|t| t.range)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<DateTimeOptions>();

    let is_dt_call = is_date_time_call(node, cx);
    let is_coercion = is_to_datetime(node, cx);

    if !is_dt_call && !is_coercion {
        return;
    }
    if is_coercion && opts.allow_coercion {
        return;
    }
    if is_dt_call && is_historic_date(node, cx) {
        return;
    }

    let message = if is_coercion { COERCION_MSG } else { CLASS_MSG };

    cx.emit_offense(cx.range(node), message, None);

    // Autocorrect only for the class case: replace DateTime name token with Time.
    if is_dt_call
        && let Some(recv) = cx.call_receiver(node).get()
        && let Some(dt_range) = find_datetime_token(recv, cx)
    {
        cx.emit_edit(dt_range, "Time");
    }
}

#[cfg(test)]
mod tests {
    use super::{DateTime, DateTimeOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_datetime_now() {
        test::<DateTime>().expect_correction(
            indoc! {"
                DateTime.now
                ^^^^^^^^^^^^ Prefer `Time` over `DateTime`.
            "},
            "Time.now\n",
        );
    }

    #[test]
    fn flags_datetime_iso8601_single_arg() {
        test::<DateTime>().expect_correction(
            indoc! {r#"
                DateTime.iso8601('2016-06-29')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Time` over `DateTime`.
            "#},
            "Time.iso8601('2016-06-29')\n",
        );
    }

    #[test]
    fn accepts_datetime_iso8601_historic() {
        // Second arg is Date::ENGLAND — historical date, skip.
        test::<DateTime>().expect_no_offenses(indoc! {r#"
            DateTime.iso8601('1751-04-23', Date::ENGLAND)
        "#});
    }

    #[test]
    fn flags_to_datetime_by_default() {
        test::<DateTime>().expect_offense(indoc! {"
            something.to_datetime
            ^^^^^^^^^^^^^^^^^^^^^ Do not use `#to_datetime`.
        "});
    }

    #[test]
    fn accepts_to_datetime_when_allowed() {
        test::<DateTime>()
            .with_options(&DateTimeOptions { allow_coercion: true })
            .expect_no_offenses("something.to_datetime\n");
    }

    #[test]
    fn flags_cbase_datetime() {
        test::<DateTime>().expect_correction(
            indoc! {"
                ::DateTime.now
                ^^^^^^^^^^^^^^ Prefer `Time` over `DateTime`.
            "},
            "::Time.now\n",
        );
    }

    #[test]
    fn accepts_non_datetime_const() {
        test::<DateTime>().expect_no_offenses("Time.now\n");
    }

    #[test]
    fn options_allow_coercion_not_bool_errors() {
        use murphy_plugin_api::CopOptions;
        let err = DateTimeOptions::from_config_json(br#"{"AllowCoercion": "yes"}"#)
            .expect_err("wrong type");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowCoercion");
        assert_eq!(*expected, "bool");
    }

    #[test]
    fn options_not_object_errors() {
        use murphy_plugin_api::CopOptions;
        let err = DateTimeOptions::from_config_json(br#"[]"#)
            .expect_err("not an object");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::NotAnObject
        ));
    }
}
murphy_plugin_api::submit_cop!(DateTime);
