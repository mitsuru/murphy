//! `Style/ZeroLengthPredicate` — prefer `empty?` over `length == 0` comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ZeroLengthPredicate
//! upstream_version_checked: 1.91.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Inner dispatch on `size`/`length` (`RESTRICT_ON_SEND`) with verbatim
//!   parent matchers (`zero_length_predicate?`, `zero_length_comparison`,
//!   `nonzero_length_comparison` — each RuboCop union arm its own matcher:
//!   top-level `{...}` unions are not expressible in v1), mirroring upstream
//!   `on_send`/`on_csend` (murphy-s1yc.44). `on_csend` covers the predicate
//!   and ZERO-comparison shapes only (upstream omits the nonzero checks), so
//!   `x&.size == 0` (and `< 1`, flipped `0 ==`/`1 >`) flag and correct to
//!   `x&.empty?` per upstream. The cop is marked `Safe: false` in the default
//!   config (unsafe autocorrect) because `empty?` may not be defined in terms
//!   of `length` on all receivers. Murphy emits autocorrect unconditionally
//!   (no unsafe-autocorrect flag in the ABI at time of authoring); the safety
//!   annotation is preserved in default.yml. Non-polymorphic exclusions cover
//!   `File.stat`, `{File,Tempfile,StringIO}` `new`/`open`, and `File::Stat.new`
//!   (all top-level only). Offense messages keep the murphy full-source style
//!   (`x.size == 0`) rather than upstream's bare (`size == 0`).
//! ```
//!
//! ## Matched shapes — EMPTY (`→ empty?`)
//!
//! - `x.size.zero?`   / `x.length.zero?`        (predicate form)
//! - `x.size == 0`    / `0 == x.size`
//! - `x.size < 1`     / `1 > x.size`
//! - `x&.size.zero?` / `x.size&.zero?`         (safe-navigation predicate)
//! - `x&.size == 0`   / `0 == x&.size`          (safe-navigation comparison)
//! - `x&.size < 1`    / `1 > x&.size`
//!
//! ## Matched shapes — NOT-EMPTY (`→ !empty?`, send inners only)
//!
//! - `x.size != 0`    / `0 != x.size`
//! - `x.size > 0`     / `0 < x.size`
//!
//! `x&.size != 0` / `x&.size > 0` still accept (upstream `on_csend` omits the
//! nonzero checks).
//!
//! ## Non-polymorphic exclusions
//!
//! Skips when the receiver of `size`/`length` is any of:
//! - `File.stat(…).size`
//! - `{File,Tempfile,StringIO}.{new,open}(…).size`
//! - `File::Stat.new(…).size`
//!
//! ## Autocorrect
//!
//! - Predicate form: replace `size.zero?` span with `empty?`.
//! - Comparison form: replace whole comparison node with `recv.empty?` or
//!   `!recv.empty?` using whole-node interpolation (structural rewrite).
//!   The dot source is preserved, so `x&.size == 0` → `x&.empty?`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop, def_node_matcher};

// RuboCop parity: `Style/ZeroLengthPredicate` `non_polymorphic_collection?`
// const heads are `(const {nil? cbase} :File)` for `stat`,
// `(const {nil? cbase} {:File :Tempfile :StringIO})` for `new`/`open`, and
// `(const (const {nil? cbase} :File) :Stat)` for `new` (top-level only).
// In Murphy `::File` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (both excluded, pinned by `boundary_accepts_cbase_file_stat_size`).
// Namespaced `Foo::File` / `Foo::File::Stat` still flag (pinned by
// `boundary_flags_namespaced_file_stat_size` +
// `s1yc44_flags_namespaced_file_stat_new_size`). Predicate-only, no captures,
// byte-identical offense emission.
def_node_matcher!(is_file_const_matcher, "(const nil? :File)");
def_node_matcher!(
    is_new_open_const_matcher,
    "(const nil? {:File :Tempfile :StringIO})"
);
def_node_matcher!(
    is_file_stat_const_matcher,
    "(const (const nil? :File) :Stat)"
);

