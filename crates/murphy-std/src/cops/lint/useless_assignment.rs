//! `Lint/UselessAssignment` — flags local-variable writes whose result is
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-ev4p
//! notes: >
//!   Known gaps remain around VariableForce-equivalent coverage and RuboCop parity details.
//! ```
//!
//! never read, *or* whose result is overwritten by a sibling write before
//! any read can observe it.
//!
//! ## Matched shapes
//!
//! - Plain `Lvasgn` (`x = 1`) — name range, RuboCop-compatible message.
//! - `Masgn` (`a, b = 1, 2`) — each LHS target is treated as its own
//!   `Lvasgn`-style write, name range, with RuboCop's underscore guidance.
//! - `OpAsgn` / `OrAsgn` / `AndAsgn` (`x += 1`, `x ||= 1`, `x &&= 1`) —
//!   the variable-name range, with RuboCop's operator guidance.
//!   The value-less `Lvasgn` *target* inside `OpAsgn`/`OrAsgn`/`AndAsgn`
//!   also counts as a read of the variable, so a preceding `x = 0` is
//!   not flagged just because `x += 1` follows.
//! - `Resbody.var` (`rescue => e`) — `e`'s name range.
//!
//! ## Flow-sensitive dataflow (murphy-xek)
//!
//! The flow-sensitive analysis now lives in the shared
//! [`VarSemanticModel`](murphy_plugin_api::var_semantic_model). The cop reads
//! [`cx.var_model()`](murphy_plugin_api::Cx::var_model) and flags every
//! [`Assignment`](murphy_plugin_api::var_semantic_model::Assignment) whose
//! `is_referenced` flag is `false` — i.e. no later read on a compatible
//! control-flow path can observe the write, *or* a dominating overwrite
//! reaches before any such read.
//!
//! The model collects writes (`Lvasgn`, `Masgn` target, `OpAsgn`/`OrAsgn`/
//! `AndAsgn`, `Resbody.var`, `For.var`) and reads (`Lvar`, plus the implicit
//! read inside `OpAsgn`/`OrAsgn`/`AndAsgn`) per scope and computes
//! `is_referenced` via a branch-aware dominance analysis: `If` / `Case` /
//! `When` / `While` / `Until` introduce exclusive-branch barriers, while
//! `Resbody` / `Rescue` / `Ensure` are *not* barriers (exception flow carries
//! partial begin-body writes into the rescue handler). See that module for the
//! barrier-chain details.
//!
//! This cop keeps only the offense-shaping concerns: mapping each
//! unreferenced assignment node back to a `WriteKind` (for the message and
//! autocorrect), the "Did you mean?" suggestion (which additionally scans
//! bare method calls the model does not track), and the autocorrect edits.
//!
//! ### Known v1 limitations
//!
//! * Two writes in different `resbody`s of the same `Rescue` are not
//!   recognised as mutually exclusive (we conservatively treat them as
//!   compatible). The cop will not flag a write in one resbody as
//!   "overwritten" by a write in a sibling resbody.
//! * Loops are treated as a single barrier — we don't unroll. A write
//!   inside a `while` body is in the loop's chunk; we don't reason about
//!   iteration counts.
//!
//! ## Known v1 limitations (Phase 4 — escalate to extend the AST)
//!
//! - `for x in xs`, pattern-match captures (`case ... in [a, b]`), and regexp
//!   implicit named captures (`/(?<name>…)/ =~ str`) are reported without
//!   autocorrect. RuboCop may rewrite some of these to `_`; Murphy keeps the
//!   parity-relevant offense behavior only.
//!
//! ## Autocorrect
//!
//! The cop mirrors RuboCop's safe local rewrites for exposed AST shapes:
//! plain local assignments drop the `name =` prefix, multiple-assignment
//! targets are renamed to `_`, and local `op=` assignments drop the `=`.
//! `||=` / `&&=` and sequential assignments remain report-only because the
//! rewrite can change local-variable declaration semantics or produce
//! invalid Ruby.

use murphy_plugin_api::var_semantic_model::{Assignment, Variable};
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

#[derive(Default)]
pub struct UselessAssignment;

