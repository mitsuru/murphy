//! `Naming/AsciiIdentifiers` — flag non-ASCII characters in identifier and
//! constant names.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/AsciiIdentifiers
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   RuboCop is token-based, scanning `tIDENTIFIER` and `tCONSTANT` tokens.
//!   Murphy reproduces that split via AST node kinds, which naturally
//!   excludes the same surfaces RuboCop excludes: instance/class/global
//!   variables (`@x`/`@@x`/`$x` lex as their own token kinds, not
//!   `tIDENTIFIER`), symbols, hash labels, keyword-argument labels
//!   (`kwarg`/`kwoptarg`), and string contents. Each exclusion was verified
//!   against rubocop 1.87.0.
//!
//!   Covered identifiers: local variables (`lvar`/`lvasgn`, including
//!   multiple- and op-assignment value-less targets and `for`-loop vars),
//!   method names and calls (`send`/`csend`, including safe navigation
//!   `&.`), method definitions (`def`/`defs`, including singleton `def
//!   self.x` and setter `name=`), and the positional/splat/block argument
//!   family (`arg`, `optarg`, `restarg`, `kwrestarg`, `blockarg`).
//!   Covered constants (gated by `AsciiConstants`, default true):
//!   `const`/`casgn`.
//!
//!   Known gaps vs RuboCop's exhaustive token scan (rare binding positions
//!   whose *reads* are still flagged as `lvar`, only the binding site is
//!   missed):
//!     * pattern-match capture bindings (`in Foo => bär`) — `match_var`,
//!       not visited;
//!     * block-local shadow args (`proc { |a; bär| }`) — Murphy does not
//!       emit a node for the shadow binding.
//!   `for`-loop variables, by contrast, are `lvasgn` nodes and ARE covered.
//!   Additionally, for a repeated identical name in a scope chain
//!   (`Cönst::Cönst`), the first-occurrence name search anchors the outer
//!   node's caret on the scope segment rather than the leaf — also rare.
//! ```
//!
//! ## Offense range
//!
//! Mirrors RuboCop's `first_offense_range`: only the **first contiguous run**
//! of non-ASCII characters within the name is highlighted, not the whole name
//! and not every run. `föo_bär` highlights only `ö`; `hello_🍣` highlights only
//! `🍣`. The range is computed in bytes (Murphy ranges are byte offsets).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const IDENTIFIER_MSG: &str = "Use only ascii symbols in identifiers.";
const CONSTANT_MSG: &str = "Use only ascii symbols in constants.";

#[derive(Default)]
pub struct AsciiIdentifiers;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AsciiConstants",
        default = true,
        description = "Check constant names for non-ascii characters."
    )]
    pub ascii_constants: bool,
}

#[cop(
    name = "Naming/AsciiIdentifiers",
    description = "Use only ascii symbols in identifiers and constants.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl AsciiIdentifiers {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root node itself; chain it so a
        // single top-level statement (e.g. `δ = 1`, whose root *is* the
        // `lvasgn`) is also inspected.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            // `is_constant == true` → the name is a constant (gated by
            // `AsciiConstants`); `false` → an identifier.
            let Some((name_start, name, is_constant)) = name_target(id, cx) else {
                continue;
            };

            if is_constant && !opts.ascii_constants {
                continue;
            }

            // Fast path: ASCII-only names never offend.
            if name.is_ascii() {
                continue;
            }

            let Some(offense_range) = first_non_ascii_run(name_start, name) else {
                continue;
            };

            let message = if is_constant {
                CONSTANT_MSG
            } else {
                IDENTIFIER_MSG
            };
            cx.emit_offense(offense_range, message, None);
        }
    }
}

