//! `Gemspec/RequireMFA` — a `Gem::Specification.new do |spec| … end` gemspec
//! must set `metadata['rubygems_mfa_required'] = 'true'`. Flags a missing
//! entry, or one whose value is anything other than the string `'true'`. The
//! cop runs only on `*.gemspec` files; the host applies the per-cop `Include`
//! from `config/default.yml`, so this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/RequireMFA
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 (`gemspec/require_mfa.rb` + `mixin/gemspec_help.rb`).
//!   Single `MSG` (`` `metadata['rubygems_mfa_required']` must be set to `'true'`. ``)
//!   for both branches; safe autocorrect (no `Safe`/`SafeAutoCorrect` key → safe).
//!
//!   GEM-SPEC MATCH — `gem_specification` matches a `Block` whose call is
//!   `Gem::Specification.new` with exactly one named block arg
//!   (`(block (send (const (const {cbase nil?} :Gem) :Specification) :new)
//!   (args (arg $_)) …)`). Only `Block` is handled — RuboCop silences
//!   `NumblockHandler`/`ItblockHandler`, so a `_1`/`it`-form gemspec is NOT
//!   flagged (pinned). The receiver const is resolved via `cx.const_name`, which
//!   collapses a `cbase`-rooted `::Gem::Specification` to `"Gem::Specification"`,
//!   matching RuboCop's `{cbase nil?}`.
//!
//!   DETECTION (faithful to the source's control flow):
//!   `metadata(node)` uses a backtick *descend* that captures the **first**
//!   metadata-assignment value in pre-order — either `(send _ :metadata= $_)` or
//!   `(send (send _ :metadata) :[]= {(str "rubygems_mfa_required")|(sym …)} $_)`.
//!   First-assignment-wins: if the first `spec.metadata = {…}` lacks the key,
//!   the cop reports MISSING even when a later index-assignment sets it.
//!   `mfa_value`: no metadata value → MISSING; the captured value is itself a
//!   `str` (index-assignment form) → that str IS the value; otherwise (hash
//!   form) → the first `rubygems_mfa_required` pair's value (string OR symbol
//!   key), else nil. Value present and `!= (str "true")` → offense on the VALUE
//!   node (quotes included). Value nil (no metadata, OR a hash without the mfa
//!   pair) → offense on the WHOLE BLOCK node. `'true'` and `"true"` both parse to
//!   `(str "true")` → both clean.
//!
//!   AUTOCORRECT (all four paths, byte-exact with RuboCop, idempotent):
//!   (1) wrong value → replace the value node with `'true'`;
//!   (2) hash with pairs → insert `",\n'rubygems_mfa_required' => 'true'"` after
//!   the last pair; (3) empty hash → insert `"'rubygems_mfa_required' => 'true'"`
//!   before the closing `}`; (4) no metadata → insert
//!   `"\n{block_var}.metadata['rubygems_mfa_required'] = 'true'"` after the last
//!   metadata-assignment if one exists, else
//!   `"{block_var}.metadata['rubygems_mfa_required'] = 'true'\n"` before the
//!   block's `end`. A `metadata=` whose value is not a hash and not a string
//!   (RuboCop's `return unless metadata.hash_type?`) is flagged but NOT corrected.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct RequireMFA;

const MSG: &str = "`metadata['rubygems_mfa_required']` must be set to `'true'`.";

#[cop(
    name = "Gemspec/RequireMFA",
    description = "Requires a gemspec to have `rubygems_mfa_required` metadata set.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl RequireMFA {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // RuboCop's `gem_specification` is a `def_node_search`, so every matching
        // `Gem::Specification.new do |spec| … end` block in the file is handled.
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(block_var) = gem_specification_block_var(node, cx) else {
                continue;
            };
            self.check_spec_block(node, block_var, cx);
        }
    }
}