// Verbatim port of the inner dispatch head (murphy-s1yc.44): upstream
// `RESTRICT_ON_SEND = %i[size length]` fires `on_send`/`on_csend` on the
// INNER call, so `(call _ {:length :size})` matches `size`/`length` on either
// send or csend (safe-navigation) with any receiver (absent or present per
// murphy-if9y). The `_` receiver also binds bare `size`, so the
// present-receiver + zero-args guards stay explicit below to mirror upstream
// inner `(call (...) {:length :size})` (pinned by
// `s1yc44_accepts_bare_size_zero_predicate` +
// `s1yc44_accepts_size_with_args_eq_zero`).
def_node_matcher!(zero_length_inner_call, "(call _ {:length :size})");

// Verbatim ports of the parent matchers (murphy-s1yc.44), evaluated against
// `node.parent` (`call` = `{send csend}` covers safe-navigation outers):
// - `zero_length_predicate?`: `(call (call (...) {:length :size}) :zero?)`
// - `zero_length_comparison`: the four `== 0` / `< 1` arms (upstream
//   `$`-captures dropped — predicate-only, message/replacement stay
//   hand-rolled below — with `(int N)` as the bare literal `N`)
// - `nonzero_length_comparison`: the two `!= 0` / `> 0` arms, ditto.
// A top-level `{...}` union is not expressible in v1 (a node pattern with a
// variable-length child list — i.e. any `(call ...)` — is rejected inside
// `{}`), so each RuboCop union arm becomes its own matcher below — each arm
// verbatim. The outer takes no trailing `...`, so `zero?`-with-args no
// longer flags (upstream parity fix, pinned by
// `s1yc44_accepts_zero_predicate_with_args`).
def_node_matcher!(
    zero_length_predicate_matcher,
    "(call (call _ {:length :size}) :zero?)"
);
def_node_matcher!(
    zero_length_eq_matcher,
    "(call (call _ {:length :size}) :== 0)"
);
def_node_matcher!(
    zero_length_eq_flipped_matcher,
    "(call 0 :== (call _ {:length :size}))"
);
def_node_matcher!(
    zero_length_lt_one_matcher,
    "(call (call _ {:length :size}) :< 1)"
);
def_node_matcher!(
    zero_length_one_gt_matcher,
    "(call 1 :> (call _ {:length :size}))"
);
def_node_matcher!(
    nonzero_length_cmp_matcher,
    "(call (call _ {:length :size}) {:> :!=} 0)"
);
def_node_matcher!(
    nonzero_length_cmp_flipped_matcher,
    "(call 0 {:< :!=} (call _ {:length :size}))"
);

/// Upstream `zero_length_comparison(node.parent)`: any of the four
/// `== 0` / `< 1` arms.
fn is_zero_length_comparison(parent: NodeId, cx: &Cx<'_>) -> bool {
    zero_length_eq_matcher(parent, cx)
        || zero_length_eq_flipped_matcher(parent, cx)
        || zero_length_lt_one_matcher(parent, cx)
        || zero_length_one_gt_matcher(parent, cx)
}

/// Upstream `nonzero_length_comparison(node.parent)`: either `!= 0` / `> 0`
/// arm.
fn is_nonzero_length_comparison(parent: NodeId, cx: &Cx<'_>) -> bool {
    nonzero_length_cmp_matcher(parent, cx) || nonzero_length_cmp_flipped_matcher(parent, cx)
}

/// Stateless unit struct.
#[derive(Default)]
pub struct ZeroLengthPredicate;

const ZERO_MSG: &str = "Use `empty?` instead of `%s`.";
const NONZERO_MSG: &str = "Use `!empty?` instead of `%s`.";

#[cop(
    name = "Style/ZeroLengthPredicate",
    description = "Use #empty? when testing for objects of length 0.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ZeroLengthPredicate {
    /// Inner `size`/`length` send: mirrors upstream `on_send` with
    /// `RESTRICT_ON_SEND = %i[size length]` — checks the predicate,
    /// zero-comparison, and nonzero-comparison parent matchers.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_inner(node, cx, true);
    }

    /// Inner `size`/`length` csend: mirrors upstream `on_csend` — checks the
    /// predicate and ZERO-comparison parent matchers only (upstream omits
    /// the nonzero checks, so `x&.size != 0` / `x&.size > 0` still accept).
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_inner(node, cx, false);
    }
}

