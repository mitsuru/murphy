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
//!   - murphy-4845
//! notes: >
//!   Known gaps remain around VariableForce-equivalent coverage and RuboCop parity details.
//!   murphy-4845: scope boundary gaps in is_scope() fixed — Defs/Numblock/Itblock now included.
//!   Regexp named captures are represented as MatchWithLvasgn containing value-less
//!   Lvasgn targets lowered from Prism's MatchWriteNode. The construct is tracked
//!   via the existing Lvasgn arm in VarSemanticModel.
//!   Block-closure reads of outer locals are counted as references to the outer
//!   assignment, matching RuboCop's treatment of closures such as `tap`.
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
//! `When` / `While` / `Until` introduce exclusive-branch barriers. `Rescue` is
//! an *asymmetric* barrier: its arms (begin `body` / each `Resbody` / `else`)
//! are mutually exclusive for domination, but the begin `body` stays
//! read-compatible with every sibling arm because exception flow carries a
//! partial begin-body write into the rescue / else / fall-through paths.
//! `Resbody` / `Ensure` are themselves *not* barriers. See that module for the
//! barrier-chain details.
//!
//! This cop keeps only the offense-shaping concerns: mapping each
//! unreferenced assignment node back to a `WriteKind` (for the message and
//! autocorrect), the "Did you mean?" suggestion (which additionally scans
//! bare method calls the model does not track), and the autocorrect edits.
//!
//! ### Known v1 limitations
//!
//! * Writes in different `resbody`s of the same `Rescue` (and a `resbody`
//!   vs. the `else`) are mutually exclusive, so a write in one resbody is
//!   never reported as "overwritten" by a write in a sibling resbody.
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
            // named captures are lowered under `MatchWithLvasgn`.
            let is_exception = {
                let mut current = asgn.node_id;
                let mut found = false;
                #[allow(clippy::while_let_loop)]
                loop {
                    let Some(parent) = cx.parent(current).get() else {
                        break;
                    };
                    match *cx.kind(parent) {
                        NodeKind::Resbody { .. }
                        | NodeKind::For { .. }
                        | NodeKind::MatchWithLvasgn { .. } => {
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
            | NodeKind::Defs { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
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
    fn block_closure_read_counts_as_reference_to_outer_assignment() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            first_page = build_page

            presenter.tap do |item|
              item.first = first_page
            end
        "#});
    }

    #[test]
    fn block_closure_assignment_to_outer_local_counts_for_later_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            backup = nil

            with_lock do
              backup = create_backup!
            end

            perform_async(backup.id)
        "#});
    }

    #[test]
    fn keyword_shorthand_counts_as_local_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            key = build_key
            sign!(response, key:, components: %w(@status content-digest))
        "#});
    }

    #[test]
    fn assignment_used_as_if_condition_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            if (supported_locale = SUPPORTED_LOCALES[locale.to_sym])
              supported_locale[1]
            else
              locale
            end
        "#});
    }

    #[test]
    fn assignment_read_inside_hash_value_ternary_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            module Admin::FilterHelper
              def filter_link_to(text, link_to_params, link_class_params = link_to_params)
                new_url = filtered_url_for(link_to_params)
                new_class = filtered_url_for(link_class_params)
                is_selected = selected?(link_class_params)

                link_to text, new_url, class: filter_link_class(new_class), 'aria-current': (is_selected ? 'true' : nil)
              end
            end
        "#});
    }

    #[test]
    fn class_method_assignment_condition_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            private_class_method def self.locale_name_for_sorting(locale)
              if (supported_locale = SUPPORTED_LOCALES[locale.to_sym])
                ASCIIFolding.new.fold(supported_locale[1]).downcase
              elsif (regional_locale = REGIONAL_LOCALE_NAMES[locale.to_sym])
                ASCIIFolding.new.fold(regional_locale).downcase
              else
                locale
              end
            end
        "#});
    }

    #[test]
    fn retry_counter_read_in_rescue_condition_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            retries = 0

            begin
              connect
            rescue NetworkError
              retries += 1

              if retries < MAX_RETRY
                retry
              end
            end
        "#});
    }

    #[test]
    fn or_assignment_result_used_after_block_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            error = nil

            accounts.each do |account|
              follow(account)
            rescue NotPermitted => e
              error ||= e
            end

            raise error if error.present?
        "#});
    }

    #[test]
    fn multiple_assignment_value_read_later_counts_as_read() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            _, pending, processed, async_refresh_key, threshold = redis.multi do |pipeline|
              pipeline.hget(key, 'threshold')
            end

            async_refresh = AsyncRefresh.new(async_refresh_key) if async_refresh_key.present?

            if pending.zero? || processed >= (threshold || 1.0).to_f * (processed + pending)
              async_refresh&.finish!
              cleanup
            end
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

    // murphy-4845: scope boundary gaps — Defs/Numblock/Itblock/Lambda must
    // stop candidate collection from crossing into nested scopes.

    #[test]
    fn singleton_method_bare_call_does_not_leak_as_candidate() {
        // `environment` inside `def self.foo` must not generate a
        // "Did you mean?" suggestion for `enviromnent` in the outer scope.
        // Pre-fix: missing `Defs` in is_scope() would cause the candidate to leak.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            enviromnent = {}
            ^^^^^^^^^^^ Useless assignment to variable - `enviromnent`.
            def self.foo
              environment
            end
        "#});
    }

    #[test]
    fn numblock_bare_call_does_not_leak_as_candidate() {
        // A bare no-arg call inside a numbered-parameter block must not leak
        // as a "Did you mean?" candidate into the outer scope.
        // Pre-fix: missing `Numblock` in is_scope() would cause the leak.
        // Note: `{ environment; _1 }` creates a Numblock (not a plain Block)
        // because `_1` is a numbered parameter.
        let offenses = run_cop::<UselessAssignment>(indoc! {r#"
            enviromnent = {}
            result = [1].map { environment; _1 }
            puts result
        "#});
        // Only `enviromnent` is flagged (result is read by puts).
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert!(
            !offenses[0].message.contains("Did you mean"),
            "numblock-internal `environment` must not leak as candidate: {}",
            offenses[0].message
        );
    }

    #[test]
    fn lambda_bare_call_does_not_leak_as_candidate() {
        // `environment` inside a lambda must not leak as a "Did you mean?"
        // suggestion into the outer scope.
        // Pre-fix: missing `Lambda` in is_scope() would generate
        // "Did you mean `environment`?" for `enviromnent`.
        let offenses = run_cop::<UselessAssignment>(indoc! {r#"
            enviromnent = {}
            lam = -> { environment }
            puts lam
        "#});
        // Only `enviromnent` is useless (lam is read by puts).
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert!(
            !offenses[0].message.contains("Did you mean"),
            "lambda-internal `environment` must not leak as candidate: {}",
            offenses[0].message
        );
    }

    #[test]
    fn rescue_alt_branch_does_not_overwrite_begin_body_write() {
        // request.rb `encoding`: begin-body write and rescue write are
        // mutually exclusive; the begin-body value reaches the read on the
        // no-exception path. RuboCop 1.87 reports nothing.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            if charset.nil?
              encoding = Encoding::BINARY
            else
              begin
                encoding = Encoding.find(charset)
              rescue ArgumentError
                encoding = Encoding::BINARY
              end
            end
            String.new(encoding: encoding)
        "#});
    }

    #[test]
    fn rescue_alt_branch_does_not_overwrite_pre_begin_write() {
        // request.rb `addresses`: a pre-begin init plus a begin-body write,
        // both with a rescue alternative. None are useless per RuboCop 1.87.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            addresses = []
            begin
              addresses = [resolve(host)]
            rescue StandardError
              addresses = lookup(host)
              addresses = addresses.first(2)
            end
            addresses.each { |a| p a }
        "#});
    }

    #[test]
    fn rescue_alt_branch_nested_in_conditional_not_flagged() {
        // process_mentions_service.rb `mentioned_account`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            mentioned_account = find_remote(username)
            if undeliverable?(mentioned_account)
              begin
                mentioned_account = resolve(match)
              rescue Error
                mentioned_account = nil
              end
            end
            use(mentioned_account)
        "#});
    }

    #[test]
    fn multi_statement_begin_body_write_observed_by_rescue() {
        // CANARY for body-arm identity: with a MULTI-statement begin body the
        // body wraps in a `(begin ...)` stmt-list node, so the arm recorded in
        // barrier_chain must still `==` Rescue.body. `value` is only read in
        // the handler via exception flow.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              setup
              value = compute
            rescue
              log(value)
            end
        "#});
    }

    #[test]
    fn genuinely_unused_rescue_handler_write_still_flagged() {
        // Guard against over-suppression: a never-read write inside a resbody
        // is still useless (RuboCop flags it too).
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              work
            rescue
              x = 1
              ^ Useless assignment to variable - `x`.
            end
        "#});
    }

    // Module-doc promise (Known v1 limitations): writes in different
    // `resbody`s of the same `Rescue` are mutually exclusive — a write in
    // one resbody is never reported as "overwritten" by a sibling resbody.
    // Verified against standalone RuboCop 1.87.0.

    #[test]
    fn sibling_resbody_writes_both_observed_by_later_read() {
        // Two resbody writes are mutually exclusive (neither dominates the
        // other), and both reach the post-block read. RuboCop 1.87 reports
        // nothing — the sibling resbody must NOT be treated as an overwrite.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              work
            rescue A
              result = 1
            rescue B
              result = 2
            end
            use(result)
        "#});
    }

    #[test]
    fn sibling_resbody_writes_dominated_by_post_block_write_are_flagged() {
        // A post-block write (`result = 3`) dominates the read regardless of
        // which rescue arm ran, so both resbody writes are dead. The sibling
        // resbody exclusivity must not suppress this — the dominating write
        // outside the begin/rescue still makes both arms useless.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              work
            rescue A
              result = 1
              ^^^^^^ Useless assignment to variable - `result`.
            rescue B
              result = 2
              ^^^^^^ Useless assignment to variable - `result`.
            end
            result = 3
            use(result)
        "#});
    }

    #[test]
    fn ensure_write_dominates_body_and_resbody_writes() {
        // The ensure write runs on every path, so it overwrites both the
        // begin-body write and the resbody write before the read. Both are
        // dead; the ensure write itself is read afterward and is clean.
        // Pins the Phase A ensure-interaction dependence.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              total = 1
              ^^^^^ Useless assignment to variable - `total`.
            rescue
              total = 2
              ^^^^^ Useless assignment to variable - `total`.
            ensure
              total = 3
            end
            use(total)
        "#});
    }

    #[test]
    fn begin_body_write_dominated_after_rescue_still_flagged() {
        // Guard the begin-body arm against over-relaxation: a begin-body
        // write that is overwritten after the whole begin/rescue and never
        // read in any arm is still dead. RuboCop 1.87 flags `count = 1`.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              count = 1
              ^^^^^ Useless assignment to variable - `count`.
            rescue
              handle
            end
            count = 2
            use(count)
        "#});
    }

    #[test]
    fn begin_body_write_killed_by_else_and_every_resbody_flagged() {
        // Distributed kill: the no-exception path overwrites via `else`, the
        // exception path via the (sole) `resbody`. Every exit of the rescue
        // overwrites `x` before the trailing read, so `x = 1` is useless.
        // RuboCop 1.87 flags it.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              x = 1
              ^ Useless assignment to variable - `x`.
            rescue
              x = 3
            else
              x = 2
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_with_non_overwriting_rescue_not_flagged() {
        // FP guard: the `rescue` arm does NOT overwrite `x`, so on the
        // exception path the begin-body value reaches the read. RuboCop 1.87
        // reports nothing — the distributed kill must require *every* resbody
        // to overwrite.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue
              log(error)
            else
              x = 2
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_without_else_arm_not_flagged() {
        // FP guard: no `else` arm ⇒ the no-exception path falls through with
        // the begin-body value intact ⇒ it is read. RuboCop 1.87 reports
        // nothing.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue
              x = 3
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_with_conditional_else_overwrite_not_flagged() {
        // FP guard: the `else`-arm overwrite is itself conditional, so the
        // no-exception path may leave the begin-body value intact. The
        // distributed kill must require an *unconditional* else overwrite.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue
              x = 3
            else
              x = 2 if foo
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_with_short_circuited_else_overwrite_not_flagged() {
        // FP guard: the `else`-arm overwrite is guarded by a short-circuit
        // `&&`, so the no-exception path with `cond` false leaves the
        // begin-body value intact and reads it. The distributed kill must not
        // treat a short-circuited (`&&`/`||`) write as a guaranteed overwrite.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue
              x = 3
            else
              cond && (x = 2)
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_with_one_silent_resbody_not_flagged() {
        // FP guard: one of two `resbody` arms does not overwrite `x`, leaving
        // the begin-body value observable on that exception path. RuboCop 1.87
        // reports nothing.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue A
              x = 3
            rescue B
              log(error)
            else
              x = 2
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_killed_by_later_body_write_without_else_flagged() {
        // Distributed kill with no `else`: the no-exception path is covered by a
        // later *unconditional begin-body* write (`x = 2`), the exception path by
        // the `resbody` (`x = 3`). Every exit overwrites `x = 1` before the
        // trailing read. RuboCop 1.87 flags `x = 1`.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              x = 1
              ^ Useless assignment to variable - `x`.
              x = 2
            rescue
              x = 3
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_killed_with_in_arm_reads_flagged() {
        // Distributed kill where the earliest compatible read sits *inside* the
        // `resbody`, but only after that arm's own overwrite (`x = 2`). The
        // no-exception path is covered by `else` (`x = 3`). No path observes
        // `x = 1`. RuboCop 1.87 flags it.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              x = 1
              ^ Useless assignment to variable - `x`.
            rescue
              x = 2
              use(x)
            else
              x = 3
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_read_before_later_body_write_not_flagged() {
        // FP guard: a begin-body read of `x` sits between the write and the
        // later body overwrite, so the no-exception path observes `x = 1`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
              use(x)
              x = 2
            rescue
              x = 3
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_read_before_resbody_overwrite_not_flagged() {
        // FP guard: the `resbody` reads `x` before overwriting it, so the
        // exception path observes `x = 1`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            rescue
              use(x)
              x = 2
            else
              x = 3
            end
            use(x)
        "#});
    }

    #[test]
    fn begin_body_write_with_escaping_exception_to_outer_ensure_not_flagged() {
        // FP guard: a `RuntimeError` bypasses the inner `rescue IOError` and the
        // outer `ensure` reads the original `x = 1` on the propagation path, so
        // the inner body write is live. The distributed kill must stay
        // conservative when the rescue is nested in an enclosing `ensure`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              begin
                x = 1
                raise RuntimeError
              rescue IOError
                x = 2
              else
                x = 3
              end
            ensure
              use(x)
            end
        "#});
    }

    #[test]
    fn retry_accumulator_op_assign_not_flagged() {
        // request_pool.rb `retries`: `retries += 1; retry` — the op-assign
        // is read on the next iteration via the retry back-edge.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            retries = 0
            begin
              do_work
            rescue StandardError
              if retries.positive?
                raise
              else
                retries += 1
                retry
              end
            end
        "#});
    }

    #[test]
    fn retry_accumulator_simple_not_flagged() {
        // snowflake.rb `tries`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            tries = 0
            begin
              insert_record
            rescue RecordNotUnique
              raise if tries > 100
              tries += 1
              retry
            end
        "#});
    }

    #[test]
    fn dead_write_in_rescue_without_retry_still_flagged() {
        // Negative control for the retry-rescue loop-ification: a rescue body
        // with NO `retry` is not loop-ified, so `is_in_retry_rescue` is false
        // and a never-read write inside it must still be flagged by normal
        // dataflow. Guards against `is_in_retry_rescue` over-broadening to any
        // rescue body.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              work
            rescue StandardError
              leftover = compute
              ^^^^^^^^ Useless assignment to variable - `leftover`.
              raise
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UselessAssignment);