impl RequireMFA {
    fn check_spec_block(&self, block: NodeId, block_var: &str, cx: &Cx<'_>) {
        let metadata_value = first_metadata_value(block, cx);
        let mfa_value = mfa_value(metadata_value, cx);

        match mfa_value {
            Some(value) => {
                if !is_true_string(value, cx) {
                    cx.emit_offense(cx.range(value), MSG, None);
                    // Wrong value → replace the value node with `'true'`.
                    cx.emit_edit(cx.range(value), "'true'");
                }
            }
            None => {
                // RuboCop `add_offense(node)` on the whole block; its own
                // `expect_offense` highlights only the block's first line
                // (`Gem::Specification.new do |spec|`). `first_line_range`
                // reproduces that exact caret span; the reported start location
                // (block start) is identical to RuboCop's.
                cx.emit_offense(crate::cops::util::first_line_range(block, cx), MSG, None);
                self.autocorrect_missing(block, block_var, metadata_value, cx);
            }
        }
    }

    /// RuboCop's `autocorrect` for the missing branch: correct the existing hash,
    /// or insert a fresh `metadata[...] = 'true'` directive.
    fn autocorrect_missing(
        &self,
        block: NodeId,
        block_var: &str,
        metadata_value: Option<NodeId>,
        cx: &Cx<'_>,
    ) {
        if let Some(metadata) = metadata_value {
            // `return unless metadata.hash_type?` — a non-hash `metadata=` value
            // is flagged but not corrected.
            if matches!(cx.kind(metadata), NodeKind::Hash(_)) {
                correct_metadata(metadata, cx);
            }
        } else {
            insert_mfa_required(block, block_var, cx);
        }
    }
}

/// `gem_specification` — the block var name if `block` is a
/// `Gem::Specification.new do |spec| … end` block (exactly one named arg).
/// `None` otherwise. Only `Block` (not `Numblock`/`Itblock`) matches, mirroring
/// RuboCop's silenced `NumblockHandler`/`ItblockHandler`.
fn gem_specification_block_var<'a>(block: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if !matches!(cx.kind(block), NodeKind::Block { .. }) {
        return None;
    }
    let call = cx.block_call(block).get()?;
    if cx.method_name(call) != Some("new") {
        return None;
    }
    let receiver = cx.call_receiver(call).get()?;
    if cx.const_name(receiver).as_deref() != Some("Gem::Specification") {
        return None;
    }
    let args = cx.block_arguments(block).get()?;
    match cx.children(args).as_slice() {
        [single] => match *cx.kind(*single) {
            NodeKind::Arg(name) => Some(cx.symbol_str(name)),
            _ => None,
        },
        _ => None,
    }
}

/// RuboCop's `metadata` matcher: the captured value of the **first**
/// metadata-assignment node in pre-order within the block — either
/// `(send _ :metadata= $_)` or
/// `(send (send _ :metadata) :[]= {str "rubygems_mfa_required" | sym} $_)`.
fn first_metadata_value(block: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    cx.descendants(block)
        .into_iter()
        .find_map(|node| metadata_assignment_value(node, cx))
}

/// The assigned value if `node` is a metadata assignment (either form), else
/// `None`. The `:[]=` form additionally requires the key to be the
/// `rubygems_mfa_required` string/symbol.
fn metadata_assignment_value(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    let method = cx.method_name(node)?;

    // `(send _ :metadata= $_)`
    if method == "metadata=" {
        return cx.call_arguments(node).first().copied();
    }

    // `(send (send _ :metadata) :[]= {str/sym key} $_)`
    if method == "[]=" {
        let receiver = cx.call_receiver(node).get()?;
        if cx.method_name(receiver) != Some("metadata") {
            return None;
        }
        let args = cx.call_arguments(node);
        let [key, value] = args else {
            return None;
        };
        if !is_mfa_key(*key, cx) {
            return None;
        }
        return Some(*value);
    }

    None
}

/// RuboCop's `metadata_assignment` (`def_node_search`): any metadata
/// assignment, regardless of key — `(send _ :metadata= _)` or
/// `(send (send _ :metadata) :[]= {str sym} _)`. Used only to choose the
/// autocorrect insertion point (after the *last* such assignment), so unlike
/// `metadata_assignment_value` it does NOT require the `rubygems_mfa_required`
/// key.
fn is_metadata_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    if method == "metadata=" {
        return true;
    }
    if method == "[]=" {
        let Some(receiver) = cx.call_receiver(node).get() else {
            return false;
        };
        if cx.method_name(receiver) != Some("metadata") {
            return false;
        }
        // `{str sym}` key — any string/symbol literal.
        let args = cx.call_arguments(node);
        return matches!(
            args.first().map(|&k| cx.kind(k)),
            Some(NodeKind::Str(_) | NodeKind::Sym(_))
        );
    }
    false
}

