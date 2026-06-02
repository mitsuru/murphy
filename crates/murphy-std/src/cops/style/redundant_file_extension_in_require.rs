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

    // Offense range: the whole argument node.
    cx.emit_offense(cx.range(arg), MSG, None);

    // Autocorrect: delete the `.rb` bytes (3 bytes) before the closing quote.
    // The string source is like `'foo.rb'` — the last byte is the closing quote,
    // so the `.rb` occupies bytes [end-4, end-1).
    // This mirrors RuboCop's `extension_range`:
    //   range_between(end_of_path_string - 4, end_of_path_string - 1)
    // where end_of_path_string is the source_range.end_pos (after closing quote).
    let arg_range = cx.range(arg);
    let rb_range = Range {
        start: arg_range.end - 4,
        end: arg_range.end - 1,
    };
    cx.emit_edit(rb_range, "");
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
}

murphy_plugin_api::submit_cop!(RedundantFileExtensionInRequire);
