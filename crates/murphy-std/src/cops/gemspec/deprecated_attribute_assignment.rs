//! `Gemspec/DeprecatedAttributeAssignment` — flag deprecated attribute
//! assignments inside a `Gem::Specification.new do |spec| … end` block. The
//! deprecated attributes are `test_files`, `date`, `specification_version`, and
//! `rubygems_version`. Both direct assignment (`spec.test_files = …`) and
//! op-assignment (`spec.test_files += …`) are flagged. The cop runs only on
//! `*.gemspec` files; the host applies the per-cop `Include` from
//! `config/default.yml`, so this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/DeprecatedAttributeAssignment
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-mc15]
//! notes: >
//!   Detection is verified byte-for-byte against standalone rubocop 1.87.0.
//!   The single accepted gap is autocorrect: RuboCop `extend AutoCorrector`s a
//!   line-removal corrector (`range_by_whole_lines(..., include_final_newline:
//!   true)` → `corrector.remove`); Murphy ships detection-only here. Tracked by
//!   murphy-mc15.
//!
//!   Mirrors RuboCop's `on_block`: for each `Gem::Specification.new` /
//!   `::Gem::Specification.new` block (matched via the same
//!   `(const (const {cbase nil?} :Gem) :Specification)` shape as the sibling
//!   `Gemspec/AttributeAssignment`), take the block's first argument's *source*
//!   text as `block_parameter`, then `descendants.detect` the FIRST node that is
//!   a deprecated assignment and emit ONE offense. A block with two deprecated
//!   assignments yields exactly one offense (on the first in source order),
//!   verified against rubocop 1.87.0.
//!
//!   Block-parameter binding follows RuboCop exactly: `block_node.first_argument
//!   .source`. Numbered params (`_1`), `it`, and arg-less blocks have no
//!   `first_argument`; RuboCop would raise `NoMethodError` there, emitting no
//!   offense, so we simply skip blocks without an explicit first arg — observably
//!   identical (verified: `_1`/`it`/arg-less gemspec blocks produce no offense).
//!
//!   Deprecated-assignment match mirrors `use_deprecated_attributes?` +
//!   `node_and_method_name`:
//!     - Regular: a `Send` whose selector is `<attr>=` and whose receiver's
//!       *source* equals `block_parameter` (RuboCop compares
//!       `node.receiver&.source == block_parameter`, a source-text compare — so
//!       any receiver shape with matching text matches; we do the same with
//!       `cx.raw_source`).
//!     - Op-assign: an `OpAsgn` (`+=` etc.) whose target is a `Send` with
//!       selector `<attr>` (bare, no `=`) and matching receiver source — but ONLY
//!       `test_files` is flagged in this form. RuboCop's `use_deprecated_
//!       attributes?` reuses its `node` local: the first loop iteration
//!       (`attribute = test_files`) unconditionally rewrites `node` to `node.lhs`
//!       via the `op_asgn_type?` branch *regardless of match*, so iterations 2-4
//!       see a non-op-assign node and look for `:"#{attribute}="` (a setter name
//!       a bare getter lhs can never have). Net: op-assign matches only when the
//!       first attribute (`test_files`) matches. Verified against rubocop 1.87.0:
//!       `spec.test_files += x` flags; `spec.date += x` /
//!       `spec.specification_version += x` / `spec.rubygems_version += x` do not.
//!       RuboCop's `op_asgn_type?` is `OpAsgn` only — `||=`/`&&=` are
//!       `or_asgn`/`and_asgn` and are NOT flagged (verified: `spec.test_files
//!       ||= x` → no offense).
//!
//!   Offense range is `cx.range(node)` — the whole assignment statement
//!   (`add_offense(assignment)` highlights `assignment.source_range`). Verified
//!   byte-for-byte against rubocop 1.87.0 for single-line regular and op-assign
//!   forms (cols 3..41 / 3..34). For a multi-line value, RuboCop's highlight also
//!   spans to the value's last line, matching the full node range.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// The deprecated gemspec attributes, exactly RuboCop's
/// `%i[test_files date specification_version rubygems_version]`.
const DEPRECATED_ATTRIBUTES: [&str; 4] =
    ["test_files", "date", "specification_version", "rubygems_version"];

#[derive(Default)]
pub struct DeprecatedAttributeAssignment;