/// RuboCop's `mfa_value`: nil metadata → nil; a `str` value (index-assignment
/// form) is the value itself; otherwise search the hash for the first
/// `rubygems_mfa_required` pair's value.
fn mfa_value(metadata_value: Option<NodeId>, cx: &Cx<'_>) -> Option<NodeId> {
    let value = metadata_value?;
    if matches!(cx.kind(value), NodeKind::Str(_)) {
        return Some(value);
    }
    // `def_node_search :rubygems_mfa_required` → first matching pair, anywhere
    // beneath the captured value (almost always a hash literal).
    std::iter::once(value)
        .chain(cx.descendants(value))
        .filter_map(|n| {
            if !matches!(cx.kind(n), NodeKind::Pair { .. }) {
                return None;
            }
            let key = cx.pair_key(n).get()?;
            if !is_mfa_key(key, cx) {
                return None;
            }
            cx.pair_value(n).get()
        })
        .next()
}

/// `{(str "rubygems_mfa_required") (sym :rubygems_mfa_required)}`.
fn is_mfa_key(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Str(id) => cx.string_str(id) == "rubygems_mfa_required",
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "rubygems_mfa_required",
        _ => false,
    }
}

/// RuboCop's `true_string?`: `(str "true")`.
fn is_true_string(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Str(id) if cx.string_str(id) == "true")
}

/// RuboCop's `correct_metadata`: append after the last pair, or insert before
/// the closing `}` of an empty hash.
fn correct_metadata(metadata: NodeId, cx: &Cx<'_>) {
    let pairs = cx.hash_pairs(metadata);
    if let Some(&last_pair) = pairs.last() {
        let at = cx.range(last_pair).end;
        cx.emit_edit(Range { start: at, end: at }, ",\n'rubygems_mfa_required' => 'true'");
    } else if let Some(close) = hash_close_brace(metadata, cx) {
        cx.emit_edit(Range { start: close, end: close }, "'rubygems_mfa_required' => 'true'");
    }
}

/// RuboCop's `insert_mfa_required`: place the directive after the last
/// metadata-assignment if any exists, else before the block's `end`.
fn insert_mfa_required(block: NodeId, block_var: &str, cx: &Cx<'_>) {
    let directive = format!("{block_var}.metadata['rubygems_mfa_required'] = 'true'");

    let last_assignment = cx
        .descendants(block)
        .into_iter()
        .rfind(|&node| is_metadata_assignment(node, cx));

    if let Some(last) = last_assignment {
        let at = cx.range(last).end;
        cx.emit_edit(Range { start: at, end: at }, &format!("\n{directive}"));
    } else if let Some(end) = block_end_keyword_start(block, cx) {
        cx.emit_edit(Range { start: end, end }, &format!("{directive}\n"));
    }
}

/// Start offset of the closing `}` of a hash literal.
fn hash_close_brace(metadata: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let range = cx.range(metadata);
    cx.tokens_in(range)
        .iter()
        .rev()
        .find(|t| t.kind == SourceTokenKind::RightBrace)
        .map(|t| t.range.start)
}

/// Start offset of the block's closing `end` keyword (RuboCop's `node.loc.end`).
fn block_end_keyword_start(block: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let range = cx.range(block);
    cx.tokens_in(range)
        .iter()
        .rev()
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"end"
        })
        .map(|t| t.range.start)
}

murphy_plugin_api::submit_cop!(RequireMFA);

#[cfg(test)]
mod tests {
    use super::RequireMFA;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_index_assignment_false() {
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['rubygems_mfa_required'] = 'false'
                                                       ^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
            end
        "#});
    }