#[cop(
    name = "Lint/UselessAssignment",
    description = "Flag local variable assignments that are never read in the same scope.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessAssignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let Some(model) = cx.var_model() else {
            return;
        };
        for (scope_node, scope) in model.scopes() {
            // Bare method calls (`environment` with no receiver/args) feed the
            // "Did you mean?" suggestion but are not tracked by the model, so
            // collect them with a scope-local walk.
            let candidates = collect_candidates(cx, scope_node);
            for var in scope.variables() {
                for asgn in &var.assignments {
                    if asgn.is_referenced {
                        continue;
                    }
                    let write = make_write(cx, var, asgn);
                    emit_useless_assignment(cx, &write, scope.variables(), &candidates);
                }
            }
        }
    }
}

/// Build the offense-emission `Write` for an unreferenced assignment by
/// inspecting the assignment node's kind. The model stores the *target* node
/// for compound assignments / rescue vars and the assignment node itself for
/// plain `Lvasgn`, so the range and `WriteKind` are derived per shape.
fn make_write(cx: &Cx<'_>, var: &Variable, asgn: &Assignment) -> Write {
    let name = var.name;
    let name_str = cx.symbol_str(name);
    match *cx.kind(asgn.node_id) {
        NodeKind::Lvasgn { value, .. } if value.get().is_some() => Write {
            name,
            end: asgn.end,
            range: assignment_name_range(cx, asgn.node_id, name_str),
            kind: WriteKind::Local { value },
            node: asgn.node_id,
        },
        NodeKind::OpAsgn { target, .. } => Write {
            name,
            end: asgn.end,
            range: cx.range(target),
            kind: WriteKind::Operator {
                op: operator_text(cx, asgn.node_id),
                autocorrect: true,
            },
            node: asgn.node_id,
        },
        NodeKind::OrAsgn { target, .. } => Write {
            name,
            end: asgn.end,
            range: cx.range(target),
            kind: WriteKind::Operator {
                op: "||",
                autocorrect: false,
            },
            node: asgn.node_id,
        },
        NodeKind::AndAsgn { target, .. } => Write {
            name,
            end: asgn.end,
            range: cx.range(target),
            kind: WriteKind::Operator {
                op: "&&",
                autocorrect: false,
            },
            node: asgn.node_id,
        },
        // Value-less `Lvasgn`: either a `Masgn` target or a binding whose
        // source cannot be safely autocorrected (exception, `for`, or regexp
        // named capture). The model records the var node directly, so ancestry
        // disambiguates report-only bindings from multiple-assignment targets.
        NodeKind::Lvasgn { value, .. } if value.get().is_none() => {
            // Walk ancestors, threading through any `Mlhs` wrappers, to decide
            // whether this value-less target is a report-only binding (name
            // range, no autocorrect) or a multiple-assignment target.
            // `for a, b in xs` wraps each target in an `Mlhs` whose parent is
            // the `For`, so the immediate parent alone is not enough. Regexp
            // named captures are lowered under a `Begin` wrapper.
            let is_exception = {
                let mut current = asgn.node_id;
                let mut found = false;
                #[allow(clippy::while_let_loop)]
                loop {
                    let Some(parent) = cx.parent(current).get() else {
                        break;
                    };
                    match *cx.kind(parent) {
                        NodeKind::Resbody { .. } | NodeKind::For { .. } | NodeKind::Begin(_) => {
                            found = true;
                            break;
                        }
                        NodeKind::Mlhs(_) => current = parent,
                        _ => break,
                    }
                }
                found
            };
            let kind = if is_exception {
                WriteKind::Exception
            } else {
                WriteKind::Multiple
            };
            Write {
                name,
                end: asgn.end,
                range: assignment_name_range(cx, asgn.node_id, name_str),
                kind,
                node: asgn.node_id,
            }
        }
        // Defensive fallback: treat anything else as an exception-style write
        // (name range, no autocorrect).
        _ => Write {
            name,
            end: asgn.end,
            range: assignment_name_range(cx, asgn.node_id, name_str),
            kind: WriteKind::Exception,
            node: asgn.node_id,
        },
    }
}

/// Collect bare method-call names (`Send` with no receiver and no args) within
/// the scope rooted at `scope_node`, not descending into nested scopes. These
/// feed the "Did you mean?" suggestion alongside the scope's variables.
fn collect_candidates(cx: &Cx<'_>, scope_node: NodeId) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for id in scope_nodes(cx, scope_node) {
        if let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(id)
            && receiver == OptNodeId::NONE
            && cx.list(args).is_empty()
        {
            candidates.push(Candidate { name: method });
        }
    }
    candidates
}

const MSG_PREFIX: &str = "Useless assignment to variable";

