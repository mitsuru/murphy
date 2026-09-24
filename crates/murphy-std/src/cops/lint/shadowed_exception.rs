//! `Lint/ShadowedException` — avoid rescuing broad exceptions before narrow ones.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ShadowedException
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's runtime hierarchy with a static built-in table
//!   (core + common stdlib: IO/Timeout/Encoding/Math/Ractor/Regexp) so no
//!   host constant resolution is needed and the plugin ABI is untouched.
//!   Unknown constants, splats, and non-const expressions resolve to nil like
//!   RuboCop's `Kernel.const_get` NameError path. Same-group `Exception` with
//!   any sibling always flags; two direct `SystemCallError` children
//!   (`Errno::*`) never compare — identical codes are excluded by RuboCop's
//!   `Errno` check and siblings have nil `<=>` — so platform-specific Errno
//!   numbers cannot change the outcome. Cross-group order uses RuboCop's
//!   consecutive-pair `sorted?` with lexicographic array `<=>` (duplicates
//!   across groups are sorted, an intervening unknown breaks shadowing).
//! ```
//!
//! ## Matched shapes
//!
//! - `rescue Exception; ...; rescue StandardError` — broad rescue shadows later narrow rescue
//! - `rescue StandardError, RuntimeError` — broad and narrow exceptions in one group
//! - duplicate exception names in one group
//!
//! ## Autocorrect
//!
//! None.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Do not shadow rescued Exceptions.";

#[derive(Default)]
pub struct ShadowedException;

#[cop(
    name = "Lint/ShadowedException",
    description = "Avoid rescuing a higher level exception before a lower level exception.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ShadowedException {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.loc(node).end_keyword() == Range::ZERO {
            return;
        }
        let NodeKind::Rescue { resbodies, .. } = *cx.kind(node) else {
            return;
        };
        let rescues = cx.list(resbodies);
        let groups = rescues
            .iter()
            .map(|&resbody| exception_group(resbody, cx))
            .collect::<Vec<_>>();

        if groups.iter().any(|group| contains_multiple_levels(group)) {
            if let Some((resbody, _)) = rescues
                .iter()
                .zip(groups.iter())
                .find(|(_, group)| contains_multiple_levels(group))
            {
                cx.emit_offense(rescue_line_range(*resbody, cx), MSG, None);
            }
            return;
        }

        for (idx, pair) in groups.windows(2).enumerate() {
            if !pair_sorted(&pair[0], &pair[1]) {
                cx.emit_offense(rescue_line_range(rescues[idx], cx), MSG, None);
                return;
            }
        }
    }
}

fn rescue_line_range(resbody: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(resbody);
    let source = cx.source();
    let line_end = source[r.start as usize..]
        .find('\n')
        .map(|offset| r.start as usize + offset)
        .unwrap_or(r.end as usize);
    Range {
        start: r.start,
        end: line_end as u32,
    }
}

/// A resolved exception reference. `None` in a group means nil in RuboCop:
/// unknown constant, splat, or non-const expression (`Kernel.const_get`
/// raised `NameError`).
#[derive(Clone, PartialEq, Eq, Debug)]
enum Exc {
    /// Canonical known class, e.g. `StandardError`, `Timeout::Error`,
    /// `IO::EAGAINWaitReadable`.
    Known(&'static str),
    /// Any `Errno::*` direct `SystemCallError` child, e.g. `Errno::ENOENT`.
    /// The specific name is retained so `Errno::EAGAIN` is recognised as the
    /// parent of `IO::EAGAINWaitReadable`, while two distinct `Errno::*`
    /// stay incomparable.
    Errno(String),
}

fn exception_group(resbody: NodeId, cx: &Cx<'_>) -> Vec<Option<Exc>> {
    let NodeKind::Resbody { exceptions, .. } = *cx.kind(resbody) else {
        return Vec::new();
    };
    let exceptions = cx.list(exceptions);
    if exceptions.is_empty() {
        return vec![Some(Exc::Known("StandardError"))];
    }
    exceptions
        .iter()
        .map(|&node| match *cx.kind(node) {
            NodeKind::Splat(_) => None,
            _ => match const_name(node, cx) {
                Some(name) => resolve_exception(&name),
                None => None,
            },
        })
        .collect()
}

fn const_name(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return None;
    };
    let name = cx.symbol_str(name);
    match scope.get() {
        None => Some(name.to_string()),
        Some(scope) if matches!(cx.kind(scope), NodeKind::Cbase) => Some(name.to_string()),
        Some(scope) => const_name(scope, cx).map(|prefix| format!("{prefix}::{name}")),
    }
}

