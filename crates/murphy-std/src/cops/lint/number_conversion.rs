//! `Lint/NumberConversion` — flags dangerous number conversions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NumberConversion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/NumberConversion: direct `to_i`/`to_f`/`to_c`/`to_r`
//!   calls with no arguments on non-numeric receivers, plus symbol forms
//!   (`&:to_i`, `try(:to_f)`, `send(:to_c)`, any single-argument call with a
//!   conversion symbol). AllowedMethods, AllowedPatterns (unanchored regex on
//!   the receiver method name), and IgnoredClasses (default Time, DateTime)
//!   are supported. Autocorrect to Kernel constructors is unsafe
//!   (`SafeAutoCorrect: false`) and mirrors RuboCop.
//! ```
//!
//! ## Matched shapes
//! - `"123".to_i` — `to_i` on a string literal silently returns 0 on failure
//! - `"10.2".to_f` — `to_f` on a string literal
//! - `"10".to_c` — `to_c` on a string literal
//! - `"1/3".to_r` — `to_r` on a string literal
//! - Variable and expression receivers (`foo.to_i`)
//! - Symbol forms: `["1"].map(&:to_i)`, `foo.try(:to_f)`, `bar.send(:to_c)`
//!
//! Numeric receivers (`42.to_i`), conversion-method receivers
//! (`Integer(x).to_f`, `foo.to_i.to_f` inner is flagged but outer is allowed),
//! and methods on ignored class instances (`Time.now.to_i`) are not flagged.
//! `to_i` with an explicit base (`"10".to_i(10)`) is not flagged, matching
//! RuboCop's zero-argument `to_method` pattern.
//!
//! ## Options
//! - `AllowedMethods` (default: `[]`) — method names whose return values
//!   may safely call number conversion methods.
//! - `AllowedPatterns` (default: `[]`) — unanchored regex patterns matched
//!   against the receiver method name.
//! - `IgnoredClasses` (default: `["Time", "DateTime"]`) — classes whose
//!   instances may safely call number conversion methods.
//! - `AllowedClasses` (default: `[]`, deprecated alias for `IgnoredClasses`)
//!   — kept for backward compatibility with the initial partial port.
//!
//! ## Autocorrect
//! Unsafe (`SafeAutoCorrect: false`), mirroring RuboCop:
//! - `"10".to_i` → `Integer("10", 10)`
//! - `"10.2".to_f` → `Float("10.2")`
//! - `"10".to_c` → `Complex("10")`
//! - `"1/3".to_r` → `Rational("1/3")`
//! - `map(&:to_i)` → `map { |i| Integer(i, 10) }`
//! - `foo.try(:to_f)` → `foo.try { |i| Float(i) }`

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const TO_METHODS: &[&str] = &["to_i", "to_f", "to_c", "to_r"];
const CONVERSION_METHODS: &[&str] = &[
    "Integer", "Float", "Complex", "Rational", "to_i", "to_f", "to_c", "to_r",
];

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
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns of receiver method names whose number conversions are allowed."
    )]
    pub allowed_patterns: Vec<String>,

    #[option(
        name = "IgnoredClasses",
        default = ["Time", "DateTime"],
        description = "Classes whose instances may safely call number conversion methods."
    )]
    pub ignored_classes: Vec<String>,

    #[option(
        name = "AllowedClasses",
        default = [],
        description = "Deprecated alias for IgnoredClasses.",
        deprecated = "IgnoredClasses"
    )]
    pub allowed_classes: Vec<String>,
}

#[cop(
    name = "Lint/NumberConversion",
    description = "Flags dangerous number conversions (e.g. `\"10\".to_i`).",
    default_severity = "warning",
    default_enabled = false,
    safe_autocorrect = false,
    options = Options,
)]
impl NumberConversion {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Direct `receiver.to_i` etc. only when the call itself is a
        // conversion method; symbol forms are handled generically.
        if let Some(method) = cx.method_name(node)
            && TO_METHODS.contains(&method)
        {
            check_direct(node, cx);
            return;
        }
        check_symbol(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(method) = cx.method_name(node)
            && TO_METHODS.contains(&method)
        {
            check_direct(node, cx);
            return;
        }
        check_symbol(node, cx);
    }
}