struct Write {
    name: Symbol,
    /// One-past-the-last byte of the assignment. Reads are considered
    /// "after" this point.
    end: u32,
    /// Range to emit the offense on.
    range: Range,
    kind: WriteKind,
    /// Node id used to compute the control-flow barrier chain when
    /// deciding whether two writes are on the same path.
    node: NodeId,
}

#[derive(Clone, Copy)]
enum WriteKind {
    Local { value: OptNodeId },
    Multiple,
    Operator { op: &'static str, autocorrect: bool },
    Exception,
}

struct Candidate {
    name: Symbol,
}

fn emit_useless_assignment(
    cx: &Cx<'_>,
    write: &Write,
    variables: &[Variable],
    candidates: &[Candidate],
) {
    let name = cx.symbol_str(write.name);
    let mut message = format!("{MSG_PREFIX} - `{name}`.");
    match write.kind {
        WriteKind::Multiple => {
            message.push_str(&format!(
                " Use `_` or `_{name}` as a variable name to indicate that it won't be used."
            ));
        }
        WriteKind::Operator { op, .. } => {
            message.push_str(&format!(" Use `{op}` instead of `{op}=`."));
        }
        WriteKind::Local { .. } | WriteKind::Exception => {
            if let Some(similar) = similar_name(cx, name, variables, candidates) {
                message.push_str(&format!(" Did you mean `{similar}`?"));
            }
        }
    }

    cx.emit_offense(write.range, &message, None);
    emit_autocorrect(cx, write);
}

fn emit_autocorrect(cx: &Cx<'_>, write: &Write) {
    match write.kind {
        WriteKind::Local { value } => {
            let Some(value) = value.get() else {
                return;
            };
            if contains_assignment(cx, value) || is_part_of_sequential_assignment(cx, write.node) {
                return;
            }
            cx.emit_edit(
                Range {
                    start: write.range.start,
                    end: cx.range(value).start,
                },
                "",
            );
        }
        WriteKind::Multiple => {
            cx.emit_edit(write.range, "_");
        }
        WriteKind::Operator {
            autocorrect: true, ..
        } => {
            if let Some(eq) = operator_equals_range(cx, write) {
                cx.emit_edit(eq, "");
            }
        }
        WriteKind::Operator {
            autocorrect: false, ..
        }
        | WriteKind::Exception => {}
    }
}

fn is_part_of_sequential_assignment(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        match *cx.kind(parent) {
            NodeKind::Lvasgn { value, .. } if value.get() == Some(current) => return true,
            NodeKind::Array(_) => {
                current = parent;
            }
            _ => return false,
        }
    }
    false
}

fn operator_equals_range(cx: &Cx<'_>, write: &Write) -> Option<Range> {
    let src = cx.source().as_bytes();
    let start = write.range.end as usize;
    let end = write.end as usize;
    let rel = src.get(start..end)?.iter().position(|b| *b == b'=')?;
    let pos = (start + rel) as u32;
    Some(Range {
        start: pos,
        end: pos + 1,
    })
}

fn contains_assignment(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if matches!(
            *cx.kind(id),
            NodeKind::Lvasgn { .. }
                | NodeKind::Masgn { .. }
                | NodeKind::OpAsgn { .. }
                | NodeKind::OrAsgn { .. }
                | NodeKind::AndAsgn { .. }
        ) {
            return true;
        }
        stack.extend(cx.children(id));
    }
    false
}

fn similar_name<'a>(
    cx: &'a Cx<'_>,
    name: &str,
    variables: &'a [Variable],
    candidates: &'a [Candidate],
) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for candidate in variables
        .iter()
        .map(|v| cx.symbol_str(v.name))
        .chain(candidates.iter().map(|c| cx.symbol_str(c.name)))
    {
        if candidate == name
            || candidate.starts_with('_')
            || candidate.contains(name)
            || name.contains(candidate)
        {
            continue;
        }
        let dist = levenshtein(name, candidate);
        if dist <= 2 && best.is_none_or(|(_, best_dist)| dist < best_dist) {
            best = Some((candidate, dist));
        }
    }
    best.map(|(candidate, _)| candidate)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b_chars.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

