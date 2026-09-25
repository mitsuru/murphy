//! `RSpec/IndexedLet` — do not set up test data using indexed names (`item_1`, `item_2`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/IndexedLet
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`spec_group?`: example groups and shared
//!   groups, bare or `RSpec` receiver): direct body children that are
//!   indexed `let`/`let!` calls group by the name with digits stripped,
//!   and groups larger than `Max` (default 1) flag every member per
//!   `add_offense(let_node)` (whole node) with `This `let` statement uses
//!   `<index>` in its name. Please give it a meaningful name.` (`<index>`
//!   is the first digit run). `let_name` covers the block form
//!   (`let(:name) { }`, any further args per the trailing `...`) and the
//!   bare-send form (`let(:name, &blk)`, exactly one value arg plus a
//!   `BlockPass`); first arg must be `Str`/`Sym`, bare receiver only.
//!   `AllowedIdentifiers` / `AllowedPatterns` are accepted locally, but
//!   upstream additionally merges the `Naming/VariableNumber` values for
//!   both — cross-cop config reads are not ported in this batch (local
//!   options only). Numbered-param (`Numblock`) groups never count,
//!   matching the upstream `NumblockHandler` exclusion. No autocorrect
//!   upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a spec group:
//!
//! - `describe { let(:item_1) {}; let(:item_2) {} }` — both flagged.
//! - `describe { let(:item_1) {}; let(:other) {} }` — single-member
//!   group, clean.
//! - `describe { let(:item1) {}; let(:item2) {} }` — no-underscore
//!   digits still count, both flagged.
//! - `describe { let!("bang_1") {}; let!("bang_2") {} }` — `let!`
//!   counts too.
//! - `describe { let(:item_1, &:to_s); let(:item_2, &:to_s) }` —
//!   bare-send form, both flagged.
//! - `Max: 2` with three `item_N` — all three flagged; with two, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; renaming test data needs human
//! judgement about what each value represents.

use std::collections::BTreeMap;

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop, regex::Regex};

use crate::cops::rspec_helpers::{is_example_group_call, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IndexedLet;

#[derive(CopOptions)]
pub struct IndexedLetOptions {
    #[option(
        name = "Max",
        default = 1,
        description = "Maximum number of lets sharing the same de-indexed name."
    )]
    pub max: i64,
    #[option(
        name = "AllowedIdentifiers",
        default = [],
        description = "Let names exempt from the indexed-name check."
    )]
    pub allowed_identifiers: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns; matching let names are exempt."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "RSpec/IndexedLet",
    description = "Do not set up test data using indexes (e.g. `item_1`, `item_2`).",
    default_severity = "warning",
    default_enabled = true,
    options = IndexedLetOptions,
)]
impl IndexedLet {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_spec_group_call(cx, call) {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        // Upstream `node.body&.child_nodes`: direct children only — a
        // multi-statement body is a `Begin`, otherwise the single body
        // node itself.
        let children: Vec<NodeId> = match *cx.kind(body_id) {
            NodeKind::Begin(members) => cx.list(members).to_vec(),
            _ => vec![body_id],
        };
        let opts = cx.options_or_default::<IndexedLetOptions>();
        let mut groups: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for child in children {
            let Some(name) = indexed_let_name(cx, child, &opts) else {
                continue;
            };
            groups.entry(stripped_name(&name)).or_default().push(child);
        }
        for members in groups.values() {
            if members.len() as i64 <= opts.max {
                continue;
            }
            for &let_node in members {
                let name = indexed_let_name(cx, let_node, &opts).unwrap_or_default();
                cx.emit_offense(
                    cx.range(let_node),
                    &format!(
                        "This `let` statement uses `{}` in its name. Please give it a meaningful name.",
                        first_digit_run(&name),
                    ),
                    None,
                );
            }
        }
    }
}

/// `true` when `call` is a spec-group entrypoint (example groups or shared
/// groups) with a bare or `RSpec` receiver.
///
/// Mirrors upstream `spec_group?` (`{(shared, example) groups}` with
/// `#rspec?` = explicit-`RSpec` or bare).
fn is_spec_group_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        return false;
    }
    let name = cx.symbol_str(method);
    is_example_group_call(cx, call)
        || matches!(
            name,
            "shared_examples" | "shared_examples_for" | "shared_context"
        )
}

/// The let name when `node` is an indexed `let`/`let!` call, else `None`.
///
/// Mirrors upstream `indexed_let?`: a `let?` shape (`Helpers.all` = `let`
/// / `let!`, bare receiver) whose name matches `/_?\d+$/` and is neither
/// an allowed identifier nor matching an allowed pattern. Covers the block
/// form (`let(:name) { }`) and the bare-send form (`let(:name, &blk)`).
fn indexed_let_name(cx: &Cx<'_>, node: NodeId, opts: &IndexedLetOptions) -> Option<String> {
    let (method, arg_ids) = match *cx.kind(node) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver, method, args,
            } = *cx.kind(call)
            else {
                return None;
            };
            if receiver != OptNodeId::NONE {
                return None;
            }
            // Block form: further args allowed (upstream trailing `...`).
            (method, cx.list(args).to_vec())
        }
        NodeKind::Send {
            receiver, method, args,
        } => {
            if receiver != OptNodeId::NONE {
                return None;
            }
            let args = cx.list(args);
            // Bare-send form is exactly one value arg plus a `BlockPass`.
            if args.len() != 2 || !matches!(*cx.kind(args[1]), NodeKind::BlockPass(_)) {
                return None;
            }
            (method, args.to_vec())
        }
        _ => return None,
    };
    if !matches!(cx.symbol_str(method), "let" | "let!") {
        return None;
    }
    let name = let_first_name(cx, arg_ids.first().copied()?)?;
    if !has_index_suffix(&name) {
        return None;
    }
    if opts.allowed_identifiers.contains(&name) {
        return None;
    }
    if opts
        .allowed_patterns
        .iter()
        .any(|pat| Regex::new(pat).is_ok_and(|re| re.is_match(&name)))
    {
        return None;
    }
    Some(name)
}