/// Resolve the `(name_start_byte, name_str, is_constant)` triple for the node
/// kinds that correspond to RuboCop's `tIDENTIFIER` / `tCONSTANT` tokens.
/// Returns `None` for every other node kind (instance/class/global variables,
/// symbols, labels, strings, structural nodes), matching RuboCop's exclusions.
fn name_target<'a>(id: NodeId, cx: &Cx<'a>) -> Option<(u32, &'a str, bool)> {
    match *cx.kind(id) {
        // --- identifiers ---
        NodeKind::Lvar(name) => {
            let s = cx.symbol_str(name);
            Some((named_start(id, s, cx), s, false))
        }
        NodeKind::Lvasgn { name, .. } => {
            let s = cx.symbol_str(name);
            Some((named_start(id, s, cx), s, false))
        }
        // `send`/`csend` carry a populated `loc.name` (method-selector range),
        // which already skips any receiver and call operator (`.` / `&.`). Use
        // it directly; fall back to a name search if it is unset.
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            let name_loc = cx.node(id).loc.name;
            let s = cx.symbol_str(method);
            let start = if name_loc == Range::ZERO {
                named_start(id, s, cx)
            } else {
                name_loc.start
            };
            Some((start, s, false))
        }
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => {
            let s = cx.symbol_str(name);
            Some((named_start(id, s, cx), s, false))
        }
        // Argument family carries a populated `loc.name` that already skips the
        // ASCII sigil (`*`/`**`/`&`). `kwarg`/`kwoptarg` are intentionally
        // absent: their labels lex as `tLABEL`, not `tIDENTIFIER`.
        NodeKind::Arg(name)
        | NodeKind::Restarg(name)
        | NodeKind::Kwrestarg(name)
        | NodeKind::Blockarg(name) => {
            let name_loc = cx.node(id).loc.name;
            let s = cx.symbol_str(name);
            let start = if name_loc == Range::ZERO {
                named_start(id, s, cx)
            } else {
                name_loc.start
            };
            Some((start, s, false))
        }
        NodeKind::Optarg { name, .. } => {
            let name_loc = cx.node(id).loc.name;
            let s = cx.symbol_str(name);
            let start = if name_loc == Range::ZERO {
                named_start(id, s, cx)
            } else {
                name_loc.start
            };
            Some((start, s, false))
        }

        // --- constants ---
        NodeKind::Const { name, .. } => {
            let s = cx.symbol_str(name);
            Some((named_start(id, s, cx), s, true))
        }
        NodeKind::Casgn { name, .. } => {
            let s = cx.symbol_str(name);
            Some((named_start(id, s, cx), s, true))
        }

        _ => None,
    }
}

/// Byte offset where `name` begins inside node `id`'s expression source.
///
/// Used for node kinds whose `loc.name` is `Range::ZERO` in Murphy (`def`,
/// `const`, `casgn`, `lvar`, `lvasgn`). The name is located by its first
/// occurrence within the node's source range. This is robust because the
/// name precedes any `= value` / method body, and — for scoped constants
/// (`SOME::Cönst`) — only the leaf segment matches `name`. Falls back to the
/// expression start if the name is not found (should not happen for these
/// kinds).
fn named_start(id: NodeId, name: &str, cx: &Cx<'_>) -> u32 {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    match src.find(name) {
        Some(off) => expr.start + off as u32,
        None => expr.start,
    }
}

