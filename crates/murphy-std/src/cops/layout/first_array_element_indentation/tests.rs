use super::{
    ArrayElementStyle, FirstArrayElementIndentation, FirstArrayElementIndentationOptions,
};
use murphy_plugin_api::test_support::{indoc, test};
use murphy_plugin_api::CopOptions;

fn opts(style: ArrayElementStyle) -> FirstArrayElementIndentationOptions {
    FirstArrayElementIndentationOptions {
        enforced_style: style,
        indentation_width: Some(2),
    }
}

// ── special_inside_parentheses (default) ────────────────────────────────────

#[test]
fn default_accepts_plain_consistent_array() {
    test::<FirstArrayElementIndentation>().expect_no_offenses(indoc! {r#"
        array = [
          :value
        ]
    "#});
}

#[test]
fn default_accepts_special_inside_parentheses() {
    test::<FirstArrayElementIndentation>().expect_no_offenses(indoc! {r#"
        but_in_a_method_call([
                               :its_like_this
                             ])
    "#});
}

#[test]
fn default_flags_first_element_not_relative_to_paren() {
    test::<FirstArrayElementIndentation>().expect_correction(
        indoc! {r#"
            and_in_a_method_call([
              :no_difference
              ^^^^^^^^^^^^^^ Use 2 spaces for indentation in an array, relative to the first position after the preceding left parenthesis.
                                 ])
        "#},
        indoc! {r#"
            and_in_a_method_call([
                                   :no_difference
                                 ])
        "#},
    );
}

// ── consistent ──────────────────────────────────────────────────────────────

#[test]
fn consistent_accepts_aligned_array() {
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::Consistent))
        .expect_no_offenses(indoc! {r#"
            and_in_a_method_call([
              :no_difference
            ])
        "#});
}

#[test]
fn consistent_flags_over_indented_first_element() {
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::Consistent))
        .expect_correction(
            indoc! {r#"
                array = [
                    :value
                    ^^^^^^ Use 2 spaces for indentation in an array, relative to the start of the line where the left square bracket is.
                ]
            "#},
            indoc! {r#"
                array = [
                  :value
                ]
            "#},
        );
}

#[test]
fn consistent_flags_special_inside_parentheses_indent() {
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::Consistent))
        .expect_correction(
            indoc! {r#"
                and_in_a_method_call([
                                       :its_like_this
                                       ^^^^^^^^^^^^^^ Use 2 spaces for indentation in an array, relative to the start of the line where the left square bracket is.
                ])
            "#},
            indoc! {r#"
                and_in_a_method_call([
                  :its_like_this
                ])
            "#},
        );
}

// ── align_brackets ──────────────────────────────────────────────────────────

#[test]
fn align_brackets_accepts_aligned() {
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::AlignBrackets))
        .expect_no_offenses(indoc! {r#"
            and_now_for_something = [
                                      :completely_different
                                    ]
        "#});
}

#[test]
fn align_brackets_flags_misaligned_right_bracket() {
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::AlignBrackets))
        .expect_correction(
            indoc! {r#"
                and_now_for_something = [
                                          :completely_different
                ]
                ^ Indent the right bracket the same as the left bracket.
            "#},
            indoc! {r#"
                and_now_for_something = [
                                          :completely_different
                                        ]
            "#},
        );
}

// ── guards: same line, single line ──────────────────────────────────────────

#[test]
fn ignores_single_line_array() {
    test::<FirstArrayElementIndentation>().expect_no_offenses("array = [1, 2, 3]\n");
}

#[test]
fn ignores_first_element_on_bracket_line() {
    test::<FirstArrayElementIndentation>().expect_no_offenses(indoc! {r#"
        array = [:value,
                 :other]
    "#});
}

#[test]
fn ignores_right_bracket_after_last_value() {
    test::<FirstArrayElementIndentation>().expect_no_offenses(indoc! {r#"
        array = [
          :value]
    "#});
}

#[test]
fn ignores_empty_array() {
    test::<FirstArrayElementIndentation>().expect_no_offenses("array = []\n");
}

#[test]
fn ignores_right_bracket_when_first_element_on_bracket_line() {
    // RuboCop returns from `check` entirely when the first element is on the
    // `[` line, so the trailing `]` on its own line is NOT checked. A common
    // trailing-comma layout must not be flagged.
    test::<FirstArrayElementIndentation>().expect_no_offenses(indoc! {r#"
        x = [1,
             2,
             ]
    "#});
}

#[test]
fn checks_right_bracket_of_empty_multiline_array() {
    // Empty arrays have no first element, so `check` still reaches the
    // right-bracket check (RuboCop's `if first_elem` guard).
    test::<FirstArrayElementIndentation>()
        .with_options(&opts(ArrayElementStyle::AlignBrackets))
        .expect_no_offenses(indoc! {r#"
            x = [
                ]
        "#});
}

/// Regression (sweep #384 follow-up): the bundled default `IndentationWidth: ~`
/// merges to JSON `null`. With an `Option<i64>` field it must decode rather than
/// error the whole struct and silently discard the user's `EnforcedStyle`.
#[test]
fn null_indentation_width_preserves_other_keys() {
    let opts = <FirstArrayElementIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":null}"#,
    )
    .expect("null IndentationWidth must decode, not discard the struct");
    let reference = <FirstArrayElementIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":4}"#,
    )
    .unwrap();
    assert!(opts.enforced_style == reference.enforced_style);
}

// ── cross-cop fallback to Layout/IndentationWidth.Width (murphy-kke2) ────────

/// With this cop's own `IndentationWidth` unset, the width comes from the
/// run-wide resolved `Layout/IndentationWidth.Width`. At width 4 a plain array
/// element indented 4 (base column 0) is accepted; under the old hardcoded 2 it
/// was flagged as over-indented.
#[test]
fn falls_back_to_layout_indentation_width() {
    test::<FirstArrayElementIndentation>()
        .with_indentation_width(4)
        .expect_no_offenses(indoc! {r#"
            array = [
                :value
            ]
        "#});
}
