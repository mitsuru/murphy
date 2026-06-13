use super::IndentationWidth;
use murphy_plugin_api::test_support::{indoc, test};

// ── def / class / module ────────────────────────────────────────────────────

#[test]
fn flags_under_indented_def_body() {
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        def test
         puts 'hello'
        ^ Use 2 (not 1) spaces for indentation.
        end
    "#});
}

#[test]
fn accepts_correctly_indented_def() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        def test
          puts 'hello'
        end
    "#});
}

#[test]
fn flags_under_indented_class_member() {
    // `def test` is at column 1 (expected 2) → offense over the 1-space indent
    // `[0,1)`. Inside that mis-indented def, `puts 'hello'` is at column 2 but
    // `def` is at column 1, a 1-space offset → a second offense over the indent
    // `[1,2)`. Both match RuboCop (it reports each level independently).
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        class A
         def test
        ^ Use 2 (not 1) spaces for indentation.
          puts 'hello'
         ^ Use 2 (not 1) spaces for indentation.
         end
        end
    "#});
}

#[test]
fn accepts_correctly_indented_class() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        class A
          def test
            puts 'hello'
          end
        end
    "#});
}

#[test]
fn accepts_correctly_indented_module() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        module M
          def test
            1
          end
        end
    "#});
}

// ── false-positive corpus (the safe-port gate) ──────────────────────────────

#[test]
fn accepts_single_line_def() {
    // `def foo; bar; end` — body on the keyword line, skip_check? same_line.
    test::<IndentationWidth>().expect_no_offenses("def foo; bar; end\n");
}

#[test]
fn accepts_empty_def() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        def foo
        end
    "#});
}

#[test]
fn accepts_deeply_nested_valid_code() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        class C
          def foo
            if x
              bar
            end
          end
        end
    "#});
}

#[test]
fn accepts_assignment_rhs_if_variable_aligned() {
    // `x = if c ... end` with the variable-aligned body — valid under
    // EndAlignment, must NOT false-fire (assignment-RHS skip).
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        x = if cond
          foo
        end
    "#});
}

#[test]
fn accepts_else_on_same_line_body() {
    // `else do_something` — body not first on its line, skip_check?.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        if x
          a
        else b
        end
    "#});
}

#[test]
fn accepts_valid_if_else() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        if x
          a
        else
          b
        end
    "#});
}

#[test]
fn accepts_valid_block() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        foo do
          bar
        end
    "#});
}

#[test]
fn accepts_valid_case() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        case x
        when 1
          a
        else
          b
        end
    "#});
}

#[test]
fn accepts_valid_while() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        while cond
          do_work
        end
    "#});
}

#[test]
fn accepts_leading_access_modifier() {
    // A class body starting with a bare `private` — select_check_member skips.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        class A
          private

          def foo
            1
          end
        end
    "#});
}

// ── more violations ─────────────────────────────────────────────────────────

#[test]
fn flags_under_indented_block_body() {
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        foo do
         bar
        ^ Use 2 (not 1) spaces for indentation.
        end
    "#});
}

#[test]
fn flags_under_indented_if_body() {
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        if cond
         foo
        ^ Use 2 (not 1) spaces for indentation.
        end
    "#});
}
