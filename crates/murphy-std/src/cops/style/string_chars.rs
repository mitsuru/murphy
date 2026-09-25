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
//!   Verbatim port of the `split` call head `(call _ :split ...)` (murphy-s1yc.4):
//!   `call` = `{send csend}` covers safe-navigation (`string&.split(//)`),
//!   mirroring RuboCop's `alias on_csend on_send`; the `_` receiver binds an
//!   absent (receiverless `split(//)`) or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list, with the one-`BAD_ARGUMENTS`
//!   guard applied separately.
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

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop, def_node_matcher};

// Verbatim port of the `split` call head (murphy-s1yc.4):
// `(call _ :split ...)` — `call` = `{send csend}` covers safe-navigation
// (`string&.split(//)`); the `_` receiver binds an absent (receiverless
// `split(//)`) or present receiver; trailing `...` absorbs any argument list,
// so the one-`BAD_ARGUMENTS`-arg guard is applied separately in `check`.
def_node_matcher!(split_call, "(call _ :split ...)");

/// Stateless unit struct.
#[derive(Default)]
pub struct StringChars;

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
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ :split ...)` head: filters to `split` calls on either
    // `send` or `csend` (safe-navigation), with any receiver (absent or
    // present). Trailing `...` matches any arg list, so the one-arg
    // `BAD_ARGUMENTS` guard below applies separately.
    if !split_call(node, cx) {
        return;
    }
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
    let message = format!("Use `chars` instead of `{offense_src}`.");
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

    // --- Characterization (murphy-s1yc.4): pin the exact node set the
    // hand-rolled send/csend dispatch matches, so the verbatim
    // `(call _ :split ...)` port can be proven byte-identical (plus the
    // one-bad-arg guard). `call` = `{send csend}` covers safe-navigation;
    // `_` binds the absent (receiverless) slot per if9y.

    #[test]
    fn s1yc4_flags_bare_split_empty_regexp() {
        // Bare `split(//)` matches: `_` binds the nil-filled receiver.
        test::<StringChars>().expect_correction(
            indoc! {r#"
                split(//)
                ^^^^^^^^^ Use `chars` instead of `split(//)`.
            "#},
            "chars\n",
        );
    }

    #[test]
    fn s1yc4_flags_bare_split_empty_string() {
        // Bare `split("")` matches as well.
        test::<StringChars>().expect_correction(
            indoc! {r#"
                split("")
                ^^^^^^^^^ Use `chars` instead of `split("")`.
            "#},
            "chars\n",
        );
    }

    #[test]
    fn s1yc4_accepts_bare_split_no_args() {
        // Bare `split` with no args: matcher matches, one-arg guard rejects.
        test::<StringChars>().expect_no_offenses("split\n");
    }

    #[test]
    fn s1yc4_accepts_csend_split_no_args() {
        // `string&.split` with no args: matcher matches, guard rejects.
        test::<StringChars>().expect_no_offenses("string&.split\n");
    }
}
murphy_plugin_api::submit_cop!(StringChars);