    #[test]
    fn flags_hash_assignment_false() {
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
                'rubygems_mfa_required' => 'false'
                                           ^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
              }
            end
        "#});
    }

    #[test]
    fn flags_symbol_key_false() {
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
                rubygems_mfa_required: 'false'
                                       ^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
              }
            end
        "#});
    }

    #[test]
    fn allows_index_assignment_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['rubygems_mfa_required'] = 'true'
            end
        "#});
    }

    #[test]
    fn allows_hash_assignment_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
                'rubygems_mfa_required' => 'true'
              }
            end
        "#});
    }

    #[test]
    fn allows_double_quoted_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['rubygems_mfa_required'] = "true"
            end
        "#});
    }

    #[test]
    fn ignores_non_gemspec_block() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Foo.new do |spec|
              spec.name = 'x'
            end
        "#});
    }

    #[test]
    fn ignores_numblock_gemspec() {
        // RuboCop silences NumblockHandler — a `_1`-form gemspec is not handled.
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do
              _1.name = 'x'
            end
        "#});
    }

    #[test]
    fn first_assignment_wins_hash_without_key() {
        // The first `metadata =` lacks the key → MISSING, even though a later
        // index-assignment sets it.
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
              spec.metadata = {}
              spec.metadata['rubygems_mfa_required'] = 'true'
            end
        "#});
    }

    #[test]
    fn allows_symbol_key_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
                rubygems_mfa_required: 'true'
              }
            end
        "#});
    }

    #[test]
    fn allows_other_keys_with_mfa_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
                'foo' => 'bar',
                'rubygems_mfa_required' => 'true',
                'baz' => 'quux'
              }
            end
        "#});
    }

    #[test]
    fn allows_key_assignment_with_mfa_true() {
        test::<RequireMFA>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['foo'] = 'bar'
              spec.metadata['rubygems_mfa_required'] = 'true'
            end
        "#});
    }

    #[test]
    fn flags_blank_specification() {
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
            end
        "#});
    }

    #[test]
    fn flags_hash_without_mfa_key() {
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
              spec.metadata = {
              }
            end
        "#});
    }

    #[test]
    fn flags_non_hash_metadata_no_correction() {
        // RuboCop's `return unless metadata.hash_type?` — flagged, not corrected.
        test::<RequireMFA>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
              spec.metadata = Metadata.new
            end
        "#});
        test::<RequireMFA>().expect_no_corrections(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = Metadata.new
            end
        "#});
    }

    #[test]
    fn autocorrect_wrong_value_index_assignment() {
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata['rubygems_mfa_required'] = 'false'
                                                           ^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata['rubygems_mfa_required'] = 'true'
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_wrong_value_in_hash() {
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata = {
                    'rubygems_mfa_required' => 'false'
                                               ^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                  }
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata = {
                    'rubygems_mfa_required' => 'true'
                  }
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_blank_specification() {
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                spec.metadata['rubygems_mfa_required'] = 'true'
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_empty_hash() {
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                  spec.metadata = {
                  }
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata = {
                  'rubygems_mfa_required' => 'true'}
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_hash_with_pairs_appends() {
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                  spec.metadata = {
                    'foo' => 'bar',
                    'baz' => 'quux'
                  }
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata = {
                    'foo' => 'bar',
                    'baz' => 'quux',
                'rubygems_mfa_required' => 'true'
                  }
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_index_assignment_after_last_metadata() {
        // The directive goes after the last metadata-assignment, not after a
        // trailing non-metadata assignment (`spec.author = …`).
        test::<RequireMFA>().expect_correction(
            indoc! {r#"
                Gem::Specification.new do |spec|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['rubygems_mfa_required']` must be set to `'true'`.
                  spec.metadata['foo'] = 'bar'
                  spec.author = 'viralpraxis'
                end
            "#},
            indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.metadata['foo'] = 'bar'
                spec.metadata['rubygems_mfa_required'] = 'true'
                  spec.author = 'viralpraxis'
                end
            "#},
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        // Re-feeding each correction's output yields no further edits (fixpoint).
        // Index-assignment fix:
        test::<RequireMFA>().expect_no_corrections(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['rubygems_mfa_required'] = 'true'
            end
        "#});
        // Empty-hash fix output (`'rubygems_mfa_required' => 'true'}` before `}`):
        test::<RequireMFA>().expect_no_corrections(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {
              'rubygems_mfa_required' => 'true'}
            end
        "#});
        // Blank-specification fix output (directive inserted before `end`):
        test::<RequireMFA>().expect_no_corrections(indoc! {r#"
            Gem::Specification.new do |spec|
            spec.metadata['rubygems_mfa_required'] = 'true'
            end
        "#});
    }
}
