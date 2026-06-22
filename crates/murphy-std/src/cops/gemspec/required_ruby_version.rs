//! `Gemspec/RequiredRubyVersion` — `required_ruby_version` in a gemspec must be
//! specified (non-dynamically) and its major.minor must equal the resolved
//! `AllCops.TargetRubyVersion`. The cop runs only on `*.gemspec` files; the host
//! applies the per-cop `Include` from `config/default.yml`, so this cop never
//! inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/RequiredRubyVersion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 (`gemspec/required_ruby_version.rb`). Two checks,
//!   two messages, no autocorrect:
//!
//!   (1) MISSING — `on_new_investigation`: if the whole AST contains no
//!   `(send _ :required_ruby_version= _)` (RuboCop's `def_node_search`), emit a
//!   global offense (`add_global_offense(MISSING_MSG)`) → Murphy renders it as a
//!   zero-width offense at byte 0 (`Lint/EmptyFile` precedent), matching
//!   RuboCop's `1:1` global location. The strict `(send ...)` pattern means a
//!   bare `required_ruby_version = 'x'` (an `lvasgn`, no receiver) does NOT count
//!   as specifying it → MISSING (verified against standalone rubocop 1.87.0).
//!
//!   (2) NOT_EQUAL — for each `required_ruby_version=` send, skip if the value is
//!   `dynamic_version?` (a send with no receiver, or a variable, or having any
//!   descendant send/variable — RuboCop's VARIABLES = ivar/gvar/cvar/lvar);
//!   otherwise extract the version and emit on the value node iff the extracted
//!   string != `target_ruby_version.to_s`. Extracted-`None` (empty string,
//!   unrecognized shape) `!= Some(target)` so it also fires — matching RuboCop's
//!   `ruby_version == target_ruby_version.to_s` guard.
//!
//!   Version extraction replicates RuboCop's quirky raw digit scan
//!   `str_content.scan(/\d/).first(2).join('.')`: collect individual digit
//!   *characters*, take the first two, join with `.`. So `'>= 2.5.0'`→`"2.5"`,
//!   `'>= 2'`→`"2"`, `'>= 2.10'`→`"2.1"` (intentionally NOT a semver parse;
//!   `2.10`→`2.1` verified against rubocop). Recognized value shapes (RuboCop's
//!   `defined_ruby_version` matcher): a single `str`, an array of *exactly two*
//!   `str` literals (`detect` picks the first containing `>` or `=`), or
//!   `Gem::Requirement.new(str+)` (first such str scanned). 1- or 3-element
//!   arrays are unrecognized → `None` → NOT_EQUAL (array-arity quirk verified).
//!
//!   Target string is `format!("{major}.{minor}")`; an unset target resolves to
//!   the documented Ruby 3.1 floor (`cx.rs` mandates the floor, not "newest").
//!   MISSING and NOT_EQUAL are mutually exclusive by construction (NOT_EQUAL only
//!   runs on a matched assignment; MISSING only when none matched), so the cop
//!   never double-emits.
//!
//!   One accepted, near-impossible edge divergence (not tracked as open work, in
//!   the same family as `Bundler/DuplicatedGem`'s documented edges): an explicit
//!   top-level `::Gem::Requirement.new('>= …')`. RuboCop's pattern is
//!   `(const (const nil? :Gem) :Requirement)` — `nil?` matches bare `Gem` but
//!   NOT a `cbase`-rooted `::Gem`, so RuboCop fails the match → extracts nothing
//!   → NOT_EQUAL. Murphy resolves the receiver via `cx.const_name`, which
//!   collapses a `cbase` root to the same `"Gem::Requirement"` string, so Murphy
//!   recognizes the shape and only flags on an actual version mismatch. Murphy
//!   thus under-flags relative to RuboCop solely when a developer writes the
//!   fully-qualified `::Gem::Requirement` with a version that happens to equal
//!   the target. Tightening this would require inline `NodeKind::Const` scope
//!   destructuring that `.claude/rules/cx-helpers.md` discourages; the divergence
//!   is accepted instead.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RubyVersion, cop};

#[derive(Default)]
pub struct RequiredRubyVersion;

const MISSING_MSG: &str = "`required_ruby_version` should be specified.";

#[cop(
    name = "Gemspec/RequiredRubyVersion",
    description = "Checks that `required_ruby_version` of gemspec is specified and equal to `TargetRubyVersion` of .rubocop.yml.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RequiredRubyVersion {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // Whole-AST source-order walk for `(send _ :required_ruby_version= _)`
        // assignments — RuboCop's `def_node_search :required_ruby_version?`.
        let mut found_any = false;
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(version_def) = required_ruby_version_value(node, cx) else {
                continue;
            };
            found_any = true;

            // RuboCop's `on_send`: skip dynamic values entirely.
            if is_dynamic_version(version_def, cx) {
                continue;
            }

            let extracted = extract_ruby_version(version_def, cx);
            let target = cx.target_ruby_version().unwrap_or(RubyVersion::new(3, 1));
            let target_str = format!("{}.{}", target.major, target.minor);
            if extracted.as_deref() == Some(target_str.as_str()) {
                continue;
            }

            let message = format!(
                "`required_ruby_version` and `TargetRubyVersion` ({target_str}, which may be specified in .rubocop.yml) should be equal."
            );
            cx.emit_offense(cx.range(version_def), &message, None);
        }

        // RuboCop's `on_new_investigation`: no assignment anywhere → global
        // offense (rendered at byte 0, the `Lint/EmptyFile` precedent).
        if !found_any {
            cx.emit_offense(Range { start: 0, end: 0 }, MISSING_MSG, None);
        }
    }
}