fn check_direct(node: NodeId, cx: &Cx<'_>) {
    let Some(method_name) = cx.method_name(node) else {
        return;
    };
    // RuboCop's `to_method` pattern is `(call $_ ${...})` with no trailing
    // `...`: only zero-argument calls match. `"10".to_i(10)` is allowed.
    if !cx.call_arguments(node).is_empty() {
        return;
    }

    let Some(receiver_id) = cx.call_receiver(node).get() else {
        return;
    };

    // Skip numeric literal receivers (42.to_i is safe). No Begin unwrapping:
    // RuboCop flags `(42).to_i` because `begin` is not numeric.
    if cx.is_numeric(receiver_id) {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    // If receiver is a call, check its method name.
    if matches!(
        *cx.kind(receiver_id),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    )
        && let Some(receiver_method) = cx.method_name(receiver_id)
    {
        // Skip if receiver is a conversion method (Integer, Float, etc.,
        // including chained to_i/to_f/to_c/to_r).
        if CONVERSION_METHODS.contains(&receiver_method) {
            return;
        }
        // Skip if receiver method is allowed explicitly or by pattern.
        if opts.allowed_methods.iter().any(|m| m == receiver_method)
            || cx.matches_any_pattern(receiver_method, &opts.allowed_patterns)
        {
            return;
        }
    }

    // Check IgnoredClasses (plus deprecated AllowedClasses alias):
    // walk up receiver chain to find top receiver.
    if let Some(top) = top_receiver(receiver_id, cx)
        && let NodeKind::Const { name, .. } = *cx.kind(top)
    {
        let const_name = cx.symbol_str(name);
        if opts.ignored_classes.iter().any(|c| c == const_name)
            || opts.allowed_classes.iter().any(|c| c == const_name)
        {
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
    cx.emit_edit(cx.range(node), &correct);
}

/// Symbol/block-pass form: any single-argument call whose argument is
/// `(sym :to_i)` or `(block-pass (sym :to_i))`, e.g. `map(&:to_i)`,
/// `foo.try(:to_f)`, `bar.send(:to_c)`. Mirrors RuboCop's
/// `to_method_symbol` + `arguments.one?` guard. Bare calls (`try(:to_f)`)
/// are flagged; the outer receiver is irrelevant.
fn check_symbol(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let arg = args[0];
    let Some(conv_method) = conversion_sym_name(arg, cx) else {
        return;
    };

    let sym_src = cx.raw_source(cx.range(arg));
    let correct_block = correct_sym_method(conv_method);
    let msg = format!(
        "Replace unsafe number conversion with number class parsing, \
         instead of using `{sym_src}`, \
         use stricter `{correct_block}`."
    );
    cx.emit_offense(cx.range(node), &msg, None);

    // RuboCop: replace the sym/block-pass with `{ |i| ... }`; if the outer
    // call is parenthesized, `(` becomes a space and `)` is removed, so
    // `map(&:to_i)` becomes `map { |i| Integer(i, 10) }`.
    if cx.is_parenthesized(node) {
        let begin = cx.loc(node).begin();
        let end = cx.loc(node).end();
        if begin != murphy_plugin_api::Range::ZERO {
            cx.emit_edit(begin, " ");
        }
        if end != murphy_plugin_api::Range::ZERO {
            cx.emit_edit(end, "");
        }
    }
    cx.emit_edit(cx.range(arg), &correct_block);
}

/// If `arg` is a conversion symbol form, return its method name.
fn conversion_sym_name<'a>(arg: NodeId, cx: &'a Cx<'a>) -> Option<&'a str> {
    match *cx.kind(arg) {
        NodeKind::Sym(sym) => {
            let name = cx.symbol_str(sym);
            if TO_METHODS.contains(&name) {
                Some(name)
            } else {
                None
            }
        }
        NodeKind::BlockPass(inner) => {
            let inner_id = inner.get()?;
            let NodeKind::Sym(sym) = *cx.kind(inner_id) else {
                return None;
            };
            let name = cx.symbol_str(sym);
            if TO_METHODS.contains(&name) {
                Some(name)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Walk up the send/csend chain and return the top receiver.
/// Returns `None` if a bare send (no receiver) is encountered mid-chain.
/// No Begin unwrapping, matching RuboCop's
/// `receiver = receiver.receiver until receiver.receiver.nil?`
/// (`(Time.now).to_i` is flagged because the top is a `begin`, not a const).
fn top_receiver(mut node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    loop {
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

fn correct_sym_method(method: &str) -> String {
    let body = match method {
        "to_i" => "Integer(i, 10)".to_string(),
        "to_f" => "Float(i)".to_string(),
        "to_c" => "Complex(i)".to_string(),
        "to_r" => "Rational(i)".to_string(),
        _ => String::new(),
    };
    format!("{{ |i| {body} }}")
}

murphy_plugin_api::submit_cop!(NumberConversion);

#[cfg(test)]
mod tests {
    use super::{NumberConversion, Options};
    use murphy_plugin_api::Cop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn unsafe_autocorrect_metadata_is_declared() {
        assert_eq!(<NumberConversion as Cop>::SAFE_AUTOCORRECT, Some(false));
    }

    // ── direct offense shapes + autocorrect ──────────────────────────────

    #[test]
    fn flags_string_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                "10".to_i
                ^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_i`, use stricter `Integer("10", 10)`.
            "#},
            "Integer(\"10\", 10)\n",
        );
    }

    #[test]
    fn flags_string_to_f() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                "10.2".to_f
                ^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10.2".to_f`, use stricter `Float("10.2")`.
            "#},
            "Float(\"10.2\")\n",
        );
    }

    #[test]
    fn flags_string_to_c() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                "10".to_c
                ^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_c`, use stricter `Complex("10")`.
            "#},
            "Complex(\"10\")\n",
        );
    }

    #[test]
    fn flags_string_to_r() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                "1/3".to_r
                ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"1/3".to_r`, use stricter `Rational("1/3")`.
            "#},
            "Rational(\"1/3\")\n",
        );
    }

    #[test]
    fn flags_variable_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                string_value = '10'
                string_value.to_i
                ^^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `string_value.to_i`, use stricter `Integer(string_value, 10)`.
            "#},
            "string_value = '10'\nInteger(string_value, 10)\n",
        );
    }

    #[test]
    fn flags_hash_access_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                params = { id: 10 }
                params[:id].to_i
                ^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `params[:id].to_i`, use stricter `Integer(params[:id], 10)`.
            "#},
            "params = { id: 10 }\nInteger(params[:id], 10)\n",
        );
    }

    #[test]
    fn flags_array_access_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                args = [1,2,3]
                args[0].to_i
                ^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `args[0].to_i`, use stricter `Integer(args[0], 10)`.
            "#},
            "args = [1,2,3]\nInteger(args[0], 10)\n",
        );
    }

    // ── direct: to_i with explicit base is allowed ───────────────────────

    #[test]
    fn accepts_to_i_with_base() {
        test::<NumberConversion>().expect_no_offenses("\"10\".to_i(10)\n");
    }

    #[test]
    fn accepts_variable_to_i_with_base() {
        test::<NumberConversion>().expect_no_offenses("foo.to_i(10)\n");
    }

    #[test]
    fn flags_to_i_with_empty_parens() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                foo.to_i()
                ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `foo.to_i`, use stricter `Integer(foo, 10)`.
            "#},
            "Integer(foo, 10)\n",
        );
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

    #[test]
    fn accepts_chained_conversion_inner_only() {
        // `foo.to_i.to_f`: outer `to_f` is allowed (receiver is `to_i`),
        // inner `foo.to_i` is still flagged.
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                foo.to_i.to_f
                ^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `foo.to_i`, use stricter `Integer(foo, 10)`.
            "#},
            "Integer(foo, 10).to_f\n",
        );
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
            .expect_correction(
                indoc! {r#"
                    10.hours.to_i
                    ^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `10.hours.to_i`, use stricter `Integer(10.hours, 10)`.
                "#},
                "Integer(10.hours, 10)\n",
            );
    }

    // ── AllowedPatterns ──────────────────────────────────────────────────

    #[test]
    fn accepts_allowed_pattern() {
        let opts = Options {
            allowed_patterns: vec!["min*".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("10.minutes.to_i\n");
    }

    #[test]
    fn accepts_allowed_pattern_substring() {
        let opts = Options {
            allowed_patterns: vec!["min*".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("10.min125.to_i\n");
    }

    #[test]
    fn flags_non_matching_pattern() {
        let opts = Options {
            allowed_patterns: vec!["min*".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                10.hours.to_i
                ^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `10.hours.to_i`, use stricter `Integer(10.hours, 10)`.
            "#});
    }

    // ── IgnoredClasses ───────────────────────────────────────────────────

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
    fn flags_non_ignored_class_to_i() {
        test::<NumberConversion>().expect_offense(indoc! {r#"
            MyClass.new.to_i
            ^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `MyClass.new.to_i`, use stricter `Integer(MyClass.new, 10)`.
        "#});
    }

    #[test]
    fn accepts_user_configured_ignored_class() {
        let opts = Options {
            ignored_classes: vec![
                "Time".to_string(),
                "DateTime".to_string(),
                "MyDuration".to_string(),
            ],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("MyDuration.new(10).to_i\n");
    }

    #[test]
    fn accepts_deprecated_allowed_classes_alias() {
        let opts = Options {
            allowed_classes: vec!["MyDuration".to_string()],
            ..Default::default()
        };
        test::<NumberConversion>()
            .with_options(&opts)
            .expect_no_offenses("MyDuration.new(10).to_i\n");
    }

    // ── safe navigation (csend) ──────────────────────────────────────────

    #[test]
    fn flags_string_safe_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                "10"&.to_i
                ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `"10".to_i`, use stricter `Integer("10", 10)`.
            "#},
            "Integer(\"10\", 10)\n",
        );
    }

    // ── symbol / block-pass forms ────────────────────────────────────────

    #[test]
    fn flags_map_block_pass_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                ["1", "2"].map(&:to_i)
                ^^^^^^^^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `&:to_i`, use stricter `{ |i| Integer(i, 10) }`.
            "#},
            "[\"1\", \"2\"].map { |i| Integer(i, 10) }\n",
        );
    }

    #[test]
    fn flags_try_to_f() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                foo.try(:to_f)
                ^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `:to_f`, use stricter `{ |i| Float(i) }`.
            "#},
            "foo.try { |i| Float(i) }\n",
        );
    }

    #[test]
    fn flags_send_to_c() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                bar.send(:to_c)
                ^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `:to_c`, use stricter `{ |i| Complex(i) }`.
            "#},
            "bar.send { |i| Complex(i) }\n",
        );
    }

    #[test]
    fn flags_bare_try_to_f() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                try(:to_f)
                ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `:to_f`, use stricter `{ |i| Float(i) }`.
            "#},
            "try { |i| Float(i) }\n",
        );
    }

    #[test]
    fn flags_generic_single_sym_arg() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                foo(:to_i)
                ^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `:to_i`, use stricter `{ |i| Integer(i, 10) }`.
            "#},
            "foo { |i| Integer(i, 10) }\n",
        );
    }

    #[test]
    fn flags_csend_try_to_f() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                foo&.try(:to_f)
                ^^^^^^^^^^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `:to_f`, use stricter `{ |i| Float(i) }`.
            "#},
            "foo&.try { |i| Float(i) }\n",
        );
    }

    #[test]
    fn accepts_two_sym_args() {
        test::<NumberConversion>().expect_no_offenses("foo(:to_f, bar)\n");
    }

    #[test]
    fn accepts_no_args_try() {
        test::<NumberConversion>().expect_no_offenses("foo.try\n");
    }

    #[test]
    fn accepts_non_conversion_sym() {
        test::<NumberConversion>().expect_no_offenses("foo(:to_s)\n");
    }

    #[test]
    fn accepts_non_conversion_block_pass() {
        test::<NumberConversion>().expect_no_offenses("[\"1\"].map(&:to_s)\n");
    }

    #[test]
    fn flags_block_body_to_i() {
        test::<NumberConversion>().expect_correction(
            indoc! {r#"
                ["1"].map { |x| x.to_i }
                                ^^^^^^ Replace unsafe number conversion with number class parsing, instead of using `x.to_i`, use stricter `Integer(x, 10)`.
            "#},
            "[\"1\"].map { |x| Integer(x, 10) }\n",
        );
    }
}