#[cop(
    name = "Gemspec/DeprecatedAttributeAssignment",
    description = "Checks that deprecated attribute assignments are not set in a gemspec file.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl DeprecatedAttributeAssignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // RuboCop's `on_block`: for each `Gem::Specification.new` block, emit at
        // most one offense (on the first deprecated assignment in source order).
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            if !matches!(cx.kind(node), NodeKind::Block { .. }) {
                continue;
            }
            if !is_gem_specification_call(cx.block_call(node).get(), cx) {
                continue;
            }
            // `block_node.first_argument.source`. No explicit first arg (`_1` /
            // `it` / arg-less) → RuboCop would raise; emit nothing → skip.
            let Some(block_parameter) = first_block_argument_source(node, cx) else {
                continue;
            };

            // `block_node.descendants.detect { use_deprecated_attributes? }` —
            // the first match only. `cx.descendants` is source order.
            if let Some((assignment, attr)) = cx
                .descendants(node)
                .into_iter()
                .find_map(|desc| deprecated_assignment(desc, block_parameter, cx))
            {
                let message = format!("Do not set `{attr}` in gemspec.");
                cx.emit_offense(cx.range(assignment), &message, None);
            }
        }
    }
}

/// `(node, attribute)` if `node` is a deprecated attribute assignment whose
/// receiver source equals `block_parameter`. Mirrors RuboCop's
/// `use_deprecated_attributes?` + `node_and_method_name`.
fn deprecated_assignment<'a>(
    node: NodeId,
    block_parameter: &str,
    cx: &Cx<'a>,
) -> Option<(NodeId, &'a str)> {
    match *cx.kind(node) {
        // Op-assign (`spec.test_files += …`): ONLY `test_files` is flagged, due
        // to a quirk in RuboCop's `use_deprecated_attributes?`. That method
        // reuses its `node` local: the first loop iteration (`attribute =
        // test_files`) unconditionally rewrites `node` to `node.lhs` (the bare
        // getter send) via the `op_asgn_type?` branch of `node_and_method_name`,
        // *regardless of match*. From iteration 2 on, `node` is no longer an
        // op-assign, so `node_and_method_name` returns `[node, :"#{attribute}="]`
        // — an `=`-suffixed name that a bare getter lhs (`spec.date`, method
        // `date`) can never match. Net effect: op-assign matches only when the
        // FIRST attribute (`test_files`) matches. The flagged node is the
        // op-assign itself. Verified against standalone rubocop 1.87.0:
        // `spec.test_files += x` flags, `spec.date += x` /
        // `spec.specification_version += x` / `spec.rubygems_version += x` do not.
        NodeKind::OpAsgn { target, .. } => {
            let attr = assignment_attr(target, block_parameter, false, cx)?;
            (attr == "test_files").then_some((node, attr))
        }
        // Regular assignment (`spec.test_files = …`): the send selector is
        // `<attr>=`. The flagged node is the send itself. All four deprecated
        // attributes are matched in this form.
        NodeKind::Send { .. } => {
            let attr = assignment_attr(node, block_parameter, true, cx)?;
            Some((node, attr))
        }
        _ => None,
    }
}

/// The deprecated attribute name if `send_node` is a `Send` whose selector is
/// the attribute (with a trailing `=` when `setter` is true, bare otherwise) and
/// whose receiver source equals `block_parameter`. `None` otherwise.
fn assignment_attr<'a>(
    send_node: NodeId,
    block_parameter: &str,
    setter: bool,
    cx: &Cx<'a>,
) -> Option<&'a str> {
    if !matches!(cx.kind(send_node), NodeKind::Send { .. }) {
        return None;
    }
    let method = cx.method_name(send_node)?;
    // RuboCop compares `node.receiver&.source == block_parameter`. `&.` short-
    // circuits a nil receiver to a failed match.
    let receiver = cx.call_receiver(send_node).get()?;
    if cx.raw_source(cx.range(receiver)) != block_parameter {
        return None;
    }
    DEPRECATED_ATTRIBUTES.into_iter().find(|&attr| {
        if setter {
            method
                .strip_suffix('=')
                .is_some_and(|bare| bare == attr)
        } else {
            method == attr
        }
    })
}

