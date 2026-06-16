//! `Style/RedundantFileExtensionInRequire` — flags the `.rb` extension in
//! `require`/`require_relative` arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantFileExtensionInRequire
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports the full RuboCop behavior:
//!     - Flags require/require_relative with nil receiver whose string argument
//!       ends with `.rb`.
//!     - Other extensions (e.g. `.so`) are not flagged.
//!     - Autocorrect removes only the `.rb` bytes (last 3 bytes before the
//!       closing quote), matching RuboCop's `extension_range`.
//!     - `dstr` arguments (interpolated strings) are skipped: the value cannot
//!       be statically known, and the node does not carry a simple string value.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! require 'foo.rb'
//! require_relative '../foo.rb'
//!
//! # good
//! require 'foo'
//! require_relative '../foo'
//! require 'foo.so'
//! ```
//!
//! ## Autocorrect
//!
//! Delete the `.rb` bytes (3 bytes) immediately before the closing quote of
//! the string literal.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantFileExtensionInRequire;

const MSG: &str = "Redundant `.rb` file extension detected.";

#[cop(
    name = "Style/RedundantFileExtensionInRequire",
    description = "Checks for the presence of superfluous `.rb` extension in the filename \
                   provided to `require` and `require_relative`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantFileExtensionInRequire {
    #[on_node(kind = "send", methods = ["require", "require_relative"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Require nil receiver (top-level call only).
    let receiver = cx.call_receiver(node);
    if receiver.get().is_some() {
        return;
    }

    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    let arg = args[0];

    // Only plain string literals (not interpolated/dynamic).
    let NodeKind::Str(str_id) = *cx.kind(arg) else {
        return;
    };

    // Check that the string value ends with ".rb".
    if !cx.string_str(str_id).ends_with(".rb") {
        return;
    }

    // Guard: also verify that the source text of the argument ends with `.rb`
    // (plus a closing quote). This prevents corrupting autocorrect for escape
    // sequences like `"\x72b"` where the *value* ends with `.rb` but the
    // *source* does not (the 3 bytes before the closing quote are `x72`, not
    // `.rb`). Only emit the edit when the source form is safe.
    let arg_range = cx.range(arg);
    let src = cx.raw_source(arg_range);
    // Source is at least 6 bytes: quote + 3 + .rb + quote — but we just need
    // the last 4 bytes to be `.rb` + closing quote.
    let src_bytes = src.as_bytes();
    let safe_to_correct = src_bytes.len() >= 5
        && src_bytes[src_bytes.len() - 4] == b'.'
        && src_bytes[src_bytes.len() - 3] == b'r'
        && src_bytes[src_bytes.len() - 2] == b'b'
        // Last byte is closing quote (single or double).
        && (src_bytes[src_bytes.len() - 1] == b'\'' || src_bytes[src_bytes.len() - 1] == b'"');

    // Offense range: the whole argument node.
    cx.emit_offense(arg_range, MSG, None);

    if safe_to_correct {
        // Autocorrect: delete the `.rb` bytes (3 bytes) before the closing quote.
        // The string source is like `'foo.rb'` — the last byte is the closing quote,
        // so the `.rb` occupies bytes [end-4, end-1).
        // This mirrors RuboCop's `extension_range`:
        //   range_between(end_of_path_string - 4, end_of_path_string - 1)
        // where end_of_path_string is the source_range.end_pos (after closing quote).
        let rb_range = Range {
            start: arg_range.end - 4,
            end: arg_range.end - 1,
        };
        cx.emit_edit(rb_range, "");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantFileExtensionInRequire;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases -----

    #[test]
    fn flags_require_rb_extension() {
        test::<RedundantFileExtensionInRequire>().expect_offense(indoc! {r#"
            require 'foo.rb'
                    ^^^^^^^^ Redundant `.rb` file extension detected.
        "#});
    }

    #[test]
    fn flags_require_relative_rb_extension() {
        test::<RedundantFileExtensionInRequire>().expect_offense(indoc! {r#"
            require_relative '../foo.rb'
                             ^^^^^^^^^^^ Redundant `.rb` file extension detected.
        "#});
    }

    #[test]
    fn flags_require_with_path() {
        test::<RedundantFileExtensionInRequire>().expect_offense(indoc! {r#"
            require 'path/to/file.rb'
                    ^^^^^^^^^^^^^^^^^ Redundant `.rb` file extension detected.
        "#});
    }

    // ----- Autocorrect cases -----

    #[test]
    fn corrects_require_rb_extension() {
        test::<RedundantFileExtensionInRequire>().expect_correction(
            indoc! {r#"
                require 'foo.rb'
                        ^^^^^^^^ Redundant `.rb` file extension detected.
            "#},
            "require 'foo'\n",
        );
    }

    #[test]
    fn corrects_require_relative_rb_extension() {
        test::<RedundantFileExtensionInRequire>().expect_correction(
            indoc! {r#"
                require_relative '../foo.rb'
                                 ^^^^^^^^^^^ Redundant `.rb` file extension detected.
            "#},
            "require_relative '../foo'\n",
        );
    }

    #[test]
    fn corrects_require_with_path() {
        test::<RedundantFileExtensionInRequire>().expect_correction(
            indoc! {r#"
                require 'path/to/file.rb'
                        ^^^^^^^^^^^^^^^^^ Redundant `.rb` file extension detected.
            "#},
            "require 'path/to/file'\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_require_without_extension() {
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("require 'foo'\n");
    }

    #[test]
    fn accepts_require_with_so_extension() {
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("require 'foo.so'\n");
    }

    #[test]
    fn accepts_require_relative_without_extension() {
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("require_relative '../foo'\n");
    }

    #[test]
    fn accepts_require_with_receiver() {
        // require with a receiver is not a top-level call — skip.
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("obj.require 'foo.rb'\n");
    }

    #[test]
    fn accepts_require_with_multiple_args() {
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("require 'foo.rb', 'bar.rb'\n");
    }

    #[test]
    fn accepts_require_with_no_args() {
        test::<RedundantFileExtensionInRequire>()
            .expect_no_offenses("require\n");
    }

    // ----- Escape sequence: offense emitted but autocorrect skipped -----

    #[test]
    fn flags_but_does_not_correct_escape_sequence_rb() {
        // The string value ends with ".rb" (\x72 = 'r', b = 'b') but the source
        // does not have literal `.rb` bytes before the closing quote.
        // The offense is still flagged, but no autocorrect edit is emitted.
        // (The test harness only checks the offense annotation here; if no
        //  emit_edit is produced the expect_offense still passes.)
        test::<RedundantFileExtensionInRequire>().expect_offense(indoc! {r#"
            require "foo.\x72b"
                    ^^^^^^^^^^^ Redundant `.rb` file extension detected.
        "#});
    }
}

murphy_plugin_api::submit_cop!(RedundantFileExtensionInRequire);
