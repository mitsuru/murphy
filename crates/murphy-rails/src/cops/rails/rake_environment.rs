//! `Rails/RakeEnvironment` — Rake tasks must depend on `:environment`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RakeEnvironment
//! upstream_version_checked: 2.35.0
//! version_added: "2.4"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_block` (plain blocks only, matching
//!   the upstream Numblock/Itblock opt-out): bare `task` sends, `:default`
//!   exemption, hash-style dependency detection (single-pair hash first
//!   arg, or hash second arg), and the argument-list (`task :foo, [...]`)
//!   correction branch. Offense is the `task` send; autocorrect rewrites
//!   the task name to `name: :environment` (sym) or `name => :environment`
//!   and appends `=> :environment` to argument lists. Upstream
//!   Include/Exclude path gating (`**/Rakefile`, `**/*.rake` minus
//!   capistrano paths) has no file-path infrastructure in Murphy yet, so
//!   the cop fires in all files.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RakeEnvironment;

#[cop(
    name = "Rails/RakeEnvironment",
    description = "Include `:environment` as a dependency for all Rake tasks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RakeEnvironment {
    // Upstream `on_block` with Numblock/Itblock handlers disabled.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(block: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, .. } = *cx.kind(block) else {
        return;
    };
    // `(block $(send nil? :task ...) ...)` — bare `task` send.
    if !matches!(*cx.kind(call), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(call) != Some("task") {
        return;
    }
    if cx.call_receiver(call).get().is_some() {
        return;
    }
    let args = cx.call_arguments(call);
    let Some(first_arg) = args.first().copied() else {
        return;
    };
    // `:default` tasks are exempt.
    if task_name(cx, first_arg) == Some("default".to_owned()) {
        return;
    }
    if with_dependencies(cx, args) {
        return;
    }
    // NOTE: `cx.range` on a block-call send covers the whole block
    // expression, so the offense range is rebuilt as call-start through
    // the last argument (plus the closing paren for parenthesized calls).
    cx.emit_offense(
        send_range(cx, call, args),
        "Include `:environment` task as a dependency for all Rake tasks.",
        None,
    );
    if with_arguments(cx, args) {
        // `task :foo, [:bar]` → `task :foo, [:bar] => :environment`.
        let task_args = args[1];
        let replacement = format!("{} => :environment", cx.raw_source(cx.range(task_args)));
        cx.emit_edit(cx.range(task_args), &replacement);
    } else {
        let replacement = task_dependency_replacement(cx, first_arg);
        cx.emit_edit(cx.range(first_arg), &replacement);
    }
}

/// Call-start through the last argument end (plus `)` when parenthesized).
fn send_range(cx: &Cx<'_>, call: NodeId, args: &[NodeId]) -> murphy_plugin_api::Range {
    let mut end = cx.range(args[args.len() - 1]).end;
    if cx.is_parenthesized(call) {
        let src = cx.raw_source(murphy_plugin_api::Range {
            start: end,
            end: cx.range(call).end,
        });
        let mut extra = 0u32;
        for b in src.as_bytes() {
            if *b == b' ' || *b == b'\t' {
                extra += 1;
            } else {
                break;
            }
        }
        if src.as_bytes().get(extra as usize) == Some(&b')') {
            end += extra + 1;
        }
    }
    murphy_plugin_api::Range {
        start: cx.range(call).start,
        end,
    }
}

/// Upstream `task_name`: sym/str value, or the single-pair hash key.
fn task_name(cx: &Cx<'_>, first_arg: NodeId) -> Option<String> {
    match *cx.kind(first_arg) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Str(id) => Some(cx.string_str(id).to_owned()),
        NodeKind::Hash(list) => {
            let pairs = cx.list(list);
            if pairs.len() != 1 {
                return None;
            }
            let NodeKind::Pair { key, .. } = *cx.kind(pairs[0]) else {
                return None;
            };
            match *cx.kind(key) {
                NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
                NodeKind::Str(id) => Some(cx.string_str(id).to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Upstream `with_arguments?`: a second argument that is an array task-arg list.
fn with_arguments(cx: &Cx<'_>, args: &[NodeId]) -> bool {
    args.len() > 1 && matches!(*cx.kind(args[1]), NodeKind::Array(_))
}

/// Upstream `with_dependencies?`.
fn with_dependencies(cx: &Cx<'_>, args: &[NodeId]) -> bool {
    let first_arg = args[0];
    if matches!(*cx.kind(first_arg), NodeKind::Hash(_)) {
        return with_hash_style_dependencies(cx, first_arg);
    }
    if args.len() < 2 {
        return false;
    }
    let task_args = args[1];
    if !matches!(*cx.kind(task_args), NodeKind::Hash(_)) {
        return false;
    }
    with_hash_style_dependencies(cx, task_args)
}

/// Upstream `with_hash_style_dependencies?`: the first pair's value is a
/// non-empty array or any non-array dependency.
fn with_hash_style_dependencies(cx: &Cx<'_>, hash: NodeId) -> bool {
    let pairs = cx.hash_pairs(hash);
    let Some(first) = pairs.first().copied() else {
        return false;
    };
    let NodeKind::Pair { value, .. } = *cx.kind(first) else {
        return false;
    };
    match *cx.kind(value) {
        NodeKind::Array(list) => !cx.list(list).is_empty(),
        _ => true,
    }
}

/// Upstream correction: sym names become `name: :environment`, anything
/// else becomes `source => :environment`.
fn task_dependency_replacement(cx: &Cx<'_>, task_name_node: NodeId) -> String {
    if let NodeKind::Sym(s) = *cx.kind(task_name_node) {
        format!("{}: :environment", cx.symbol_str(s))
    } else {
        format!("{} => :environment", cx.raw_source(cx.range(task_name_node)))
    }
}

#[cfg(test)]
mod tests {
    use super::RakeEnvironment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_task() {
        test::<RakeEnvironment>().expect_correction(
            indoc! {r#"
                task :foo do
                ^^^^^^^^^ Include `:environment` task as a dependency for all Rake tasks.
                  do_something
                end
            "#},
            "task foo: :environment do\n  do_something\nend\n",
        );
    }

    #[test]
    fn flags_string_task_name() {
        test::<RakeEnvironment>().expect_correction(
            indoc! {r#"
                task 'foo' do
                ^^^^^^^^^^ Include `:environment` task as a dependency for all Rake tasks.
                end
            "#},
            "task 'foo' => :environment do\nend\n",
        );
    }

    #[test]
    fn flags_task_with_arg_list() {
        test::<RakeEnvironment>().expect_correction(
            indoc! {r#"
                task :foo, [:bar] do
                ^^^^^^^^^^^^^^^^^ Include `:environment` task as a dependency for all Rake tasks.
                end
            "#},
            "task :foo, [:bar] => :environment do\nend\n",
        );
    }

    #[test]
    fn allows_default_task() {
        test::<RakeEnvironment>().expect_no_offenses(indoc! {r#"
            task default: :spec do
            end
        "#});
    }

    #[test]
    fn allows_task_with_environment() {
        test::<RakeEnvironment>().expect_no_offenses(indoc! {r#"
            task foo: :environment do
            end
        "#});
    }

    #[test]
    fn allows_non_task_block() {
        test::<RakeEnvironment>().expect_no_offenses(indoc! {r#"
            describe :foo do
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(RakeEnvironment);