/// The first-arg name of a `let` call: `Str` content or `Sym` spelling.
///
/// Mirrors upstream `let_name` (`({str sym} $_)`).
fn let_first_name(cx: &Cx<'_>, arg: NodeId) -> Option<String> {
    match *cx.kind(arg) {
        NodeKind::Str(id) => Some(cx.string_str(id).to_owned()),
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_owned()),
        _ => None,
    }
}

/// Mirrors `SUFFIX_INDEX_REGEX` (`/_?\d+$/`).
fn has_index_suffix(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i == bytes.len() {
        return false;
    }
    if i > 0 && bytes[i - 1] == b'_' {
        return true;
    }
    // `_?` is optional: `item1` counts (verified vs 3.7.0).
    true
}

/// Mirrors `INDEX_REGEX` (`/\d+/`): the first digit run in the name.
fn first_digit_run(name: &str) -> &str {
    let bytes = name.as_bytes();
    let mut start = None;
    for (i, b) in bytes.iter().enumerate() {
        if b.is_ascii_digit() {
            start = Some(i);
            break;
        }
    }
    let Some(s) = start else {
        return "";
    };
    let mut end = s;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    &name[s..end]
}

/// Mirrors `let_name_stripped_index` (`gsub(INDEX_REGEX, '')`): the name
/// with every digit run removed.
fn stripped_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for chunk in name.split(|c: char| c.is_ascii_digit()) {
        out.push_str(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{IndexedLet, IndexedLetOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn max_two() -> IndexedLetOptions {
        IndexedLetOptions {
            max: 2,
            allowed_identifiers: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    fn allow_item(pattern_or_id: &str, as_pattern: bool) -> IndexedLetOptions {
        if as_pattern {
            IndexedLetOptions {
                max: 1,
                allowed_identifiers: Vec::new(),
                allowed_patterns: vec![pattern_or_id.to_owned()],
            }
        } else {
            IndexedLetOptions {
                max: 1,
                allowed_identifiers: vec![pattern_or_id.to_owned()],
                allowed_patterns: Vec::new(),
            }
        }
    }

    #[test]
    fn flags_indexed_pair() {
        test::<IndexedLet>().expect_offense(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let(:item_2) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_indexed_let() {
        test::<IndexedLet>().expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  let(:other) { create(:item) }
                end
            "#});
    }

    #[test]
    fn flags_no_underscore_digits() {
        // `/_?\d+$/`: the underscore is optional (verified vs 3.7.0).
        test::<IndexedLet>().expect_offense(indoc! {r#"
                RSpec.describe "x" do
                  let(:item1) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let(:item2) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn flags_string_names_and_bang() {
        // `Str` first args and `let!` count (verified vs 3.7.0).
        test::<IndexedLet>().expect_offense(indoc! {r#"
                RSpec.describe "x" do
                  let("item_1") { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let!("item_2") { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn flags_bare_send_form() {
        // `let(:item_1, &:to_s)` — the `(send ... _ block_pass)` arm
        // (verified vs 3.7.0).
        test::<IndexedLet>().expect_offense(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1, &:to_s)
                  ^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let(:item_2, &:to_s)
                  ^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn max_two_flags_triple_only() {
        test::<IndexedLet>()
            .with_options(&max_two())
            .expect_offense(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let(:item_2) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                  let(:item_3) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `3` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn max_two_allows_pair() {
        test::<IndexedLet>()
            .with_options(&max_two())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  let(:item_2) { create(:item) }
                end
            "#});
    }

    #[test]
    fn allowed_identifier_skips() {
        test::<IndexedLet>()
            .with_options(&allow_item("item_1", false))
            .expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  let(:item_2) { create(:item) }
                end
            "#});
    }

    #[test]
    fn allowed_pattern_skips() {
        test::<IndexedLet>()
            .with_options(&allow_item("item", true))
            .expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  let(:item_2) { create(:item) }
                end
            "#});
    }

    #[test]
    fn different_stems_do_not_group() {
        // `item_` vs `other_`: single-member groups, clean.
        test::<IndexedLet>().expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  let(:item_1) { create(:item) }
                  let(:other_1) { create(:item) }
                end
            "#});
    }

    #[test]
    fn flags_shared_group() {
        // `spec_group?` includes shared groups (verified vs 3.7.0).
        test::<IndexedLet>().expect_offense(indoc! {r#"
                shared_examples "s" do
                  let(:sh_1) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `1` in its name. Please give it a meaningful name.
                  let(:sh_2) { create(:item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ This `let` statement uses `2` in its name. Please give it a meaningful name.
                end
            "#});
    }

    #[test]
    fn does_not_flag_lets_outside_groups() {
        test::<IndexedLet>().expect_no_offenses(indoc! {r#"
                let(:item_1) { create(:item) }
                let(:item_2) { create(:item) }
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_let() {
        // Upstream `let?` requires a bare (`nil?`) receiver.
        test::<IndexedLet>().expect_no_offenses(indoc! {r#"
                RSpec.describe "x" do
                  obj.let(:item_1) { create(:item) }
                  obj.let(:item_2) { create(:item) }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(IndexedLet);
