//! `Lint/NumberConversion` — flags dangerous number conversions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NumberConversion
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Core checks for to_i, to_f, to_c, to_r on non-numeric receivers.
//!   AllowedMethods and AllowedClasses options are supported.
//!   Symbol forms (&:to_i, try(:to_f), send(:to_c)), AllowedPatterns (regex),
//!   and autocorrect are v1 gaps.
//! ```
//!
//! ## Matched shapes
//! - `"123".to_i` — `to_i` on a string literal silently returns 0 on failure
//! - `"10.2".to_f` — `to_f` on a string literal
//! - `"10".to_c` — `to_c` on a string literal
//! - `"1/3".to_r` — `to_r` on a string literal
//! - Variable and expression receivers (`foo.to_i`)
//!
//! Numeric receivers (`42.to_i`), conversion-method receivers
//! (`Integer(x).to_f`), and methods on allowed class instances
//! (`Time.now.to_i`) are not flagged.
//!
//! ## Options
//! - `AllowedMethods` (default: `[]`) — method names whose return values
//!   may safely call number conversion methods.
//! - `AllowedClasses` (default: `["Time", "DateTime"]`) — classes whose
//!   instances may safely call number conversion methods.
//!
//! ## Autocorrect
//! None (v1 gap). RuboCop corrects to the corresponding Kernel constructor.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const CONVERSION_METHODS: &[&str] = &["Integer", "Float", "Complex", "Rational"];

#[derive(Default)]
pub struct NumberConversion;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods whose return values may safely call number conversion methods."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "AllowedClasses",
        default = ["Time", "DateTime"],
        description = "Classes whose instances may safely call number conversion methods."
    )]
    pub allowed_classes: Vec<String>,
}