fn resolve_exception(name: &str) -> Option<Exc> {
    if let Some(short) = name.strip_prefix("Errno::") {
        if short.is_empty() || short.contains(':') {
            return None;
        }
        return Some(Exc::Errno(name.to_string()));
    }
    known_exception(name).map(Exc::Known)
}

fn known_exception(name: &str) -> Option<&'static str> {
    match name {
        "Exception" => Some("Exception"),
        "NoMemoryError" => Some("NoMemoryError"),
        "ScriptError" => Some("ScriptError"),
        "LoadError" => Some("LoadError"),
        "NotImplementedError" => Some("NotImplementedError"),
        "SyntaxError" => Some("SyntaxError"),
        "SecurityError" => Some("SecurityError"),
        "SignalException" => Some("SignalException"),
        "Interrupt" => Some("Interrupt"),
        "SystemExit" => Some("SystemExit"),
        "SystemStackError" => Some("SystemStackError"),
        "StandardError" => Some("StandardError"),
        "ArgumentError" => Some("ArgumentError"),
        "UncaughtThrowError" => Some("UncaughtThrowError"),
        "EncodingError" => Some("EncodingError"),
        "Encoding::CompatibilityError" => Some("Encoding::CompatibilityError"),
        "FiberError" => Some("FiberError"),
        "IOError" => Some("IOError"),
        "EOFError" => Some("EOFError"),
        "IO::TimeoutError" => Some("IO::TimeoutError"),
        "IndexError" => Some("IndexError"),
        "KeyError" => Some("KeyError"),
        "StopIteration" => Some("StopIteration"),
        "ClosedQueueError" => Some("ClosedQueueError"),
        "LocalJumpError" => Some("LocalJumpError"),
        "Math::DomainError" => Some("Math::DomainError"),
        "NameError" => Some("NameError"),
        "NoMethodError" => Some("NoMethodError"),
        "NoMatchingPatternError" => Some("NoMatchingPatternError"),
        "NoMatchingPatternKeyError" => Some("NoMatchingPatternKeyError"),
        "RangeError" => Some("RangeError"),
        "FloatDomainError" => Some("FloatDomainError"),
        "RegexpError" => Some("RegexpError"),
        "Regexp::TimeoutError" => Some("Regexp::TimeoutError"),
        "RuntimeError" => Some("RuntimeError"),
        "FrozenError" => Some("FrozenError"),
        "Ractor::Error" => Some("Ractor::Error"),
        "Timeout::Error" => Some("Timeout::Error"),
        "SocketError" => Some("SocketError"),
        "SystemCallError" => Some("SystemCallError"),
        "ThreadError" => Some("ThreadError"),
        "TypeError" => Some("TypeError"),
        "ZeroDivisionError" => Some("ZeroDivisionError"),
        "IO::EAGAINWaitReadable" => Some("IO::EAGAINWaitReadable"),
        "IO::EAGAINWaitWritable" => Some("IO::EAGAINWaitWritable"),
        "IO::EINPROGRESSWaitReadable" => Some("IO::EINPROGRESSWaitReadable"),
        "IO::EINPROGRESSWaitWritable" => Some("IO::EINPROGRESSWaitWritable"),
        _ => None,
    }
}

fn canonical_name(exc: &Exc) -> &str {
    match exc {
        Exc::Known(name) => name,
        Exc::Errno(name) => name.as_str(),
    }
}

/// Direct superclass canonical name, mirroring `Class#superclass` in a
/// RuboCop 1.87 runtime. Generic `Errno::*` are direct `SystemCallError`
/// children.
fn parent_of(name: &str) -> Option<&'static str> {
    if let Some(known) = known_parent(name) {
        return Some(known);
    }
    if name.starts_with("Errno::") {
        return Some("SystemCallError");
    }
    None
}

