use super::{ArgIndentStyle, FirstArgumentIndentation, FirstArgumentIndentationOptions};
use murphy_plugin_api::test_support::{indoc, test};
use murphy_plugin_api::CopOptions;

fn opts(style: ArgIndentStyle) -> FirstArgumentIndentationOptions {
    FirstArgumentIndentationOptions {
        enforced_style: style,
        indentation_width: Some(2),
    }
}

// ── consistent ──────────────────────────────────────────────────────────────

#[test]
fn consistent_accepts_indented_first_arg() {
    test::<FirstArgumentIndentation>()
        .with_options(&opts(ArgIndentStyle::Consistent))
        .expect_no_offenses(indoc! {r#"
            foo(
              bar
            )
        "#});
}

#[test]
fn consistent_flags_under_indented_first_arg() {
    test::<FirstArgumentIndentation>()
        .with_options(&opts(ArgIndentStyle::Consistent))
        .expect_correction(
            indoc! {r#"
                foo(
                bar
                ^^^ Indent the first argument one step more than the start of the previous line.
                )
            "#},
            indoc! {r#"
                foo(
                  bar
                )
            "#},
        );
}

#[test]
fn consistent_flags_over_indented_first_arg() {
    test::<FirstArgumentIndentation>()
        .with_options(&opts(ArgIndentStyle::Consistent))
        .expect_correction(
            indoc! {r#"
                foo(
                      bar
                      ^^^ Indent the first argument one step more than the start of the previous line.
                )
            "#},
            indoc! {r#"
                foo(
                  bar
                )
            "#},
        );
}

#[test]
fn consistent_uses_previous_line_indent() {
    test::<FirstArgumentIndentation>()
        .with_options(&opts(ArgIndentStyle::Consistent))
        .expect_no_offenses(indoc! {r#"
            if condition
              foo(
                bar
              )
            end
        "#});
}

// ── guards ──────────────────────────────────────────────────────────────────

#[test]
fn ignores_single_line_call() {
    test::<FirstArgumentIndentation>().expect_no_offenses("foo(bar, baz)\n");
}

#[test]
fn ignores_first_arg_on_call_line() {
    test::<FirstArgumentIndentation>().expect_no_offenses(indoc! {r#"
        foo(bar,
            baz)
    "#});
}

#[test]
fn ignores_call_without_arguments() {
    test::<FirstArgumentIndentation>().expect_no_offenses(indoc! {r#"
        foo(
        )
    "#});
}

#[test]
fn ignores_bare_operator_method() {
    test::<FirstArgumentIndentation>().expect_no_offenses(indoc! {r#"
        a +
          b
    "#});
}

#[test]
fn ignores_setter_method() {
    test::<FirstArgumentIndentation>().expect_no_offenses(indoc! {r#"
        foo.bar = (
          1)
    "#});
}

// ── special_for_inner_method_call_in_parentheses (default) ──────────────────

#[test]
fn default_accepts_consistent_outer_call() {
    test::<FirstArgumentIndentation>().expect_no_offenses(indoc! {r#"
        foo(
          bar
        )
    "#});
}

#[test]
fn default_flags_outer_call_under_indent() {
    test::<FirstArgumentIndentation>().expect_correction(
        indoc! {r#"
            foo(
            bar
            ^^^ Indent the first argument one step more than the start of the previous line.
            )
        "#},
        indoc! {r#"
            foo(
              bar
            )
        "#},
    );
}

#[test]
fn default_aligns_inner_call_to_inner_method() {
    // `merge`'s first arg should be indented relative to the start of the
    // inner call (`defaults.merge(`), not the outer line.
    test::<FirstArgumentIndentation>().expect_correction(
        indoc! {r#"
            run(:foo, defaults.merge(
              bar: 3))
              ^^^^^^ Indent the first argument one step more than `defaults.merge(`.
        "#},
        indoc! {r#"
            run(:foo, defaults.merge(
                        bar: 3))
        "#},
    );
}

#[test]
fn special_in_parens_requires_parenthesized_outer() {
    // Without parens on the outer call, the special rule does not apply, so
    // the inner arg is checked against the previous line like `consistent`.
    test::<FirstArgumentIndentation>()
        .with_options(&opts(
            ArgIndentStyle::SpecialForInnerMethodCallInParentheses,
        ))
        .expect_no_offenses(indoc! {r#"
            run :foo, defaults.merge(
              bar: 3)
        "#});
}

// ── special_for_inner_method_call ───────────────────────────────────────────

#[test]
fn special_inner_aligns_without_outer_parens() {
    test::<FirstArgumentIndentation>()
        .with_options(&opts(ArgIndentStyle::SpecialForInnerMethodCall))
        .expect_correction(
            indoc! {r#"
                run :foo, defaults.merge(
                  bar: 3)
                  ^^^^^^ Indent the first argument one step more than `defaults.merge(`.
            "#},
            indoc! {r#"
                run :foo, defaults.merge(
                            bar: 3)
            "#},
        );
}

/// Regression (sweep #384 follow-up): the bundled default `IndentationWidth: ~`
/// merges to JSON `null`. With an `Option<i64>` field it must decode rather than
/// error the whole struct and silently discard the user's `EnforcedStyle`.
#[test]
fn null_indentation_width_preserves_other_keys() {
    let opts = <FirstArgumentIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":null}"#,
    )
    .expect("null IndentationWidth must decode, not discard the struct");
    let reference = <FirstArgumentIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":4}"#,
    )
    .unwrap();
    assert!(opts.enforced_style == reference.enforced_style);
}

// ── cross-cop fallback to Layout/IndentationWidth.Width (murphy-kke2) ────────

/// With this cop's own `IndentationWidth` unset, the width comes from the
/// run-wide resolved `Layout/IndentationWidth.Width`. At width 4 the first
/// argument indented 4 (base column 0) is accepted; under the old hardcoded 2
/// it was flagged as over-indented.
#[test]
fn falls_back_to_layout_indentation_width() {
    let opts = FirstArgumentIndentationOptions {
        enforced_style: ArgIndentStyle::Consistent,
        indentation_width: None,
    };
    test::<FirstArgumentIndentation>()
        .with_options(&opts)
        .with_indentation_width(4)
        .expect_no_offenses(indoc! {r#"
            foo(
                bar
            )
        "#});
}