fn operator_text(cx: &Cx<'_>, node: NodeId) -> &'static str {
    match *cx.kind(node) {
        NodeKind::OpAsgn { op, .. } => match cx.symbol_str(op) {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "%" => "%",
            "**" => "**",
            "&" => "&",
            "|" => "|",
            "^" => "^",
            "<<" => "<<",
            ">>" => ">>",
            _ => "operator",
        },
        NodeKind::OrAsgn { .. } => "||",
        NodeKind::AndAsgn { .. } => "&&",
        _ => "operator",
    }
}

fn scope_nodes(cx: &Cx<'_>, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        out.push(node);
        if node != root && is_scope(cx, node) {
            continue;
        }
        let mut children = cx.children(node);
        children.reverse();
        stack.extend(children);
    }
    out
}

fn is_scope(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Def { .. }
            | NodeKind::Block { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. }
    )
}

/// `Lvasgn` ranges always start at the variable name: Prism emits
/// `[name_start, value_end)` for `x = 1` and `[name_start, name_end)` for
/// the LHS of `x += 1` (the enclosing `OpAsgn` carries the RHS).
fn assignment_name_range(cx: &Cx<'_>, node: NodeId, name: &str) -> Range {
    let start = cx.range(node).start;
    Range {
        start,
        end: start + name.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::UselessAssignment;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_assignments_that_are_never_read() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            used = 1
            unused = 2
            ^^^^^^ Useless assignment to variable - `unused`.
            used
        "#});
    }

    #[test]
    fn ignores_underscore_assignments() {
        test::<UselessAssignment>().expect_no_offenses("名前 = 1\n名前\n_unused = 2\n");
    }

    #[test]
    fn autocorrects_plain_assignment_by_removing_lhs() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                unused = 1
                ^^^^^^ Useless assignment to variable - `unused`.
            "#},
            "1\n",
        );
    }

    #[test]
    fn autocorrects_assignment_inside_call_arguments() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                some_method(unused = 1) do
                            ^^^^^^ Useless assignment to variable - `unused`.
                end
            "#},
            "some_method(1) do\nend\n",
        );
    }

    #[test]
    fn skips_autocorrect_for_sequential_assignment() {
        let run = run_cop_with_edits::<UselessAssignment>("foo = 1, bar = 2\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn suggests_similar_variable_like_names() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            environment = nil
            enviromnent = {}
            ^^^^^^^^^^^ Useless assignment to variable - `enviromnent`. Did you mean `environment`?
            puts environment
        "#});
    }

    #[test]
    fn suggests_similar_bare_method_names() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            enviromnent = {}
            ^^^^^^^^^^^ Useless assignment to variable - `enviromnent`. Did you mean `environment`?
            another_symbol
            puts environment
        "#});
    }

    #[test]
    fn nested_method_read_does_not_satisfy_outer_assignment() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            outer = 1
            ^^^^^ Useless assignment to variable - `outer`.
            def inner
              outer
            end
        "#});
    }

    #[test]
    fn earlier_read_does_not_satisfy_later_assignment() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x
            x = 1
            ^ Useless assignment to variable - `x`.
        "#});
    }

    // murphy-8k4y: cover Masgn / OpAsgn / OrAsgn / AndAsgn / Resbody shapes.

    #[test]
    fn masgn_flags_only_unused_targets() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                a, b = 1, 2
                ^ Useless assignment to variable - `a`. Use `_` or `_a` as a variable name to indicate that it won't be used.
                b
            "#},
            "_, b = 1, 2\nb\n",
        );
    }

    #[test]
    fn masgn_nested_lhs_targets_are_inspected() {
        // `a, (b, c) = [1, [2, 3]]` — the inner `(b, c)` is its own
        // `Mlhs`. `b` is used, `a` and `c` are not. The builder-based
        // caret grammar can't annotate two offsets on one line, so check
        // via `run_cop` directly.
        let offenses = run_cop::<UselessAssignment>("a, (b, c) = [1, [2, 3]]\nb\n");
        let names: Vec<&str> = offenses
            .iter()
            .map(|o| {
                let r = o.range;
                &"a, (b, c) = [1, [2, 3]]\nb\n"[r.start as usize..r.end as usize]
            })
            .collect();
        assert_eq!(
            names,
            vec!["a", "c"],
            "expected only `a` and `c` flagged, got {names:?} (offenses={offenses:?})",
        );
        for offense in &offenses {
            assert!(
                offense
                    .message
                    .starts_with("Useless assignment to variable - `"),
                "unexpected message: {}",
                offense.message
            );
        }
    }

    #[test]
    fn masgn_with_all_used_targets_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                a, b = 1, 2
                a + b
            "#});
    }

    #[test]
    fn op_asgn_flags_only_the_op_asgn_when_result_unused() {
        // `x = 0` is read by `x += 1` (the operator-assignment implicitly
        // reads `x`); only the `x += 1` write is useless.
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                x = 0
                x += 1
                ^ Useless assignment to variable - `x`. Use `+` instead of `+=`.
            "#},
            "x = 0\nx + 1\n",
        );
    }

    #[test]
    fn or_asgn_uses_operator_message() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x ||= 1
            ^ Useless assignment to variable - `x`. Use `||` instead of `||=`.
        "#});
        let run = run_cop_with_edits::<UselessAssignment>("x = 0\nx ||= 1\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn and_asgn_uses_operator_message() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x &&= 1
            ^ Useless assignment to variable - `x`. Use `&&` instead of `&&=`.
        "#});
    }

    #[test]
    fn op_asgn_target_counts_as_read_for_prior_assignment() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 0
                x += 1
                x
            "#});
    }

    #[test]
    fn rescue_var_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              raise
            rescue => e
                      ^ Useless assignment to variable - `e`.
              :rescued
            end
        "#});
    }

    #[test]
    fn rescue_var_used_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  raise
                rescue => e
                  e.message
                end
            "#});
    }

    #[test]
    fn rescue_var_underscore_is_silenced() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  raise
                rescue => _err
                  :rescued
                end
            "#});
    }

    // For-loop iteration variable: now inspected via VarSemanticModel's
    // `For.var` assignment tracking (matches RuboCop). Reported, not
    // autocorrected.

    #[test]
    fn for_loop_variable_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            for i in [1, 2]
                ^ Useless assignment to variable - `i`.
              do_something
            end
        "#});
    }

    #[test]
    fn for_loop_variable_used_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                for i in [1, 2]
                  puts i
                end
            "#});
    }

    #[test]
    fn for_loop_unused_variable_is_not_autocorrected() {
        // RuboCop rewrites the index to `_`; Murphy reports without a fix.
        let run = run_cop_with_edits::<UselessAssignment>("for i in [1, 2]\n  do_something\nend\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn for_destructuring_unused_variable_is_not_autocorrected() {
        // `for a, b in xs` wraps each target in an `Mlhs` whose parent is the
        // `For`. The unused targets must be classified as exception-style
        // writes (no autocorrect), not multiple-assignment targets (which
        // would be rewritten to `_`).
        let run =
            run_cop_with_edits::<UselessAssignment>("for a, b in [[1, 2]]\n  do_something\nend\n");
        assert!(
            !run.offenses.is_empty(),
            "unused for-destructuring targets should be flagged"
        );
        assert_eq!(
            run.edits.len(),
            0,
            "for-destructuring targets must not be autocorrected, got {:?}",
            run.edits
        );
    }

    #[test]
    fn pattern_match_capture_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            case value
            in {name: name}
                      ^^^^ Useless assignment to variable - `name`.
            end
        "#});
    }

    #[test]
    fn pattern_match_capture_used_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                case value
                in {name: name}
                  puts name
                end
            "#});
    }

    #[test]
    fn pattern_match_shorthand_capture_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            case value
            in {name:}
                ^^^^ Useless assignment to variable - `name`.
            end
        "#});
    }

    #[test]
    fn regexp_named_capture_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            /(?<name>foo)/ =~ value
                ^^^^ Useless assignment to variable - `name`.
        "#});
    }

    #[test]
    fn regexp_named_capture_used_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                /(?<name>foo)/ =~ value
                puts name
            "#});
    }

    #[test]
    fn pattern_and_regexp_captures_are_not_autocorrected() {
        for src in [
            "case value\nin {name: name}\nend\n",
            "/(?<name>foo)/ =~ value\n",
        ] {
            let run = run_cop_with_edits::<UselessAssignment>(src);
            assert_eq!(
                run.edits.len(),
                0,
                "captures must not be autocorrected: {src}"
            );
        }
    }

    // murphy-xek: flow-sensitive dataflow — overwrite-before-read +
    // branch-aware barriers.

    #[test]
    fn overwrite_before_read_flags_the_overwritten_write() {
        // `x = 1` is overwritten by `x = 2` before any read.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                ^ Useless assignment to variable - `x`.
                x = 2
                x
            "#});
    }

    #[test]
    fn overwrite_with_no_read_flags_both_writes() {
        let offenses =
            murphy_plugin_api::test_support::run_cop::<UselessAssignment>("x = 1\nx = 2\n");
        assert_eq!(
            offenses.len(),
            2,
            "expected both writes flagged, got {offenses:?}"
        );
    }

    #[test]
    fn conditional_overwrite_does_not_flag_outer_write() {
        // `x = 1` may survive — the `x = 2` is inside an `if`, so on
        // the `cond == false` path the final `x` reads `1`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 1
                if cond
                  x = 2
                end
                x
            "#});
    }

    #[test]
    fn overwrite_inside_same_branch_still_flags() {
        // Both writes live inside the same `if` body — straight-line
        // within the branch, so the first is still overwritten.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to variable - `x`.
                  x = 2
                  x
                end
            "#});
    }

    #[test]
    fn overwrite_separated_by_use_does_not_flag() {
        // `x = 1; foo(x); x = 2` — the call reads `x`, so `x = 1` is
        // used.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                foo(x)
                x = 2
                ^ Useless assignment to variable - `x`.
            "#});
    }

    #[test]
    fn shallower_overwrite_after_branch_dominates_outer_write() {
        // `x = 1` is overwritten by `x = 3` regardless of whether the
        // `if` body runs, because `x = 3` is in a shallower (and thus
        // unconditionally reached) chunk. The `x = 2` inside the `if`
        // is *also* useless for the same reason — `x = 3` dominates it
        // through the shallower prefix of its chain. Fix for PR #70
        // review job 1158 finding 1.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                ^ Useless assignment to variable - `x`.
                if cond
                  x = 2
                  ^ Useless assignment to variable - `x`.
                end
                x = 3
                x
            "#});
    }

    #[test]
    fn exclusive_branches_of_an_if_are_not_overwrites_of_each_other() {
        // `x = 1` and `x = 2` live in different arms of the same `if`,
        // so neither overwrites the other at runtime — they are
        // mutually exclusive. The fix for PR #70 review job 1158
        // finding 2.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                if cond
                  x = 1
                else
                  x = 2
                end
                x
            "#});
    }

    #[test]
    fn read_in_a_sibling_conditional_observes_the_outer_write() {
        // `x = 1` is set when `a` is true; `puts x` runs when `b` is
        // true. The path `a && b` actually observes `x = 1`. The cop
        // must not flag `x = 1` as useless. (PR #70 review job 1163
        // finding 1.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                if a
                  x = 1
                end
                if b
                  puts x
                end
            "#});
    }

    #[test]
    fn begin_body_write_interrupted_by_exception_is_observed_by_rescue() {
        // `may_raise` can throw between `x = 1` and `x = 2`. On the
        // exception path, `rescue` reads x = 1 — the value survives.
        // The cop must not flag x = 1 as overwritten by x = 2.
        // (PR #70 review job 1165.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  x = 1
                  may_raise
                  x = 2
                rescue
                  puts x
                end
            "#});
    }

    #[test]
    fn rescue_handler_read_observes_begin_body_write() {
        // Exception flow carries the partial begin-body write into
        // the rescue handler — `puts x` can see `x = 1`. (PR #70
        // review job 1163 finding 2.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  x = 1
                  raise
                rescue
                  puts x
                end
            "#});
    }

    #[test]
    fn read_inside_a_later_conditional_observes_the_outer_write() {
        // The `puts x` runs when `cond` is true — that path observes
        // the outer `x = 1`, so the write must not be flagged.
        // (PR #70 review job 1162.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 1
                if cond
                  puts x
                end
            "#});
    }

    #[test]
    fn read_in_exclusive_branch_does_not_save_a_write_from_another_branch() {
        // `x = 1` is in the `then` arm; `puts x` is in the `else` arm.
        // They are mutually exclusive at runtime, so the `puts x` does
        // *not* observe `x = 1`. The dominating `x = 2` afterward
        // overwrites `x = 1` unconditionally → flag.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to variable - `x`.
                else
                  puts x
                end
                x = 2
                x
            "#});
    }

    #[test]
    fn op_asgn_overwriting_a_prior_write_does_not_flag_the_prior_write() {
        // Already covered by the murphy-8k4y tests, but pin again under
        // the dataflow framing: `x = 0; x += 1` — only the OpAsgn is
        // useless.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 0
                x += 1
                ^ Useless assignment to variable - `x`. Use `+` instead of `+=`.
            "#});
    }
}
