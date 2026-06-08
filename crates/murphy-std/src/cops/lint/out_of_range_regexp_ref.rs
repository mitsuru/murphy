//! `Lint/OutOfRangeRegexpRef` — flags back-references beyond the number of capture groups.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/OutOfRangeRegexpRef
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-zvzj
//! notes: >
//!   Ported from RuboCop Lint/OutOfRangeRegexpRef. A whole-file descendant scan
//!   walks the AST in pre-order, tracking the capture-group count from the last
//!   regexp literal used in a matching context and checking each NthRef ($1, $2,
//!   …) against it.
//!   Gaps vs RuboCop:
//!     - `when` clause regexps are not tracked.
//!     - `in` pattern-match regexps are not tracked.
//!     - Safe-navigation (`&.`) with regexp methods is not tracked.
//!     - `Regexp.new` / `Regexp.compile` calls are not tracked.
//!     - RuboCop's `match_with_lvasgn` hook and `after_send` hook provide
//!       precise ordering; Murphy approximates via pre-order descendant walk.
//! ```
//!
//! ## Matched shapes
//!
//! - `/(a)(b)/ =~ str; $3` — $3 is out of range for 2 capture groups
//! - `/(a)(b)/ === str; $3` — regexp as `===` receiver
//! - `str =~ /(a)(b)/; $3` — regexp as right-hand side of `=~`
//! - `str.match(/(a)(b)/); $3` — regexp as argument to `match`
//! - `str.grep(/(a)(b)/) { $3 }` — regexp as argument to `grep`
//! - `$1` with no preceding regexp match — always out of range
//!
//! ## No autocorrect
//!
//! This cop has no safe autocorrect. The user must change the back-reference
//! number or rework the regexp capture groups.
//!
//! ## Known v1 limitation: stateful tracking via whole-file scan
//!
//! RuboCop's implementation uses instance state (`@valid_ref`) updated across
//! multiple per-node dispatch hooks (`on_match_with_lvasgn`, `after_send`,
//! `on_when`, `on_in_pattern`, `on_nth_ref`). Murphy's cop model is stateless,
//! so this port uses a single `on_new_investigation` pass that walks all
//! descendants in pre-order, maintaining a local `valid_ref` variable. This
//! approximates the RuboCop behavior but may diverge in edge cases where
//! RuboCop's dispatcher order differs from pre-order traversal.

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Sentinel value for "unknown" capture group count.
/// Used when a regexp is present but cannot be analyzed (interpolated,
/// variable argument, etc.), meaning we cannot determine the capture count.
const UNKNOWN: u32 = u32::MAX;

/// Method names that indicate the regexp is the receiver of the call.
const REGEXP_RECEIVER_METHODS: &[&str] = &["=~", "===", "match"];

/// Method names that indicate a regexp literal can appear as the first argument.
const REGEXP_ARGUMENT_METHODS: &[&str] = &[
    "=~",
    "match",
    "grep",
    "gsub",
    "gsub!",
    "sub",
    "sub!",
    "[]",
    "slice",
    "slice!",
    "index",
    "rindex",
    "scan",
    "partition",
    "rpartition",
    "start_with?",
    "end_with?",
];

/// Emit an offense for an out-of-range NthRef.
fn emit_nth_ref_offense(n: u32, valid_ref: u32, node: NodeId, cx: &Cx<'_>) {
    let count_display = if valid_ref == 0 {
        "no".to_string()
    } else {
        valid_ref.to_string()
    };
    let group_str = if valid_ref == 1 {
        "group"
    } else {
        "groups"
    };
    let message = format!(
        "${} is out of range ({} regexp capture {} detected).",
        n, count_display, group_str
    );
    cx.emit_offense(cx.range(node), &message, None);
}

#[derive(Default)]
pub struct OutOfRangeRegexpRef;