/// The first-argument value node of `node` if it is a
/// `(send _ :required_ruby_version= _)` assignment — RuboCop's
/// `def_node_search` pattern. Send-only, exactly one argument, any receiver
/// (including none). `None` otherwise.
fn required_ruby_version_value(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(node)? != "required_ruby_version=" {
        return None;
    }
    let args = cx.call_arguments(node);
    match args {
        [only] => Some(*only),
        _ => None,
    }
}

/// RuboCop's `dynamic_version?`:
///   `(node.send_type? && !node.receiver) || node.variable? ||
///    node.each_descendant(:send, *VARIABLES).any?`
/// where `VARIABLES = [:ivar, :gvar, :cvar, :lvar]`. `each_descendant` excludes
/// `node` itself, matching `cx.descendants`.
fn is_dynamic_version(node: NodeId, cx: &Cx<'_>) -> bool {
    // Clause 1: a send with no receiver.
    if matches!(cx.kind(node), NodeKind::Send { .. }) && cx.call_receiver(node).get().is_none() {
        return true;
    }
    // Clause 2: the node itself is a variable.
    if is_variable_node(node, cx) {
        return true;
    }
    // Clause 3: any descendant is a send or a variable.
    cx.descendants(node)
        .iter()
        .any(|&d| matches!(cx.kind(d), NodeKind::Send { .. }) || is_variable_node(d, cx))
}

/// RuboCop's `RuboCop::AST::Node::VARIABLES` plus the explicit `variable?`
/// check: instance / global / class / local variable reads.
fn is_variable_node(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Ivar(_) | NodeKind::Gvar(_) | NodeKind::Cvar(_) | NodeKind::Lvar(_)
    )
}

/// RuboCop's `extract_ruby_version(defined_ruby_version(version_def))`.
///
/// `defined_ruby_version` accepts only: a single `str`, an array of *exactly
/// two* `str` literals, or `Gem::Requirement.new(str+)`. From an array (literal
/// or `Gem::Requirement` args) it `detect`s the first str whose content has a
/// `>` or `=`. Then it scans the chosen str's content for individual digit
/// characters and joins the first two with `.`. Returns `None` for unrecognized
/// shapes or a value yielding no digits.
fn extract_ruby_version(version_def: NodeId, cx: &Cx<'_>) -> Option<String> {
    let chosen = match *cx.kind(version_def) {
        NodeKind::Str(_) => version_def,
        NodeKind::Array(list) => {
            // `defined_ruby_version` matches only a 2-element array of strings.
            let elems = cx.list(list);
            if elems.len() != 2 || !elems.iter().all(|&e| matches!(cx.kind(e), NodeKind::Str(_))) {
                return None;
            }
            detect_constraint_str(elems, cx)?
        }
        NodeKind::Send { .. } => {
            // `Gem::Requirement.new(str+)`: method `:new`, receiver
            // `Gem::Requirement`, one or more str arguments.
            if cx.method_name(version_def)? != "new" {
                return None;
            }
            if !is_gem_requirement_receiver(version_def, cx) {
                return None;
            }
            let args = cx.call_arguments(version_def);
            if args.is_empty() || !args.iter().all(|&a| matches!(cx.kind(a), NodeKind::Str(_))) {
                return None;
            }
            detect_constraint_str(args, cx)?
        }
        _ => return None,
    };

    let NodeKind::Str(id) = *cx.kind(chosen) else {
        return None;
    };
    digit_scan_version(cx.string_str(id))
}

/// RuboCop's `detect { |v| /[>=]/.match?(v.str_content) }` over the string
/// elements: the first whose content contains `>` or `=`. `None` if none match.
fn detect_constraint_str(elems: &[NodeId], cx: &Cx<'_>) -> Option<NodeId> {
    elems.iter().copied().find(|&e| {
        let NodeKind::Str(id) = *cx.kind(e) else {
            return false;
        };
        cx.string_str(id).chars().any(|c| c == '>' || c == '=')
    })
}

/// True iff `node`'s receiver is the `Gem::Requirement` const — RuboCop's
/// `(const (const nil? :Gem) :Requirement)`. `is_global_const` only matches
/// single-segment names, so resolve the full path via `const_name` (which
/// treats a `cbase` root the same as RuboCop's `nil?`).
fn is_gem_requirement_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.const_name(recv).as_deref() == Some("Gem::Requirement")
}