fn known_parent(name: &str) -> Option<&'static str> {
    match name {
        "Exception" => None,
        "NoMemoryError" => Some("Exception"),
        "ScriptError" => Some("Exception"),
        "LoadError" => Some("ScriptError"),
        "NotImplementedError" => Some("ScriptError"),
        "SyntaxError" => Some("ScriptError"),
        "SecurityError" => Some("Exception"),
        "SignalException" => Some("Exception"),
        "Interrupt" => Some("SignalException"),
        "SystemExit" => Some("Exception"),
        "SystemStackError" => Some("Exception"),
        "StandardError" => Some("Exception"),
        "ArgumentError" => Some("StandardError"),
        "UncaughtThrowError" => Some("ArgumentError"),
        "EncodingError" => Some("StandardError"),
        "Encoding::CompatibilityError" => Some("EncodingError"),
        "FiberError" => Some("StandardError"),
        "IOError" => Some("StandardError"),
        "EOFError" => Some("IOError"),
        "IO::TimeoutError" => Some("IOError"),
        "IndexError" => Some("StandardError"),
        "KeyError" => Some("IndexError"),
        "StopIteration" => Some("IndexError"),
        "ClosedQueueError" => Some("StopIteration"),
        "LocalJumpError" => Some("StandardError"),
        "Math::DomainError" => Some("StandardError"),
        "NameError" => Some("StandardError"),
        "NoMethodError" => Some("NameError"),
        "NoMatchingPatternError" => Some("StandardError"),
        "NoMatchingPatternKeyError" => Some("NoMatchingPatternError"),
        "RangeError" => Some("StandardError"),
        "FloatDomainError" => Some("RangeError"),
        "RegexpError" => Some("StandardError"),
        "Regexp::TimeoutError" => Some("RegexpError"),
        "RuntimeError" => Some("StandardError"),
        "FrozenError" => Some("RuntimeError"),
        "Ractor::Error" => Some("RuntimeError"),
        "Timeout::Error" => Some("RuntimeError"),
        "SocketError" => Some("StandardError"),
        "SystemCallError" => Some("StandardError"),
        "ThreadError" => Some("StandardError"),
        "TypeError" => Some("StandardError"),
        "ZeroDivisionError" => Some("StandardError"),
        "IO::EAGAINWaitReadable" => Some("Errno::EAGAIN"),
        "IO::EAGAINWaitWritable" => Some("Errno::EAGAIN"),
        "IO::EINPROGRESSWaitReadable" => Some("Errno::EINPROGRESS"),
        "IO::EINPROGRESSWaitWritable" => Some("Errno::EINPROGRESS"),
        _ => None,
    }
}

fn is_ancestor(ancestor: &str, descendant: &str) -> bool {
    let mut current = parent_of(descendant);
    while let Some(parent) = current {
        if parent == ancestor {
            return true;
        }
        current = parent_of(parent);
    }
    false
}

/// True when both are direct `SystemCallError` children (`ancestors[1] ==
/// SystemCallError` in RuboCop). RuboCop never treats such a pair as
/// comparable: identical codes are excluded by the `Errno` check and
/// siblings have nil `<=>`.
fn is_errno_sibling_pair(a: &Exc, b: &Exc) -> bool {
    parent_of(canonical_name(a)) == Some("SystemCallError")
        && parent_of(canonical_name(b)) == Some("SystemCallError")
}

fn group_contains_exception(group: &[Option<Exc>]) -> bool {
    group.iter().any(|exc| {
        matches!(exc, Some(Exc::Known("Exception")))
    })
}

fn contains_multiple_levels(group: &[Option<Exc>]) -> bool {
    // Always treat `Exception` as the highest level exception.
    if group.len() > 1 && group_contains_exception(group) {
        return true;
    }
    for i in 0..group.len() {
        for j in i + 1..group.len() {
            let (Some(a), Some(b)) = (&group[i], &group[j]) else {
                continue;
            };
            if same_group_shadows(a, b) {
                return true;
            }
        }
    }
    false
}