/// The raw source text of a block's first explicit argument — RuboCop's
/// `block_node.first_argument.source`. `None` when the block's `(args ...)` is
/// empty (numbered/`it`/arg-less blocks have no `first_argument`).
fn first_block_argument_source<'a>(block: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let args = cx.block_arguments(block).get()?;
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let first = *cx.list(list).first()?;
    Some(cx.raw_source(cx.range(first)))
}

/// True when `call` is `Gem::Specification.new` (or `::Gem::Specification.new`).
fn is_gem_specification_call(call: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(call) = call else {
        return false;
    };
    if cx.method_name(call) != Some("new") {
        return false;
    }
    let Some(receiver) = cx.call_receiver(call).get() else {
        return false;
    };
    is_gem_specification_const(receiver, cx)
}

/// True when `node` is the const `Gem::Specification` or `::Gem::Specification`,
/// mirroring RuboCop's `(const (const {cbase nil?} :Gem) :Specification)`.
fn is_gem_specification_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "Specification" {
        return false;
    }
    let Some(scope) = scope.get() else {
        return false;
    };
    cx.is_global_const(scope, "Gem")
}

murphy_plugin_api::submit_cop!(DeprecatedAttributeAssignment);

#[cfg(test)]
mod tests {
    use super::DeprecatedAttributeAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_test_files_direct_assignment() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "x"
              spec.test_files = Dir.glob("test/**/*")
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `test_files` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_test_files_op_assignment() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.test_files += Dir.glob("x")
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `test_files` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_date() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.date = "2020-01-01"
              ^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `date` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_specification_version() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.specification_version = 1
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `specification_version` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_rubygems_version() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.rubygems_version = "1"
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `rubygems_version` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_only_first_deprecated_assignment_per_block() {
        // RuboCop's `descendants.detect` returns the FIRST match only: a block
        // with two deprecated assignments yields exactly one offense.
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.test_files = Dir.glob("test/**/*")
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `test_files` in gemspec.
              spec.date = "2020-01-01"
            end
        "#});
    }

    #[test]
    fn flags_one_offense_per_block_separately() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |s|
              s.specification_version = 1
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `specification_version` in gemspec.
            end
            Gem::Specification.new do |s|
              s.rubygems_version = "1"
              ^^^^^^^^^^^^^^^^^^^^^^^^ Do not set `rubygems_version` in gemspec.
            end
        "#});
    }

    #[test]
    fn flags_with_cbase_gem_specification() {
        test::<DeprecatedAttributeAssignment>().expect_offense(indoc! {r#"
            ::Gem::Specification.new do |spec|
              spec.date = "2020"
              ^^^^^^^^^^^^^^^^^^ Do not set `date` in gemspec.
            end
        "#});
    }

    #[test]
    fn allows_non_deprecated_attributes() {
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "x"
              spec.version = "1.0.0"
              spec.files = Dir.glob("lib/**/*")
            end
        "#});
    }

    #[test]
    fn allows_or_assignment() {
        // `||=` is `or_asgn`, not `op_asgn` → not flagged (verified vs rubocop).
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.test_files ||= Dir.glob("x")
            end
        "#});
    }

    #[test]
    fn allows_op_assignment_for_non_test_files_attributes() {
        // RuboCop's `node`-reassignment quirk: op-assign flags ONLY `test_files`.
        // `date`, `specification_version`, and `rubygems_version` are NOT flagged
        // in op-assign form (verified byte-for-byte against rubocop 1.87.0).
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.date += "x"
            end
        "#});
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.specification_version += 1
            end
        "#});
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.rubygems_version += "1"
            end
        "#});
    }

    #[test]
    fn allows_numbered_block_param() {
        // `_1` block has no `first_argument` → RuboCop raises → no offense.
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do
              _1.test_files = "x"
            end
        "#});
    }

    #[test]
    fn allows_it_block_param() {
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do
              it.test_files = "x"
            end
        "#});
    }

    #[test]
    fn allows_assignment_on_non_block_var_receiver() {
        // Receiver source `other` != block_parameter `spec` → not flagged.
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              other.test_files = Dir.glob("x")
            end
        "#});
    }

    #[test]
    fn allows_deprecated_attribute_outside_gemspec_block() {
        // Not a `Gem::Specification.new` block → ignored entirely.
        test::<DeprecatedAttributeAssignment>().expect_no_offenses(indoc! {r#"
            foo.test_files = Dir.glob("x")
            bar do |spec|
              spec.date = "2020"
            end
        "#});
    }
}