/// RuboCop's `str_content.scan(/\d/).first(2).join('.')`: collect the first two
/// individual digit *characters* and join with `.`. `None` if the content has no
/// digits (so an empty/blank value yields `None`, which is `!= Some(target)`).
fn digit_scan_version(content: &str) -> Option<String> {
    let mut digits = content.chars().filter(|c| c.is_ascii_digit());
    let first = digits.next()?;
    match digits.next() {
        Some(second) => Some(format!("{first}.{second}")),
        None => Some(first.to_string()),
    }
}

murphy_plugin_api::submit_cop!(RequiredRubyVersion);

#[cfg(test)]
mod tests {
    use super::RequiredRubyVersion;
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    #[test]
    fn flags_missing_required_ruby_version() {
        // Global offense (byte 0) — can't caret-annotate; assert via run_cop.
        // The MISSING message is target-independent, so the default context
        // (Ruby 3.1 floor) suffices.
        let offenses = run_cop::<RequiredRubyVersion>(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.summary = 'x'
            end
        "#});
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "`required_ruby_version` should be specified.");
        assert_eq!(offenses[0].range.start, 0);
        assert_eq!(offenses[0].range.end, 0);
    }

    #[test]
    fn flags_bare_lvasgn_as_missing() {
        // `required_ruby_version = 'x'` is an lvasgn (no receiver), NOT a send,
        // so RuboCop's strict pattern does not match → MISSING.
        let offenses = run_cop::<RequiredRubyVersion>(indoc! {r#"
            Gem::Specification.new do |spec|
              required_ruby_version = '>= 3.1'
            end
        "#});
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "`required_ruby_version` should be specified.");
    }

    #[test]
    fn flags_version_mismatch_single_string() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = '>= 2.5.0'
                                               ^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (3.1, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn accepts_matching_version_with_teeny() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = '>= 2.5.0'
                end
            "#});
    }

    #[test]
    fn accepts_matching_version_without_teeny() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = '>= 2.5'
                end
            "#});
    }

    #[test]
    fn flags_empty_string_as_mismatch() {
        // '' yields no digits → extracted None → != target → NOT_EQUAL.
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ''
                                               ^^ `required_ruby_version` and `TargetRubyVersion` (3.1, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn digit_scan_quirk_2_10_extracts_2_1() {
        // '>= 2.10' digit-scans to "2.1" (NOT 2.10), so target 2.1 is clean.
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 1)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = '>= 2.10'
                end
            "#});
    }

    #[test]
    fn accepts_two_element_array_when_matching() {
        // Array of two strings: detect first with `>`/`=` → '>= 2.5.0' → "2.5".
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ['>= 2.5.0', '< 2.7.0']
                end
            "#});
    }

    #[test]
    fn flags_two_element_array_mismatch() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ['>= 2.5.0', '< 2.7.0']
                                               ^^^^^^^^^^^^^^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (3.1, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn one_element_array_unrecognized_flags_as_mismatch() {
        // Array arity quirk: 1-element array is NOT matched → None → NOT_EQUAL.
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ['>= 2.5.0']
                                               ^^^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (2.5, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn three_element_array_unrecognized_flags_as_mismatch() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ['>= 2.5.0', '< 2.7.0', '!= 2.6.0']
                                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (2.5, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn gem_requirement_new_is_statically_evaluated() {
        // `Gem::Requirement.new('>= 2.5.0')` is a send WITH a receiver and no
        // descendant send/variable → NOT dynamic → mismatch fires.
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 1)
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = Gem::Requirement.new('>= 2.5.0')
                                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (2.1, which may be specified in .rubocop.yml) should be equal.
                end
            "#});
    }

    #[test]
    fn gem_requirement_new_matching_is_clean() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(2, 5)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = Gem::Requirement.new('>= 2.5.0')
                end
            "#});
    }

    #[test]
    fn dynamic_variable_value_is_skipped() {
        // Bare `some_var` parses as send-no-receiver → dynamic → no offense.
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = some_var
                end
            "#});
    }

    #[test]
    fn dynamic_array_with_send_is_skipped() {
        // Array with a descendant send → dynamic → no offense (skipped before
        // the unrecognized-shape extraction would have flagged it).
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = ['>= 2.5.0', foo]
                end
            "#});
    }

    #[test]
    fn dynamic_ivar_value_is_skipped() {
        test::<RequiredRubyVersion>()
            .with_target_ruby_version(3, 1)
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.required_ruby_version = @version
                end
            "#});
    }

    #[test]
    fn target_defaults_to_3_1_floor_when_unset() {
        // No target configured → resolves to Ruby 3.1; value '>= 2.5.0' → "2.5"
        // != "3.1" → mismatch with the 3.1 floor in the message.
        test::<RequiredRubyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.required_ruby_version = '>= 2.5.0'
                                           ^^^^^^^^^^ `required_ruby_version` and `TargetRubyVersion` (3.1, which may be specified in .rubocop.yml) should be equal.
            end
        "#});
    }
}
