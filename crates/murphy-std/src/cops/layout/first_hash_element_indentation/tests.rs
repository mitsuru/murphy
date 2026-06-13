use super::{
    FirstHashElementIndentation, FirstHashElementIndentationOptions, HashElementStyle,
};
use murphy_plugin_api::test_support::{indoc, test};
use murphy_plugin_api::CopOptions;

fn opts(style: HashElementStyle) -> FirstHashElementIndentationOptions {
    FirstHashElementIndentationOptions {
        enforced_style: style,
        indentation_width: Some(2),
    }
}

// ── special_inside_parentheses (default) ────────────────────────────────────

#[test]
fn default_accepts_plain_consistent_hash() {
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        hash = {
          key: :value
        }
    "#});
}

#[test]
fn default_accepts_special_inside_parentheses() {
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        but_in_a_method_call({
                               its_like: :this
                             })
    "#});
}

#[test]
fn default_flags_first_key_not_relative_to_paren() {
    test::<FirstHashElementIndentation>().expect_correction(
        indoc! {r#"
            and_in_a_method_call({
              no: :difference
              ^^^^^^^^^^^^^^^ Use 2 spaces for indentation in a hash, relative to the first position after the preceding left parenthesis.
                                 })
        "#},
        indoc! {r#"
            and_in_a_method_call({
                                   no: :difference
                                 })
        "#},
    );
}

// ── consistent ──────────────────────────────────────────────────────────────

#[test]
fn consistent_accepts_aligned_hash() {
    test::<FirstHashElementIndentation>()
        .with_options(&opts(HashElementStyle::Consistent))
        .expect_no_offenses(indoc! {r#"
            and_in_a_method_call({
              no: :difference
            })
        "#});
}

#[test]
fn consistent_flags_special_inside_parentheses_indent() {
    // Under `consistent`, the first key must be one step past the START of the
    // `{` line (column 0), NOT past the `(`. The right brace must also align to
    // the start of the `{` line, so both the key and the brace are flagged —
    // matching RuboCop's two offenses for this shape.
    test::<FirstHashElementIndentation>()
        .with_options(&opts(HashElementStyle::Consistent))
        .expect_correction(
            indoc! {r#"
                but_in_a_method_call({
                                       its_like: :this
                                       ^^^^^^^^^^^^^^^^ Use 2 spaces for indentation in a hash, relative to the start of the line where the left curly brace is.
                                     })
                                     ^ Indent the right brace the same as the start of the line where the left brace is.
            "#},
            indoc! {r#"
                but_in_a_method_call({
                  its_like: :this
                })
            "#},
        );
}

// ── align_braces ────────────────────────────────────────────────────────────

#[test]
fn align_braces_accepts_brace_aligned_hash() {
    test::<FirstHashElementIndentation>()
        .with_options(&opts(HashElementStyle::AlignBraces))
        .expect_no_offenses(indoc! {r#"
            and_now_for_something = {
                                      completely: :different
                                    }
        "#});
}

#[test]
fn align_braces_flags_right_brace_misalignment() {
    // Under `align_braces` the right brace must align to the `{` column.
    test::<FirstHashElementIndentation>()
        .with_options(&opts(HashElementStyle::AlignBraces))
        .expect_correction(
            indoc! {r#"
                and_now_for_something = {
                                          completely: :different
                }
                ^ Indent the right brace the same as the left brace.
            "#},
            indoc! {r#"
                and_now_for_something = {
                                          completely: :different
                                        }
            "#},
        );
}

// ── same-line guard ─────────────────────────────────────────────────────────

#[test]
fn accepts_single_line_hash() {
    test::<FirstHashElementIndentation>().expect_no_offenses("h = { a: 1, b: 2 }\n");
}

#[test]
fn accepts_first_key_on_brace_line() {
    // When the first key shares the `{` line, the cop returns early and does
    // not check the right brace either.
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        h = { a: 1,
              b: 2 }
    "#});
}

#[test]
fn accepts_empty_hash() {
    test::<FirstHashElementIndentation>().expect_no_offenses("h = {}\n");
}

#[test]
fn ignores_braceless_kwargs_hash() {
    // A braceless kwargs hash (`foo(a: 1)`) has no `{`; the cop must not fire.
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        foo(
          a: 1,
          b: 2,
        )
    "#});
}

// ── right brace on the value line is accepted ───────────────────────────────

#[test]
fn accepts_right_brace_after_value() {
    // When the `}` shares the last value's line, it is accepted (non-ws
    // precedes it).
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        hash = {
          key: :value }
    "#});
}

// ── false-positive corpus (the safe-port bar) ───────────────────────────────

#[test]
fn accepts_idiomatic_assignment_hash() {
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        config = {
          name: "murphy",
          version: 1,
          enabled: true,
        }
    "#});
}

#[test]
fn accepts_nested_hash_value() {
    // A hash nested as a value, each braced hash indented one step past its
    // own `{` line — idiomatic and clean.
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        outer = {
          inner: {
            a: 1,
          },
        }
    "#});
}

#[test]
fn accepts_method_call_hash_argument_special_inside_parens() {
    // Default style: a braced hash argument whose `{` shares the `(` line is
    // indented relative to the position after `(`. `validates(` puts `(` at
    // column 9, so the first key sits at column 12 (10 + width 2) and the right
    // brace at column 10 (the position after `(`). NB: indoc strips the common
    // leading indent, so these columns are absolute in the test source.
    test::<FirstHashElementIndentation>().expect_no_offenses(
        "validates(:name, {\n            presence: true,\n          })\n",
    );
}

#[test]
fn correction_is_idempotent() {
    // Applying the correction once produces clean source on the next pass.
    test::<FirstHashElementIndentation>().expect_no_offenses(indoc! {r#"
        and_in_a_method_call({
                               no: :difference
                             })
    "#});
}

/// Regression (sweep #384 follow-up): the bundled default `IndentationWidth: ~`
/// merges to JSON `null`. With an `Option<i64>` field it must decode rather than
/// error the whole struct and silently discard the user's `EnforcedStyle`.
#[test]
fn null_indentation_width_preserves_other_keys() {
    let opts = <FirstHashElementIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":null}"#,
    )
    .expect("null IndentationWidth must decode, not discard the struct");
    let reference = <FirstHashElementIndentationOptions as CopOptions>::from_config_json(
        br#"{"EnforcedStyle":"consistent","IndentationWidth":4}"#,
    )
    .unwrap();
    assert!(opts.enforced_style == reference.enforced_style);
}
