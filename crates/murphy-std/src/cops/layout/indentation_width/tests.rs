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

// ── adjacent def modifier (`private def …`) ─────────────────────────────────

#[test]
fn accepts_modifier_wrapped_def_singleton() {
    // `private_class_method def self.foo` — RuboCop's `adjacent_def_modifier?`
    // makes the indentation base the modifier send's column (default
    // `Layout/DefEndAlignment` `start_of_line`), not the inner `def` keyword.
    // Previously this false-fired with `Use 2 (not -19)` (Mastodon:
    // app/helpers/languages_helper.rb:253).
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        module M
          private_class_method def self.locale_name_for_sorting(locale)
            if locale
              locale
            end
          end
        end
    "#});
}

#[test]
fn accepts_modifier_wrapped_def_instance() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        private def foo
          bar
        end
    "#});
}

#[test]
fn accepts_modifier_wrapped_def_after_inline_statement() {
    // The modifier is not at the start of the line, so measuring the body
    // against its column would produce a negative indentation width.
    test::<IndentationWidth>()
        .expect_no_offenses("class Foo\n  x = 1; private def foo\n    bar\n  end\nend\n");
}

#[test]
fn flags_misindented_modifier_wrapped_def_body() {
    // The modifier base still catches genuine misindentation: `bar` is indented
    // 4 past the `private` column, not 2.
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        private def foo
            bar
        ^^^^ Use 2 (not 4) spaces for indentation.
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

#[test]
fn accepts_case_else_body_indented_from_else_keyword() {
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
fn flags_case_else_body_indented_from_the_wrong_keyword() {
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        case x
          when 1
            a
        else
            b
        ^^^^ Use 2 (not 4) spaces for indentation.
        end
    "#});
}

#[test]
fn flags_block_body_when_end_is_misaligned() {
    // RuboCop bases the body on `end` (`start_of_line`): `bar` at column 2 is
    // -2 past the column-4 `end`, so this flags (verified vs RuboCop 1.91.0).
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        foo do
          bar
          ^^ Use 2 (not -2) spaces for indentation.
            end
    "#});
}

#[test]
fn accepts_block_body_aligned_past_misaligned_end() {
    // `bar` at column 4 is exactly 2 past the column-2 `end` — RuboCop
    // accepts (verified vs RuboCop 1.91.0).
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        foo do
            bar
          end
    "#});
}

#[test]
fn flags_brace_block_body_against_its_opener_line() {
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        foo {
            bar
        ^^^^ Use 2 (not 4) spaces for indentation.
        }
    "#});
}

#[test]
fn accepts_assigned_block_body_indented_from_opener_line() {
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        result = foo do
          bar
        end
    "#});
}

// ── Mastodon batch-3 FPs (murphy-bjrg.3): block body measures from `end` ────
// RuboCop's default `EnforcedStyleAlignWith: start_of_line` bases the block
// body on the closing `end`/`}` when it begins its line — NOT on the call's
// opening line. Each shape below is a reduced Mastodon hit that RuboCop 1.91.0
// (full config, TargetRubyVersion 3.3) accepts.

#[test]
fn accepts_hash_body_inside_do_block() {
    // app/lib/annual_report/top_hashtags.rb: the `{` body sits 16 past the
    // `top:` line but exactly 2 past the block's `end`.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        def generate
          {
            top: items.map do |x|
                            {
                              name: x,
                            }
                          end,
          }
        end
    "#});
}

#[test]
fn accepts_chained_call_inside_brace_block() {
    // app/models/tag.rb (`scope :recently_used, lambda { ... }`): the chain
    // body is 24 past the statement start but 2 past the closing `}`.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        scope :recently_used, lambda { |account|
                                joins(:statuses)
                                  .where(x: 1)
                              }
    "#});
}

#[test]
fn accepts_chain_block_body_past_receiver() {
    // app/workers/move_worker.rb: `.in_batches do` body is 4 past the chain
    // receiver start but 2 past the block's `end`.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        source_local_followers
          .where(x: 1)
          .in_batches do |follows|
            ListAccount.where(follow: follows)
          end
    "#});
}

#[test]
fn accepts_brace_body_after_chained_do_block() {
    // spec/support/fasp/provider_request_helper.rb: the `{` body is 4 past
    // the `stub_request` line but 2 past the block's `end`.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        stub_request(method, url)
          .to_return do |_request|
            {
              status: 200,
            }
          end
    "#});
}

#[test]
fn accepts_rspec_allow_chain_block_body() {
    // spec/workers/tagged_collection_resolve_worker_spec.rb: `allow(...).to`
    // chain block body 4 past `allow` but 2 past the block's `end`.
    test::<IndentationWidth>().expect_no_offenses(indoc! {r#"
        allow(service_double)
          .to receive(:call)
          .with(uri, anything) do
            Fabricate(:thing, uri: uri)
          end
    "#});
}

#[test]
fn accepts_block_body_when_end_shares_its_line() {
    // `end` not first on its line → RuboCop skips `on_block` entirely.
    test::<IndentationWidth>().expect_no_offenses("foo do\n  bar end\n");
}

#[test]
fn flags_block_body_measured_from_misaligned_end() {
    // `end` at column 4, body at column 2 → `Use 2 (not -2)`: the body must
    // sit 2 past `end`, so this under-indented body still flags (true pin).
    test::<IndentationWidth>().expect_offense(indoc! {r#"
        foo do
          bar
          ^^ Use 2 (not -2) spaces for indentation.
            end
    "#});
}
