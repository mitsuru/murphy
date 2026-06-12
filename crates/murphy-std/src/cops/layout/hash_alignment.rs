//! `Layout/HashAlignment` — aligns the keys of a multi-line hash literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/HashAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-1d9i
//! notes: >
//!   Implements RuboCop's default `EnforcedHashRocketStyle: key` /
//!   `EnforcedColonStyle: key` behaviour (the `KeyAlignment` style): a
//!   multi-line hash literal whose pairs each begin their own line must align
//!   every such pair's key column with the first pair's key column. A
//!   misaligned key is flagged at the pair with the message "Align the keys of
//!   a hash literal if they span more than one line." Only pairs that begin
//!   their own line are checked (RuboCop's `Util.begins_its_line?`), and
//!   single-line hashes are skipped (`node.single_line?`).
//!   Known gaps versus RuboCop (all documented, none bypass the ABI):
//!   (1) Only the default `key` style is modelled. The `separator` and `table`
//!       styles (`EnforcedHashRocketStyle`/`EnforcedColonStyle`) and the
//!       separator/value column deltas they enforce are not implemented; this
//!       cop checks key-column alignment only, not separator or value alignment.
//!   (2) `EnforcedLastArgumentHashStyle` is not modelled — a hash passed as a
//!       method's last argument is always inspected (RuboCop's default
//!       `always_inspect`), but `always_ignore` / `ignore_explicit` /
//!       `ignore_implicit` are not honoured.
//!   (3) AUTOCORRECT IS NOT EMITTED. RuboCop reindents each misaligned key (and,
//!       for non-`key` styles, separators/values) via `AlignmentCorrector`;
//!       that whole-line reflow is deferred. The cop reports offenses only.
//!   (4) `KeywordSplatAlignment` (`**rest`) handling is not modelled.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

const MSG: &str = "Align the keys of a hash literal if they span more than one line.";

#[derive(Default)]
pub struct HashAlignment;

#[cop(
    name = "Layout/HashAlignment",
    description = "Align the keys of a multi-line hash literal.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HashAlignment {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let pairs = cx.hash_pairs(node);
    // `return if node.pairs.empty? || node.single_line?`.
    let Some(&first_pair) = pairs.first() else {
        return;
    };
    if is_single_line(node, cx) {
        return;
    }

    let src = cx.source();
    // Anchor column = the first pair's key column (the `key` style aligns every
    // line-beginning pair's key to the first pair's key).
    let Some(first_key) = pair_key(first_pair, cx) else {
        return;
    };
    let anchor_column = column_of(src, cx.range(first_key).start as usize);

    // `node.children.each` — the first pair is the anchor and is only checked
    // for separator/value deltas (not modelled here), so key-alignment offenses
    // start from the second pair onward, but only for pairs that begin their
    // own line.
    for &pair in &pairs[1..] {
        // `Util.begins_its_line?(current_pair.source_range)`: the pair is the
        // first non-whitespace token on its line.
        if !begins_its_line(pair, cx) {
            continue;
        }
        let Some(key) = pair_key(pair, cx) else {
            continue;
        };
        let key_column = column_of(src, cx.range(key).start as usize);
        // `key_delta = first_pair.key.column - current.key.column`; non-zero is
        // a misalignment.
        if key_column != anchor_column {
            cx.emit_offense(cx.range(pair), MSG, None);
        }
    }
}

/// The key node of a hash pair (its first child), or `None`.
fn pair_key(pair: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    cx.children(pair).first().copied()
}

/// Whether the hash literal occupies a single source line (RuboCop's
/// `node.single_line?`): the first and last pair share a line.
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let pairs = cx.hash_pairs(node);
    let (Some(&first), Some(&last)) = (pairs.first(), pairs.last()) else {
        return true;
    };
    let src = cx.source();
    line_of(cx.range(first).start, src) == line_of(cx.range(last).end.saturating_sub(1), src)
}

/// `Util.begins_its_line?`: the node's start is the first non-whitespace
/// position on its source line.
fn begins_its_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.source();
    let bytes = src.as_bytes();
    let start = cx.range(node).start as usize;
    let line_start = bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    bytes[line_start..start].iter().all(|&b| b == b' ' || b == b'\t')
}

/// 1-based source line number containing byte `offset`.
fn line_of(offset: u32, src: &str) -> usize {
    src.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// 0-based column (char count) of `offset` within its source line.
fn column_of(src: &str, offset: usize) -> usize {
    let bytes = src.as_bytes();
    let start = bytes[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[start..offset].chars().count()
}

murphy_plugin_api::submit_cop!(HashAlignment);

#[cfg(test)]
mod tests {
    use super::HashAlignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_misaligned_colon_key() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              foo: 1,
                barbaz: 2,
                ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
            }
        "#});
    }

    #[test]
    fn flags_misaligned_rocket_key() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              "a" => 1,
                "bb" => 2,
                ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
            }
        "#});
    }

    #[test]
    fn accepts_aligned_keys() {
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo: 1,
              barbaz: 2,
            }
        "#});
    }

    #[test]
    fn accepts_single_line_hash() {
        test::<HashAlignment>().expect_no_offenses("h = { foo: 1, bar: 2 }\n");
    }

    #[test]
    fn accepts_empty_hash() {
        test::<HashAlignment>().expect_no_offenses("h = {}\n");
    }

    #[test]
    fn ignores_pair_not_beginning_its_line() {
        // The second pair shares a line with the first, so it does not begin
        // its own line and is not subject to key-alignment.
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo: 1, bar: 2,
              baz: 3,
            }
        "#});
    }

    #[test]
    fn flags_multiple_misaligned_keys() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              a: 1,
               b: 2,
               ^^^^ Align the keys of a hash literal if they span more than one line.
                 c: 3,
                 ^^^^ Align the keys of a hash literal if they span more than one line.
            }
        "#});
    }

    #[test]
    fn emits_no_correction() {
        // Autocorrect is intentionally not implemented (documented parity gap).
        test::<HashAlignment>().expect_no_corrections(indoc! {r#"
            h = {
              foo: 1,
                barbaz: 2,
            }
        "#});
    }
}