/// Byte range of the first contiguous run of non-ASCII characters within
/// `name`, anchored at `name_start`. Mirrors RuboCop's `first_offense_range`
/// (`/[^[:ascii:]]+/`). Returns `None` when `name` is entirely ASCII.
fn first_non_ascii_run(name_start: u32, name: &str) -> Option<Range> {
    let first = name.char_indices().find(|(_, c)| !c.is_ascii())?;
    let run_len: usize = name[first.0..]
        .chars()
        .take_while(|c| !c.is_ascii())
        .map(char::len_utf8)
        .sum();
    let start = name_start + first.0 as u32;
    Some(Range {
        start,
        end: start + run_len as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::{AsciiIdentifiers, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- identifiers (ground-truth carets derived from rubocop 1.87.0
    //     column/last_column; leading spaces = column-1, carets =
    //     last_column-column+1). ---

    #[test]
    fn flags_non_ascii_method_definition_name() {
        // rubocop: line 1, col 5..7 (`なまえ`)
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def なまえ
                ^^^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_non_ascii_local_variable() {
        // rubocop: col 1..1 (`δ`)
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            δ = 1
            ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_only_first_non_ascii_run() {
        // `föo_bär`: rubocop flags only the first run `ö` (col 2..2),
        // NOT the later `ä`.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            föo_bär = 1
             ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_emoji_run_only() {
        // `hello_🍣`: rubocop flags the emoji (a single char) at col 7..7.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            hello_🍣 = 1
                  ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_non_ascii_method_call() {
        // `obj.μεθοδος`: method name `μεθοδος` at col 5..11.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            obj.μεθοδος
                ^^^^^^^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_safe_navigation_method_call() {
        // `obj&.μεθοδος`: method name `μεθοδος` at col 6..12 (after `&.`).
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            obj&.μεθοδος
                 ^^^^^^^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_singleton_method_definition() {
        // `def self.μεθοδος`: name `μεθοδος` at col 10..16.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def self.μεθοδος
                     ^^^^^^^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_setter_method_call() {
        // `m.señor = 5`: identifier `señor`, first run `ñ` at col 5..5.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            m = "x"
            m.señor = 5
                ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_setter_definition() {
        // `def señor=(v)`: identifier `señor`, first run `ñ` at col 7..7.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def señor=(v)
                  ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_required_argument() {
        // `def foo(δ)`: arg `δ` at col 9..9.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def foo(δ)
                    ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_splat_argument() {
        // `def foo(*bär)`: arg name `bär` (after `*`), first run `ä` at col 11.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def foo(*bär)
                      ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_double_splat_argument() {
        // `def foo(**bär)`: first run `ä` at col 12.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def foo(**bär)
                       ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_block_argument() {
        // `def foo(&bär)`: first run `ä` at col 11.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def foo(&bär)
                      ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_optional_argument() {
        // `def foo(bär = 1)`: first run `ä` at col 10.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            def foo(bär = 1)
                     ^ Use only ascii symbols in identifiers.
            end
        "#});
    }

    #[test]
    fn flags_block_param_and_read() {
        // `proc { |bär| bär }`: block param at col 10, read at col 15.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            proc { |bär| bär }
                     ^ Use only ascii symbols in identifiers.
                          ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_multiple_assignment_targets() {
        // `föo, bär = 1, 2`: rubocop flags `föo` (col 2) and `bär` (col 7).
        // Both are value-less `lvasgn` targets — must not be filtered out.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            föo, bär = 1, 2
             ^ Use only ascii symbols in identifiers.
                  ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_op_assignment_target() {
        // `bäz += 1`: value-less `lvasgn` target, first run `ä` at col 2.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            bäz += 1
             ^ Use only ascii symbols in identifiers.
        "#});
    }

    #[test]
    fn flags_for_loop_variable() {
        // `for ël in [1]`: loop var is an `lvasgn`, first run `ë` at col 5.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            for ël in [1]
                ^ Use only ascii symbols in identifiers.
              x = 1
            end
        "#});
    }

    // --- constants ---

    #[test]
    fn flags_non_ascii_constant_assignment() {
        // `FOÖ = 1`: constant, first run `Ö` at col 3..3.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            FOÖ = 1
              ^ Use only ascii symbols in constants.
        "#});
    }

    #[test]
    fn flags_non_ascii_class_name() {
        // `class Foö`: constant `Foö`, first run `ö` at col 9..9.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            class Foö
                    ^ Use only ascii symbols in constants.
            end
        "#});
    }

    #[test]
    fn flags_scoped_constant_leaf() {
        // `SOME::Cönst`: leaf constant `Cönst`, first run `ö` at col 8..8.
        // The outer `SOME` const is ASCII and must NOT fire.
        test::<AsciiIdentifiers>().expect_offense(indoc! {r#"
            SOME = Module.new
            SOME::Cönst
                   ^ Use only ascii symbols in constants.
        "#});
    }

    #[test]
    fn respects_ascii_constants_false() {
        // With AsciiConstants: false, constants are not checked.
        test::<AsciiIdentifiers>()
            .with_options(&Options { ascii_constants: false })
            .expect_no_offenses("FOÖ = 1\n");
    }

    #[test]
    fn flags_constant_when_ascii_constants_true() {
        // Default (AsciiConstants: true) still flags the constant.
        test::<AsciiIdentifiers>()
            .with_options(&Options { ascii_constants: true })
            .expect_offense(indoc! {r#"
                FOÖ = 1
                  ^ Use only ascii symbols in constants.
            "#});
    }

    // --- exclusions (verified against rubocop: NOT flagged) ---

    #[test]
    fn ignores_symbol_contents() {
        // `:عرض_gteq` is a symbol; rubocop does NOT flag symbol contents.
        test::<AsciiIdentifiers>().expect_no_offenses(indoc! {r#"
            params = {}
            params[:عرض_gteq]
        "#});
    }

    #[test]
    fn ignores_hash_label() {
        test::<AsciiIdentifiers>().expect_no_offenses("h = { föo: 1 }\n");
    }

    #[test]
    fn ignores_keyword_argument_label() {
        // `bär:` is a kwarg label (tLABEL), not an identifier.
        test::<AsciiIdentifiers>().expect_no_offenses(indoc! {r#"
            def foo(bär: 1)
            end
        "#});
    }

    #[test]
    fn ignores_string_contents() {
        test::<AsciiIdentifiers>().expect_no_offenses(r#"x = "café""#);
    }

    #[test]
    fn ignores_instance_variable() {
        test::<AsciiIdentifiers>().expect_no_offenses("@δ = 1\n");
    }

    #[test]
    fn ignores_class_variable() {
        test::<AsciiIdentifiers>().expect_no_offenses("@@δ = 1\n");
    }

    #[test]
    fn ignores_global_variable() {
        test::<AsciiIdentifiers>().expect_no_offenses("$δ = 1\n");
    }

    #[test]
    fn no_offense_for_ascii_only_code() {
        test::<AsciiIdentifiers>().expect_no_offenses(indoc! {r#"
            def say_hello
              height = 10
              puts height
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(AsciiIdentifiers);