/// Inner-dispatch check shared by the send/csend handlers: `node` is the
/// `size`/`length` call, the offense (if any) is on `node.parent`.
fn check_inner(node: NodeId, cx: &Cx<'_>, is_send: bool) {
    // Verbatim `(call _ {:length :size})` head: filters to the
    // `RESTRICT_ON_SEND` methods on either send or csend. Without this, an
    // unrelated call (e.g. `x.count == 0`) would run the parent checks below
    // instead of being rejected up front.
    if !zero_length_inner_call(node, cx) {
        return;
    }

    // Upstream inner `(call (...) {:length :size})`: present receiver, no
    // args. The head's `_` also binds bare `size`, so this stays explicit.
    let Some(length_recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !cx.call_arguments(node).is_empty() {
        return;
    }

    let Some(parent) = cx.parent(node).get() else {
        return;
    };

    if zero_length_predicate_matcher(parent, cx) {
        if is_non_polymorphic(length_recv, cx) {
            return;
        }
        emit_predicate_offense(node, parent, cx);
    } else if is_zero_length_comparison(parent, cx) {
        if is_non_polymorphic(length_recv, cx) {
            return;
        }
        emit_comparison_offense(node, parent, cx, true);
    } else if is_send && is_nonzero_length_comparison(parent, cx) {
        if is_non_polymorphic(length_recv, cx) {
            return;
        }
        emit_comparison_offense(node, parent, cx, false);
    }
}

// --------------------------------------------------------------------------
// Predicate form: `x.size.zero?` / `x.length.zero?`
// --------------------------------------------------------------------------

/// `inner` is the `size`/`length` call, `parent` the matched `zero?` outer.
/// Offense range: from start of inner method name (e.g. `size`) to end of
/// parent (e.g. end of `zero?`). Mirrors RuboCop's
/// `node.loc.selector.join(node.parent.source_range.end)`.
fn emit_predicate_offense(inner: NodeId, parent: NodeId, cx: &Cx<'_>) {
    let inner_name_start = cx.node(inner).loc.name.start;
    let parent_end = cx.range(parent).end;
    let offense_range = Range {
        start: inner_name_start,
        end: parent_end,
    };
    let offense_src = cx.raw_source(offense_range);
    let message = ZERO_MSG.replace("%s", offense_src);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: replace `size.zero?` span with `empty?`.
    cx.emit_edit(offense_range, "empty?");
}

// --------------------------------------------------------------------------
// Comparison forms
// --------------------------------------------------------------------------

/// `inner` is the `size`/`length` call, `parent` the matched comparison
/// outer, `is_zero` selects the `empty?` / `!empty?` direction.
fn emit_comparison_offense(inner: NodeId, parent: NodeId, cx: &Cx<'_>, is_zero: bool) {
    let Some(lhs_id) = cx.call_receiver(parent).get() else {
        return;
    };
    let args = cx.call_arguments(parent);
    let Some(&rhs_id) = args.first() else {
        return;
    };
    let Some(op) = cx.method_name(parent) else {
        return;
    };

    // Message: "<lhs> <op> <rhs>".
    let lhs_src = cx.raw_source(cx.range(lhs_id));
    let rhs_src = cx.raw_source(cx.range(rhs_id));
    let current = format!("{} {} {}", lhs_src, op, rhs_src);
    let message = if is_zero {
        ZERO_MSG.replace("%s", &current)
    } else {
        NONZERO_MSG.replace("%s", &current)
    };

    let outer_range = cx.range(parent);
    cx.emit_offense(outer_range, &message, None);

    // Autocorrect: `recv<dot>empty?` or `!recv<dot>empty?`. The dot source
    // preserves safe-navigation (`x&.size == 0` → `x&.empty?` per upstream).
    let length_recv = cx.call_receiver(inner).get().expect("guarded present");
    let dot_src = cx.raw_source(cx.loc(inner).dot());
    let recv_src = cx.raw_source(cx.range(length_recv));
    let replacement = if is_zero {
        format!("{}{}empty?", recv_src, dot_src)
    } else {
        format!("!{}{}empty?", recv_src, dot_src)
    };
    cx.emit_edit(outer_range, &replacement);
}

// --------------------------------------------------------------------------
// Non-polymorphic exclusion
// --------------------------------------------------------------------------

fn is_non_polymorphic(recv: NodeId, cx: &Cx<'_>) -> bool {
    let Some(const_id) = cx.call_receiver(recv).get() else {
        return false;
    };
    let Some(method_name) = cx.method_name(recv) else {
        return false;
    };

    // `(const nil? :File)` — top-level only (`::` collapses to
    // `Const{scope:None}`). Namespaced still flags.
    if method_name == "stat" && is_file_const_matcher(const_id, cx) {
        return true;
    }
    // `(const nil? {:File :Tempfile :StringIO})` — top-level only.
    if matches!(method_name, "new" | "open") && is_new_open_const_matcher(const_id, cx) {
        return true;
    }
    // `(const (const nil? :File) :Stat)` — `File::Stat.new`, top-level only.
    if method_name == "new" && is_file_stat_const_matcher(const_id, cx) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ZeroLengthPredicate;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Predicate form: size.zero? / length.zero? → empty? -----

    #[test]
    fn flags_size_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x.size.zero?
                  ^^^^^^^^^^ Use `empty?` instead of `size.zero?`.
            "},
            "x.empty?\n",
        );
    }

    #[test]
    fn flags_length_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x.length.zero?
                  ^^^^^^^^^^^^ Use `empty?` instead of `length.zero?`.
            "},
            "x.empty?\n",
        );
    }

    // ----- Comparison forms: → empty? -----

    #[test]
    fn flags_length_eq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {r#"
                [1, 2, 3].length == 0
                ^^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `[1, 2, 3].length == 0`.
            "#},
            "[1, 2, 3].empty?\n",
        );
    }

    #[test]
    fn flags_zero_eq_length() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {r#"
                0 == "foobar".length
                ^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `0 == "foobar".length`.
            "#},
            "\"foobar\".empty?\n",
        );
    }

    #[test]
    fn flags_size_eq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                hash.size == 0
                ^^^^^^^^^^^^^^ Use `empty?` instead of `hash.size == 0`.
            "},
            "hash.empty?\n",
        );
    }

    #[test]
    fn flags_length_lt_one() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                array.length < 1
                ^^^^^^^^^^^^^^^^ Use `empty?` instead of `array.length < 1`.
            "},
            "array.empty?\n",
        );
    }

    #[test]
    fn flags_one_gt_length() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                1 > array.length
                ^^^^^^^^^^^^^^^^ Use `empty?` instead of `1 > array.length`.
            "},
            "array.empty?\n",
        );
    }

    // ----- Comparison forms: → !empty? -----

    #[test]
    fn flags_length_neq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                {a: 1, b: 2}.length != 0
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `!empty?` instead of `{a: 1, b: 2}.length != 0`.
            "},
            "!{a: 1, b: 2}.empty?\n",
        );
    }

    #[test]
    fn flags_length_gt_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                string.length > 0
                ^^^^^^^^^^^^^^^^^ Use `!empty?` instead of `string.length > 0`.
            "},
            "!string.empty?\n",
        );
    }

    #[test]
    fn flags_size_gt_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                hash.size > 0
                ^^^^^^^^^^^^^ Use `!empty?` instead of `hash.size > 0`.
            "},
            "!hash.empty?\n",
        );
    }

    #[test]
    fn flags_zero_lt_size() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                0 < hash.size
                ^^^^^^^^^^^^^ Use `!empty?` instead of `0 < hash.size`.
            "},
            "!hash.empty?\n",
        );
    }

    #[test]
    fn flags_zero_neq_size() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                0 != string.size
                ^^^^^^^^^^^^^^^^ Use `!empty?` instead of `0 != string.size`.
            "},
            "!string.empty?\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_empty_predicate() {
        test::<ZeroLengthPredicate>().expect_no_offenses("[1, 2, 3].empty?\n");
    }

    #[test]
    fn accepts_non_zero_comparison() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x.size == 1\n");
    }

    #[test]
    fn accepts_non_size_method() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x.count_words == 0\n");
    }

    // ----- Non-polymorphic exclusions -----

    #[test]
    fn accepts_file_stat_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.stat(f).size == 0\n");
    }

    #[test]
    fn accepts_file_new_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.new(f).size == 0\n");
    }

    #[test]
    fn accepts_stringio_new_length() {
        test::<ZeroLengthPredicate>().expect_no_offenses("StringIO.new(f).length == 0\n");
    }

    #[test]
    fn accepts_tempfile_open_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("Tempfile.open(f).size == 0\n");
    }

    #[test]
    fn accepts_file_open_size_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.open(f).size.zero?\n");
    }

    // ----- Safe-navigation (csend) inner forms -----
    // murphy-s1yc.44 design decision: mirror upstream `on_csend`, which
    // checks the predicate and ZERO-comparison parent matchers (but NOT the
    // nonzero checks). So `x&.size == 0` (and `< 1`, flipped `0 ==`/`1 >`)
    // flag and correct to `x&.empty?` per upstream rubocop 1.91.0 (verified:
    // `x&.size == 0` → `x&.empty?`), while `x&.size != 0` / `x&.size > 0`
    // still accept. The pre-port nil-semantics deviation
    // (`accepts_csend_size_eq_zero`) is retired by this decision.

    #[test]
    fn flags_csend_size_eq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x&.size == 0
                ^^^^^^^^^^^^ Use `empty?` instead of `x&.size == 0`.
            "},
            "x&.empty?\n",
        );
    }

    #[test]
    fn accepts_csend_size_gt_zero() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x&.size > 0\n");
    }

    #[test]
    fn s1yc44_accepts_csend_size_neq_zero() {
        // Nonzero direction on csend: upstream `on_csend` omits the nonzero
        // checks (verified: `x&.size != 0` clean under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_no_offenses("x&.size != 0\n");
    }

    #[test]
    fn s1yc44_flags_csend_size_zero_predicate() {
        // Predicate form on csend inner (verified: `x&.size.zero?` flags
        // "size.zero?", corrects to `x&.empty?`).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x&.size.zero?
                   ^^^^^^^^^^ Use `empty?` instead of `size.zero?`.
            "},
            "x&.empty?\n",
        );
    }

    #[test]
    fn s1yc44_flags_send_size_csend_zero_predicate() {
        // Send inner with csend outer predicate (verified: `x.size&.zero?`
        // flags "size&.zero?", corrects to `x.empty?`).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x.size&.zero?
                  ^^^^^^^^^^^ Use `empty?` instead of `size&.zero?`.
            "},
            "x.empty?\n",
        );
    }

    // --- Boundary characterization (murphy-ft88.27): pin the exact node set
    // the hand-rolled `is_non_polymorphic` (`is_global_const(File)` for
    // `stat`, `is_global_const(File/Tempfile/StringIO)` for `new`/`open`)
    // matches, so the verbatim const-inner refactor can be proven equivalent.
    // `::File` collapses to `Const{scope:None}`: `nil?` covers bare + `::`
    // (both excluded, pinned by `boundary_accepts_cbase_file_stat_size`).
    // Namespaced `Foo::File` still flags (not excluded, pinned by
    // `boundary_flags_namespaced_file_stat_size`). Upstream's third arm
    // (`File::Stat.new`) is NOT implemented in Murphy and stays deferred.

    #[test]
    fn boundary_accepts_cbase_file_stat_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("::File.stat(f).size == 0\n");
    }

    #[test]
    fn boundary_flags_namespaced_file_stat_size() {
        test::<ZeroLengthPredicate>().expect_offense(indoc! {r#"
            Foo::File.stat(f).size == 0
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `Foo::File.stat(f).size == 0`.
        "#});
    }

    #[test]
    fn boundary_accepts_cbase_tempfile_open_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("::Tempfile.open(f).size == 0\n");
    }

    #[test]
    fn boundary_flags_namespaced_tempfile_open_size() {
        test::<ZeroLengthPredicate>().expect_offense(indoc! {r#"
            Foo::Tempfile.open(f).size == 0
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `Foo::Tempfile.open(f).size == 0`.
        "#});
    }

    #[test]
    fn boundary_accepts_cbase_stringio_new_length() {
        test::<ZeroLengthPredicate>().expect_no_offenses("::StringIO.new(f).length == 0\n");
    }

    // --- Inner-dispatch boundary pins (murphy-s1yc.44): the upstream
    // inner `(call (...) {:length :size})` requires a present receiver and
    // zero args, and the parent matchers take no trailing `...`, so bare
    // `size`, `size`-with-args, and `zero?`-with-args all accept (the last is
    // an upstream parity fix: the pre-port outer dispatch flagged
    // `x.size.zero?(1)`). All verified against rubocop 1.91.0.

    #[test]
    fn s1yc44_accepts_bare_size_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_no_offenses("size.zero?\n");
    }

    #[test]
    fn s1yc44_accepts_size_with_args_eq_zero() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x.size(1) == 0\n");
    }

    #[test]
    fn s1yc44_accepts_zero_predicate_with_args() {
        // `zero?`-with-args: the parent matcher `(call ... :zero?)` takes no
        // args, so the inner dispatch no longer flags (upstream parity fix;
        // verified: `x.size.zero?(1)` clean under rubocop 1.91.0 — only the
        // six documented shapes flag).
        test::<ZeroLengthPredicate>().expect_no_offenses("x.size.zero?(1)\n");
    }

    #[test]
    fn s1yc44_accepts_file_stat_new_size_eq_zero() {
        // Third non-polymorphic arm `(const (const nil? :File) :Stat)` `:new`
        // (verified: `File::Stat.new(f).size == 0` clean under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_no_offenses("File::Stat.new(f).size == 0\n");
    }

    #[test]
    fn s1yc44_accepts_file_stat_new_size_zero_predicate() {
        // Predicate form of the third arm (verified:
        // `File::Stat.new(f).size.zero?` clean under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_no_offenses("File::Stat.new(f).size.zero?\n");
    }

    #[test]
    fn s1yc44_accepts_cbase_file_stat_new_size() {
        // `::File::Stat` collapses like `::File` (`Const{scope:None}`), so
        // `nil?` covers it (verified: `::File::Stat.new(f).size == 0` clean
        // under rubocop 1.91.0 via `{nil? cbase}`).
        test::<ZeroLengthPredicate>().expect_no_offenses("::File::Stat.new(f).size == 0\n");
    }

    #[test]
    fn s1yc44_flags_namespaced_file_stat_new_size() {
        // Namespaced inner const still flags (verified:
        // `Foo::File::Stat.new(f).size == 0` flags, corrects to
        // `Foo::File::Stat.new(f).empty?` under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                Foo::File::Stat.new(f).size == 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `Foo::File::Stat.new(f).size == 0`.
            "},
            "Foo::File::Stat.new(f).empty?\n",
        );
    }

    #[test]
    fn s1yc44_flags_csend_flipped_zero_eq() {
        // Flipped csend ZERO-comparison (verified: `0 == x&.size` flags
        // "0 == size", corrects to `x&.empty?` under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                0 == x&.size
                ^^^^^^^^^^^^ Use `empty?` instead of `0 == x&.size`.
            "},
            "x&.empty?\n",
        );
    }

    #[test]
    fn s1yc44_flags_csend_size_lt_one() {
        // csend `x&.size < 1` (verified: flags "size < 1", corrects to
        // `x&.empty?` under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x&.size < 1
                ^^^^^^^^^^^ Use `empty?` instead of `x&.size < 1`.
            "},
            "x&.empty?\n",
        );
    }

    #[test]
    fn s1yc44_flags_csend_one_gt_size() {
        // Flipped csend `1 > x&.size` (verified: flags "1 > size", corrects
        // to `x&.empty?` under rubocop 1.91.0).
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                1 > x&.size
                ^^^^^^^^^^^ Use `empty?` instead of `1 > x&.size`.
            "},
            "x&.empty?\n",
        );
    }
}
murphy_plugin_api::submit_cop!(ZeroLengthPredicate);