#[cop(
    name = "Lint/OutOfRangeRegexpRef",
    description = "Flags out-of-range regexp capture group references.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl OutOfRangeRegexpRef {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();
        let descendants = cx.descendants(root);
        let mut valid_ref = 0u32;
        let mut handled_nth_refs = HashSet::new();

        for &id in &descendants {
            match cx.kind(id) {
                NodeKind::MatchWithLvasgn { call, .. } => {
                    if let Some(count) = regexp_count_in_match_with_lvasgn(*call, cx) {
                        valid_ref = count;
                    }
                }
                NodeKind::Send { .. } => {
                    // If the receiver is an NthRef, check it BEFORE updating
                    // valid_ref from the Send's arguments. This matches
                    // RuboCop's `after_send` ordering (children then parent).
                    if let NodeKind::Send { receiver, .. } = *cx.kind(id) {
                        if let Some(recv_id) = receiver.get() {
                            if let NodeKind::NthRef(n) = cx.kind(recv_id) {
                                if *n > valid_ref {
                                    emit_nth_ref_offense(*n, valid_ref, recv_id, cx);
                                }
                                handled_nth_refs.insert(recv_id);
                            }
                        }
                    }

                    if let Some(count) = regexp_count_in_send(id, cx) {
                        valid_ref = count;
                    }
                }
                NodeKind::NthRef(n) => {
                    if handled_nth_refs.contains(&id) {
                        continue;
                    }
                    if valid_ref == UNKNOWN {
                        continue;
                    }
                    if *n > valid_ref {
                        emit_nth_ref_offense(*n, valid_ref, id, cx);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Count capture groups in a regexp that is the receiver or argument of a
/// `MatchWithLvasgn` call (the `call` field is the `=~` send).
fn regexp_count_in_match_with_lvasgn(call: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let NodeKind::Send { receiver, method: _, args } = *cx.kind(call) else {
        return None;
    };

    // Check receiver (e.g. `/(re)/ =~ str`)
    if let Some(recv_id) = receiver.get() {
        match try_count_regexp_captures(recv_id, cx) {
            CountResult::Count(n) => return Some(n),
            CountResult::Unknown => return Some(UNKNOWN),
            CountResult::NotRegexp => {}
        }
    }

    // Check first argument (e.g. `str =~ /(re)/`)
    let args_list = cx.list(args);
    if let Some(&first_arg) = args_list.first() {
        match try_count_regexp_captures(first_arg, cx) {
            CountResult::Count(n) => return Some(n),
            CountResult::Unknown => return Some(UNKNOWN),
            CountResult::NotRegexp => return None,
        }
    }

    None
}

/// Check if a `Send` node has a regexp in a matching position (receiver or
/// first argument) with an appropriate method, and return the capture count.
fn regexp_count_in_send(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };
    let method_str = cx.symbol_str(method);
    let is_receiver_method = REGEXP_RECEIVER_METHODS.contains(&method_str);
    let is_arg_method = REGEXP_ARGUMENT_METHODS.contains(&method_str);

    if !is_receiver_method && !is_arg_method {
        return None;
    }

    // Receiver regexp: e.g. `/(re)/.match(str)`.
    if is_receiver_method {
        if let Some(recv_id) = receiver.get() {
            match try_count_regexp_captures(recv_id, cx) {
                CountResult::Count(n) => return Some(n),
                CountResult::Unknown => return Some(UNKNOWN),
                CountResult::NotRegexp => {
                    // If the method only expects a regexp as receiver
                    // (e.g. `===`) and the receiver is not a literal,
                    // we cannot determine the count.
                    if !is_arg_method {
                        return Some(UNKNOWN);
                    }
                    // Method also accepts regexp as argument (e.g. `=~`, `match`);
                    // fall through to check arguments.
                }
            }
        }
    }

    // First-argument regexp: e.g. `str.match(/(re)/)`.
    if is_arg_method {
        let args_list = cx.list(args);
        if let Some(&first_arg) = args_list.first() {
            return match try_count_regexp_captures(first_arg, cx) {
                CountResult::Count(n) => Some(n),
                CountResult::Unknown => Some(UNKNOWN),
                // Method expects a regexp as argument but the argument
                // is not a literal (e.g., a variable) — unknown count.
                CountResult::NotRegexp => Some(UNKNOWN),
            };
        }
    }

    None
}

/// Result of attempting to count capture groups.
enum CountResult {
    /// A regexp literal with a known count of capture groups.
    Count(u32),
    /// A regexp that is present but whose groups cannot be determined
    /// (e.g. interpolated regexp, variable used as regexp argument).
    Unknown,
    /// The node is not a regexp at all.
    NotRegexp,
}

/// Attempt to count capture groups in a regexp node.
fn try_count_regexp_captures(node_id: NodeId, cx: &Cx<'_>) -> CountResult {
    let NodeKind::Regexp { parts, .. } = *cx.kind(node_id) else {
        return CountResult::NotRegexp;
    };

    let parts_list = cx.list(parts);
    // RuboCop skips interpolated regexps — we can't determine the capture count.
    if parts_list.len() != 1 {
        return CountResult::Unknown;
    }
    if !matches!(cx.kind(parts_list[0]), NodeKind::Str(_)) {
        return CountResult::Unknown;
    }

    let part_range = cx.range(parts_list[0]);
    let source = cx.raw_source(part_range);
    CountResult::Count(count_regexp_captures(source))
}

/// Count capture groups in the raw body text of a regexp.
///
/// Handles:
/// - `(...)` — numbered capture groups
/// - `(?<name>...)` — named capture groups
/// - `(?'name'...)` — named capture groups (alternate syntax)
/// - `(?:...)` — non-capturing groups (skipped)
/// - `(?=...)` / `(?!...)` — lookaheads (skipped)
/// - `(?<=...)` / `(?<!...)` — lookbehinds (skipped)
/// - `(?>...)` — atomic groups (skipped)
/// - `(?~...)` — absence groups (skipped)
/// - `(?flags:...)` / `(?flags-flags:...)` — groups with flags (skipped)
/// - `[` … `]` — character classes (skipped)
/// - `\(` — escaped parens
fn count_regexp_captures(source: &str) -> u32 {
    let bytes = source.as_bytes();
    let mut count = 0u32;
    let mut i = 0usize;
    let len = bytes.len();

    // Track character-class depth for `[...]`.
    let mut cc_depth = 0u32;

    while i < len {
        match bytes[i] {
            b'\\' => {
                // Skip the escaped character.
                i += 2;
            }
            b'[' => {
                cc_depth += 1;
                i += 1;
            }
            b']' => {
                if cc_depth > 0 {
                    cc_depth -= 1;
                }
                i += 1;
            }
            b'(' => {
                if cc_depth > 0 {
                    i += 1;
                    continue;
                }
                i += 1;
                if i < len && bytes[i] == b'?' {
                    i += 1;
                    if i >= len {
                        continue;
                    }
                    match bytes[i] {
                        // Non-capturing groups and special groups:
                        // (?:...) (?=...) (?!...) (?<=...) (?<!...)
                        // (?>...) (?~...) (?flags:...)
                        b':' | b'=' | b'!' | b'>' | b'~' | b'-' | b'i' | b'm'
                        | b'x' | b'd' | b'a' | b'u' => {
                            // Skip this group — not a capturing group.
                            i += 1;
                            continue;
                        }
                        // Named capture: (?<name>...) or (?'name'...)
                        b'<' | b'\'' => {
                            count += 1;
                            i += 1;
                            continue;
                        }
                        // Other (?...) — treat as non-capturing.
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                }
                // Plain capturing group: (...)
                count += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    count
}

murphy_plugin_api::submit_cop!(OutOfRangeRegexpRef);

#[cfg(test)]
mod tests {
    use super::OutOfRangeRegexpRef;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_out_of_range_ref_with_no_regexp() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            puts $3
                 ^^ $3 is out of range (no regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_out_of_range_for_numbered_captures() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(foo)(bar)/ =~ "foobar"
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_out_of_range_for_named_captures() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(?<foo>FOO)(?<bar>BAR)/ =~ "FOOBAR"
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_out_of_range_for_mixed_named_and_numbered() {
        // (?<foo>FOO) is a named group (group 1), (BAR) is group 2.
        // Both named and numbered groups contribute to the count, so $3 is out of range.
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(?<foo>FOO)(BAR)/ =~ "FOOBAR"
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_out_of_range_for_non_captures() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /bar/ =~ "foobar"
            puts $1
                 ^^ $1 is out of range (no regexp capture groups detected).
        "#});
    }

    #[test]
    fn does_not_flag_valid_ref_for_numbered_captures() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            /(foo)(bar)/ =~ "foobar"
            puts $1
            puts $2
        "#});
    }

    #[test]
    fn does_not_flag_valid_ref_for_named_captures() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            /(?<foo>FOO)(?<bar>BAR)/ =~ "FOOBAR"
            puts $1
            puts $2
        "#});
    }

    #[test]
    fn does_not_flag_valid_ref_for_mixed_captures() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            /(?<foo>FOO)(BAR)/ =~ "FOOBAR"
            puts $1
        "#});
    }

    #[test]
    fn does_not_flag_regexp_with_encoding_option() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            /(foo)(bar)/u =~ "foobar"
            puts $1
            puts $2
        "#});
    }

    #[test]
    fn does_not_flag_interpolated_regexp() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            var = '(\d+)'
            /(?<foo>#{var}*)/ =~ "12"
            puts $1
            puts $2
        "#});
    }

    #[test]
    fn flags_when_regexp_is_on_rhs_of_equal_tilde() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar" =~ /(foo)(bar)/
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_when_regexp_is_matched_with_triple_equals() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(foo)(bar)/ === "foobar"
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_when_regexp_is_matched_with_match_method() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(foo)(bar)/.match("foobar")
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn ignores_calls_to_match_query() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            /(foo)(bar)/.match("foobar")
            /(foo)(bar)(baz)/.match?("foobarbaz")
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn ignores_match_with_no_arguments() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            foo.match
        "#});
    }

    #[test]
    fn ignores_match_with_no_receiver() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            match(bar)
        "#});
    }

    #[test]
    fn only_flags_literal_regexp_not_variable() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            foo_bar_regexp = /(foo)(bar)/
            foo_regexp = /(foo)/

            foo_bar_regexp =~ "foobar"
            puts $2
        "#});
    }

    #[test]
    fn checks_grep_with_block() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            %w[foo foobar].grep(/(foo)/) { $2 }
                                           ^^ $2 is out of range (1 regexp capture group detected).
        "#});
    }

    #[test]
    fn does_not_flag_grep_with_variable_argument() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            %w[foo foobar].grep(some_regexp) { $2 }
        "#});
    }

    #[test]
    fn flags_out_of_range_with_bracket_access() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar"[/(foo)(bar)/]
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn does_not_flag_bracket_access_with_variable_argument() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            "foobar"[some_regexp]
            puts $3
        "#});
    }

    #[test]
    fn flags_call_on_nth_ref_itself() {
        // When the NthRef is used as a receiver of a method call like gsub,
        // it should still be flagged if out of range.
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            if "some : line " =~ / : (.+)/
              $2.gsub(/\s{2}/, " ")
              ^^ $2 is out of range (1 regexp capture group detected).
            end
        "#});
    }

    // --- gsub / gsub! / sub / sub! / scan with block ---

    #[test]
    fn flags_gsub_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".gsub(/(foo)(bar)/) { $3 }
                                          ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn does_not_flag_gsub_with_valid_ref() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            "foobar".gsub(/(foo)(bar)/) { $2 }
        "#});
    }

    #[test]
    fn does_not_flag_gsub_with_variable_arg() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            some_string.gsub(some_regexp) { $3 }
        "#});
    }

    #[test]
    fn flags_sub_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".sub(/(foo)(bar)/) { $3 }
                                         ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    // --- scan with regexp argument ---

    #[test]
    fn flags_scan_with_out_of_range_ref() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".scan(/(foo)(bar)/) { $3 }
                                          ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn does_not_flag_scan_with_valid_ref() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            "foobar".scan(/(foo)(bar)/) { $2 }
        "#});
    }

    // --- match argument (regexp as first arg to String#match) ---

    #[test]
    fn flags_string_match_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".match(/(foo)(bar)/)
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn does_not_flag_string_match_with_valid_ref() {
        test::<OutOfRangeRegexpRef>().expect_no_offenses(indoc! {r#"
            "foobar".match(/(foo)(bar)/)
            puts $2
        "#});
    }

    // --- index / rindex / partition / rpartition / start_with? / end_with? ---

    #[test]
    fn flags_index_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".index(/(foo)(bar)/)
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_partition_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".partition(/(foo)(bar)/)
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }

    #[test]
    fn flags_start_with_with_regexp_arg() {
        test::<OutOfRangeRegexpRef>().expect_offense(indoc! {r#"
            "foobar".start_with?(/(foo)(bar)/)
            puts $3
                 ^^ $3 is out of range (2 regexp capture groups detected).
        "#});
    }
}