fn same_group_shadows(a: &Exc, b: &Exc) -> bool {
    if is_errno_sibling_pair(a, b) {
        return false;
    }
    let an = canonical_name(a);
    let bn = canonical_name(b);
    an == bn || is_ancestor(an, bn) || is_ancestor(bn, an)
}

/// `Module#<=>` for two resolved classes: `0` if equal, `1` if `a` is an
/// ancestor of `b`, `-1` if `b` is an ancestor of `a`, else nil.
fn class_cmp(a: &Exc, b: &Exc) -> Option<i32> {
    let an = canonical_name(a);
    let bn = canonical_name(b);
    if an == bn {
        return Some(0);
    }
    if is_ancestor(an, bn) {
        return Some(1);
    }
    if is_ancestor(bn, an) {
        return Some(-1);
    }
    None
}

fn elem_cmp(a: &Option<Exc>, b: &Option<Exc>) -> Option<i32> {
    match (a, b) {
        (None, None) => Some(0),
        (None, _) | (_, None) => None,
        (Some(a), Some(b)) => class_cmp(a, b),
    }
}

/// Ruby `Array#<=>` for rescued groups, with `nil` for incomparable.
fn array_cmp(x: &[Option<Exc>], y: &[Option<Exc>]) -> Option<i32> {
    let common = x.len().min(y.len());
    for i in 0..common {
        match elem_cmp(&x[i], &y[i]) {
            None => return None,
            Some(0) => continue,
            Some(ordered) => return Some(ordered),
        }
    }
    if x.len() == y.len() {
        Some(0)
    } else if x.len() < y.len() {
        Some(-1)
    } else {
        Some(1)
    }
}

fn pair_sorted(x: &[Option<Exc>], y: &[Option<Exc>]) -> bool {
    if group_contains_exception(x) {
        return false;
    }
    if group_contains_exception(y)
        || x.iter().all(|exc| exc.is_none())
        || y.iter().all(|exc| exc.is_none())
    {
        return true;
    }
    (array_cmp(x, y).unwrap_or(0)) <= 0
}

#[cfg(test)]
mod tests {
    use super::ShadowedException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_shadowed_exception_in_later_rescue() {
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue Exception
            ^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              handle_exception
            rescue StandardError
              handle_standard_error
            end
        "#});
    }