#[cop(
    name = "Lint/NumberConversion",
    description = "Flags dangerous number conversions (e.g. `\"10\".to_i`).",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl NumberConversion {
    #[on_node(kind = "send", methods = ["to_i", "to_f", "to_c", "to_r"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(method) = cx.method_name(node)
            && matches!(method, "to_i" | "to_f" | "to_c" | "to_r")
        {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(method_name) = cx.method_name(node) else {
        return;
    };

    let Some(receiver_id) = cx.call_receiver(node).get() else {
        return;
    };

    // Skip numeric literal receivers (42.to_i is safe)
    if cx.is_numeric(receiver_id) {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    // If receiver is a call, check its method name
    if matches!(
        *cx.kind(receiver_id),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    )
        && let Some(receiver_method) = cx.method_name(receiver_id) {
            // Skip if receiver is a conversion method (Integer, Float, etc.)
            if CONVERSION_METHODS.contains(&receiver_method) {
                return;
            }
            // Skip if receiver method is in AllowedMethods
            if opts.allowed_methods.iter().any(|m| m == receiver_method) {
                return;
            }
        }

    // Check AllowedClasses: walk up receiver chain to find top receiver
    if let Some(top) = top_receiver(receiver_id, cx)
        && let NodeKind::Const { name, .. } = *cx.kind(top)
    {
        let const_name = cx.symbol_str(name);
        if opts.allowed_classes.iter().any(|c| c == const_name) {
            return;
        }
    }

    let receiver_src = cx.raw_source(cx.range(receiver_id));
    let correct = correct_method(method_name, receiver_src);
    let msg = format!(
        "Replace unsafe number conversion with number class parsing, \
         instead of using `{receiver_src}.{method_name}`, \
         use stricter `{correct}`."
    );
    cx.emit_offense(cx.range(node), &msg, None);
}

/// Walk up the send/csend chain and return the top receiver.
/// Returns `None` if a bare send (no receiver) is encountered mid-chain.
fn top_receiver(mut node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    loop {
        while let NodeKind::Begin(list) = *cx.kind(node) {
            let children = cx.list(list);
            if children.len() == 1 {
                node = children[0];
            } else {
                break;
            }
        }
        match *cx.kind(node) {
            NodeKind::Send { receiver, .. } => {
                node = receiver.get()?;
            }
            NodeKind::Csend { receiver, .. } => {
                node = receiver;
            }
            _ => return Some(node),
        }
    }
}

fn correct_method(method: &str, receiver_src: &str) -> String {
    match method {
        "to_i" => format!("Integer({receiver_src}, 10)"),
        "to_f" => format!("Float({receiver_src})"),
        "to_c" => format!("Complex({receiver_src})"),
        "to_r" => format!("Rational({receiver_src})"),
        _ => String::new(),
    }
}

murphy_plugin_api::submit_cop!(NumberConversion);

#[cfg(test)]
mod tests {
    use super::{NumberConversion, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // ── offense shapes ───────────────────────────────────────────────────

    #[test]
    fn flags_string_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            "10".to_i
            ^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_i`, use stricter `Integer("10", 10)`.
        "#});
    }

    #[test]
    fn flags_string_to_f() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            "10.2".to_f
            ^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10.2".to_f`, use stricter `Float("10.2")`.
        "#});
    }

    #[test]
    fn flags_string_to_c() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            "10".to_c
            ^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_c`, use stricter `Complex("10")`.
        "#});
    }

    #[test]
    fn flags_string_to_r() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            "1/3".to_r
            ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"1/3".to_r`, use stricter `Rational("1/3")`.
        "#});
    }

    #[test]
    fn flags_variable_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            string_value = '10'
            string_value.to_i
            ^^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `string_value.to_i`, use stricter `Integer(string_value, 10)`.
        "#});
    }

    #[test]
    fn flags_hash_access_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            params = { id: 10 }
            params[:id].to_i
            ^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `params[:id].to_i`, use stricter `Integer(params[:id], 10)`.
        "#});
    }

    #[test]
    fn flags_array_access_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            args = [1,2,3]
            args[0].to_i
            ^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `args[0].to_i`, use stricter `Integer(args[0], 10)`.
        "#});
    }

    // ── no-offense: numeric receivers ────────────────────────────────────

    #[test]
    fn accepts_integer_to_i() {
        test::<NumberConversion>().expect_no_offenses("42.to_i\n");
    }

    #[test]
    fn accepts_float_to_i() {
        test::<NumberConversion>().expect_no_offenses("42.0.to_i\n");
    }

    #[test]
    fn accepts_integer_to_f() {
        test::<NumberConversion>().expect_no_offenses("42.to_f\n");
    }

    #[test]
    fn accepts_float_to_f() {
        test::<NumberConversion>().expect_no_offenses("42.0.to_f\n");
    }

    #[test]
    fn accepts_integer_to_c() {
        test::<NumberConversion>().expect_no_offenses("42.to_c\n");
    }

    #[test]
    fn accepts_integer_to_r() {
        test::<NumberConversion>().expect_no_offenses("42.to_r\n");
    }

    // ── no-offense: bare to_i without receiver ───────────────────────────

    #[test]
    fn accepts_bare_to_i() {
        test::<NumberConversion>().expect_no_offenses("to_i\n");
    }

    // ── no-offense: conversion method receivers ──────────────────────────

    #[test]
    fn accepts_integer_wrapper_to_f() {
        test::<NumberConversion>().expect_no_offenses("Integer(var, 10).to_f\n");
    }

    #[test]
    fn accepts_float_wrapper_to_i() {
        test::<NumberConversion>().expect_no_offenses("Float(var).to_i\n");
    }

    #[test]
    fn accepts_complex_wrapper_to_f() {
        test::<NumberConversion>().expect_no_offenses("Complex(var).to_f\n");
    }

    #[test]
    fn accepts_rational_wrapper_to_f() {
        test::<NumberConversion>().expect_no_offenses("Rational(var).to_i\n");
    }

    // ── AllowedMethods ───────────────────────────────────────────────────

    #[test]
    fn accepts_allowed_method() {
        let opts = Options {
            allowed_methods: vec!["minutes".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("10.minutes.to_i\n");
    }

    #[test]
    fn flags_non_allowed_method() {
        let opts = Options {
            allowed_methods: vec!["minutes".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                10.hours.to_i
                ^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `10.hours.to_i`, use stricter `Integer(10.hours, 10)`.
            "#});
    }

    // ── AllowedClasses ───────────────────────────────────────────────────

    #[test]
    fn accepts_time_now_to_i() {
        test::<NumberConversion>().expect_no_offenses("Time.now.to_i\n");
    }

    #[test]
    fn accepts_time_now_to_f() {
        test::<NumberConversion>().expect_no_offenses("Time.now.to_f\n");
    }

    #[test]
    fn accepts_datetime_to_i() {
        test::<NumberConversion>().expect_no_offenses("DateTime.new(2012, 8, 29, 22, 35, 0).to_i\n");
    }

    #[test]
    fn accepts_time_chained_to_i() {
        test::<NumberConversion>().expect_no_offenses("Time.now.to_datetime.to_i\n");
    }

    #[test]
    fn accepts_dotted_const_to_i() {
        test::<NumberConversion>().expect_no_offenses("Time.strptime(\"2000-10-31\", \"%Y-%m-%d\").to_i\n");
    }

    #[test]
    fn flags_non_allowed_class_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            MyClass.new.to_i
            ^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `MyClass.new.to_i`, use stricter `Integer(MyClass.new, 10)`.
        "#});
    }

    #[test]
    fn accepts_user_configured_class() {
        let opts = Options {
            allowed_classes: vec!["Time".to_string(), "DateTime".to_string(), "MyDuration".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("MyDuration.new(10).to_i\n");
    }

    // ── safe navigation (csend) ──────────────────────────────────────────

    #[test]
    fn flags_string_safe_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            "10"&.to_i
            ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_i`, use stricter `Integer("10", 10)`.
        "#});
    }
}
