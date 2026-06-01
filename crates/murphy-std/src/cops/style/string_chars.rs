//! `Style/StringChars` — prefer `chars` over `split` with empty-match argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringChars
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers `split(//)`, `split('')`, and `split("")` for both `send` and
//!   `csend` forms, matching RuboCop's `BAD_ARGUMENTS` source-text check.
//!   The cop is marked `Safe: false` in the default config (unsafe autocorrect)
//!   because it cannot be guaranteed the receiver is actually a String.
//!   Murphy emits autocorrect unconditionally (no unsafe-autocorrect flag in
//!   the ABI at time of authoring).
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes with method `split` and exactly one argument whose
//! raw source is `//`, `''`, or `""`.
//!
//! - `string.split(//)` → offense, correct to `string.chars`
//! - `string.split('')` → offense, correct to `string.chars`
//! - `string.split("")` → offense, correct to `string.chars`
//! - `string&.split(//)` → offense (csend form)
//!
//! ## Autocorrect
//!
//! Replaces the range from the `split` selector to the end of the node with
//! `chars`. This is a whole selector+args replacement: `split(//)` → `chars`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct StringChars;

const MSG: &str = "Use `chars` instead of `%s`.";

/// Raw source text of the three bad split arguments.
const BAD_ARGS: &[&str] = &["//", "''", "\"\""];

#[cop(
    name = "Style/StringChars",
    description = "Checks for uses of `String#split` with empty string or regexp literal argument.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl StringChars {
    #[on_node(kind = "send", methods = ["split"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("split") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    let arg = args[0];
    let arg_src = cx.raw_source(cx.range(arg));
    if !BAD_ARGS.contains(&arg_src) {
        return;
    }

    // Offense range: from start of `split` selector to end of the whole call.
    // Matches RuboCop's range_between(node.loc.selector.begin_pos, node.source_range.end_pos).
    let selector_start = cx.node(node).loc.name.start;
    let node_end = cx.range(node).end;
    let offense_range = Range {
        start: selector_start,
        end: node_end,
    };
    let offense_src = cx.raw_source(offense_range);
    let message = MSG.replace("%s", offense_src);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: replace `split(//)` (or `split('')` / `split("")`) with `chars`.
    cx.emit_edit(offense_range, "chars");
}

#[cfg(test)]
mod tests {
    use super::StringChars;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_split_empty_regexp() {
        test::<StringChars>().expect_correction(
            indoc! {r#"
                string.split(//)
                       ^^^^^^^^^ Use `chars` instead of `split(//)`.
            "#},
            "string.chars\n",
        );
    }

    #[test]
    fn flags_split_single_quoted_empty() {
        test::<StringChars>().expect_correction(
            indoc! {"
                string.split('')
                       ^^^^^^^^^ Use `chars` instead of `split('')`.
            "},
            "string.chars\n",
        );
    }

    #[test]
    fn flags_split_double_quoted_empty() {
        test::<StringChars>().expect_correction(
            indoc! {r#"
                string.split("")
                       ^^^^^^^^^ Use `chars` instead of `split("")`.
            "#},
            "string.chars\n",
        );
    }

    #[test]
    fn flags_csend_split_empty_regexp() {
        test::<StringChars>().expect_correction(
            indoc! {r#"
                string&.split(//)
                        ^^^^^^^^^ Use `chars` instead of `split(//)`.
            "#},
            "string&.chars\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_chars() {
        test::<StringChars>().expect_no_offenses("string.chars\n");
    }

    #[test]
    fn accepts_split_with_non_empty_string() {
        test::<StringChars>().expect_no_offenses("string.split(' ')\n");
    }

    #[test]
    fn accepts_split_with_non_empty_regexp() {
        test::<StringChars>().expect_no_offenses("string.split(/[a-z]/)\n");
    }

    #[test]
    fn accepts_split_with_no_args() {
        test::<StringChars>().expect_no_offenses("string.split\n");
    }

    #[test]
    fn accepts_split_with_multiple_args() {
        test::<StringChars>().expect_no_offenses("string.split(//, 3)\n");
    }
}
murphy_plugin_api::submit_cop!(StringChars);