    #[test]
    fn accepts_broad_before_narrow_broken_by_unknown() {
        // RuboCop only compares consecutive groups: an intervening unknown
        // (`nil`) group is `none?` and counts as sorted, so the earlier
        // `StandardError` does not shadow the later `RuntimeError`.
        test::<ShadowedException>().expect_no_offenses(indoc! {r#"
            begin
              something
            rescue StandardError
              handle_standard_error
            rescue UnknownException
              handle_unknown
            rescue RuntimeError
              handle_runtime_error
            end
        "#});
    }

    #[test]
    fn flags_exception_before_unknown() {
        // `Exception` in an earlier group always shadows, even unknowns.
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              a
            rescue Exception
            ^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              b
            rescue UnknownException
              c
            end
        "#});
    }

    #[test]
    fn flags_multiple_levels_or_duplicates_in_same_rescue() {
        test::<ShadowedException>()
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError, NameError
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  foo
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue NameError, NameError
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  foo
                end
            "#});
    }

    #[test]
    fn flags_exception_with_unknown_in_same_rescue() {
        // `Exception` with any sibling in one group always flags, even when
        // the sibling is unresolvable.
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              something
            rescue NonStandardError, Exception
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              handle_error
            end
        "#});
    }

    #[test]
    fn flags_top_level_prefixed_and_system_call_error_shadowing() {
        test::<ShadowedException>()
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue ::StandardError
                ^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue RuntimeError
                  handle_runtime_error
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue SystemCallError
                  handle_system_call_error
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError
                ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  handle_standard_error
                rescue Errno::ENOENT
                  handle_errno
                end
            "#});
    }

    #[test]
    fn accepts_distinct_errno_siblings_in_one_group() {
        // Mastodon FP: distinct `Errno::*` constants are SystemCallError
        // subclasses whose `<=>` is nil (incomparable), so RuboCop never flags
        // a group of distinct Errno siblings. Clean.
        test::<ShadowedException>().expect_no_offenses(indoc! {r#"
            begin
              foo
            rescue Errno::EEXIST, Errno::ENOTEMPTY, Errno::ENOENT
              bar
            end
        "#});
    }

    #[test]
    fn accepts_duplicate_errno_in_one_group() {
        // Even identical `Errno::*` constants do not shadow (their `Errno`
        // codes compare equal, excluding the pair), unlike duplicate
        // `NameError`. Clean.
        test::<ShadowedException>().expect_no_offenses(indoc! {r#"
            begin
              foo
            rescue Errno::ENOENT, Errno::ENOENT
              bar
            end
        "#});
    }

    #[test]
    fn flags_standard_error_shadowing_errno() {
        // Regression guard: a broad `StandardError` rescue before a narrow
        // `Errno::ENOENT` rescue still shadows it.
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              foo
            rescue StandardError
            ^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              a
            rescue Errno::ENOENT
              b
            end
        "#});
    }

    #[test]
    fn flags_system_call_error_shadowing_errno() {
        // `SystemCallError` is the direct superclass of every `Errno::*`, so a
        // broad `SystemCallError` rescue before a narrow `Errno::ENOENT` rescue
        // shadows it.
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              foo
            rescue SystemCallError
            ^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              a
            rescue Errno::ENOENT
              b
            end
        "#});
    }

    #[test]
    fn flags_builtin_hierarchy_beyond_core_ten() {
        // The static table covers core built-ins beyond the original ten:
        // `IOError` is the parent of `EOFError`, `StandardError` the parent
        // of `IOError`, and `Timeout::Error`/`FrozenError` sit under
        // `RuntimeError`.
        test::<ShadowedException>()
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue IOError
                ^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  a
                rescue EOFError
                  b
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue StandardError, IOError
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  foo
                end
            "#})
            .expect_offense(indoc! {r#"
                begin
                  something
                rescue RuntimeError
                ^^^^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
                  a
                rescue Timeout::Error
                  b
                end
            "#});
    }

    #[test]
    fn accepts_narrow_before_broad_and_unrelated_chains() {
        test::<ShadowedException>()
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue EOFError
                  a
                rescue IOError
                  b
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue ArgumentError
                  b
                rescue Interrupt
                  c
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue Interrupt
                  b
                rescue ArgumentError
                  c
                end
            "#});
    }

    #[test]
    fn accepts_duplicates_across_groups_but_flags_same_group() {
        // Identical classes across consecutive groups are `0 <= 0` sorted in
        // RuboCop, so only same-group duplicates flag.
        test::<ShadowedException>().expect_no_offenses(indoc! {r#"
            begin
              something
            rescue NameError
              a
            rescue NameError
              b
            end
        "#});
    }

    #[test]
    fn accepts_splat_and_unknown_shapes() {
        test::<ShadowedException>()
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue *FOO
                  b
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue *FOO
                  b
                rescue *BAR
                  c
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue StandardError
                  b
                rescue *BAR
                  c
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue StandardError
                  b
                rescue UnknownException
                  c
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                begin
                  a
                rescue foo
                  b
                rescue [bar]
                  c
                end
            "#});
    }

    #[test]
    fn flags_exception_before_splat() {
        test::<ShadowedException>().expect_offense(indoc! {r#"
            begin
              a
            rescue Exception
            ^^^^^^^^^^^^^^^^ Do not shadow rescued Exceptions.
              b
            rescue *BAR
              c
            end
        "#});
    }

    #[test]
    fn accepts_narrow_before_broad_single_rescue_modifier_and_unknowns() {
        test::<ShadowedException>()
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue StandardError
                  handle_standard_error
                rescue Exception
                  handle_exception
                end
            "#})
            .expect_no_offenses("foo rescue nil\n")
            .expect_no_offenses(indoc! {r#"
                begin
                  something
                rescue UnknownException
                  handle_unknown
                rescue StandardError
                  handle_standard_error
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ShadowedException);
