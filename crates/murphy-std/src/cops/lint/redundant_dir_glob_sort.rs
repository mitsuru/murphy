//! `Lint/RedundantDirGlobSort` — detects redundant `sort` after `Dir.glob`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantDirGlobSort
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers `Dir.glob(...).sort`, `::Dir.glob(...).sort`, and
//!   `Dir[...].sort` without comparator blocks. TargetRubyVersion gating is a
//!   v1 gap; Murphy enables the cop according to its own configuration.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

#[derive(Default)]
pub struct RedundantDirGlobSort;

#[cop(
    name = "Lint/RedundantDirGlobSort",
    description = "Checks for redundant `sort` method to `Dir.glob` and `Dir[]`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantDirGlobSort {
    #[on_node(kind = "send", methods = ["sort"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.block_node(node).get().is_some() || !cx.call_arguments(node).is_empty() {
            return;
        }
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if !is_dir_glob(receiver, cx) {
            return;
        }
        cx.emit_offense(cx.selector(node), "Remove redundant sort.", None);
        let dot = cx.loc(node).dot();
        if dot != Range::ZERO {
            cx.emit_edit(
                Range {
                    start: dot.start,
                    end: cx.selector(node).end,
                },
                "",
            );
        }
    }
}

fn is_dir_glob(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return false;
    };
    let Some(receiver) = receiver.get() else {
        return false;
    };
    if !cx.is_global_const(receiver, "Dir") {
        return false;
    }
    let args = cx.list(args);
    if args.iter().any(|&arg| matches!(cx.kind(arg), NodeKind::Splat(_))) {
        return false;
    }
    if args
        .iter()
        .any(|&arg| cx.raw_source(cx.range(arg)).contains("sort: false"))
    {
        return false;
    }
    match cx.method_name(node) {
        Some("glob" | "[]") => !args.is_empty(),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(RedundantDirGlobSort);

#[cfg(test)]
mod tests {
    use super::RedundantDirGlobSort;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_dir_glob_sort() {
        test::<RedundantDirGlobSort>().expect_correction(
            indoc! {r#"
                Dir.glob('*.rb').sort
                                 ^^^^ Remove redundant sort.
            "#},
            "Dir.glob('*.rb')\n",
        );
    }

    #[test]
    fn flags_and_corrects_dir_glob_sort_with_options() {
        test::<RedundantDirGlobSort>()
            .expect_correction(
                indoc! {r#"
                    Dir.glob('*.rb', base: '.').sort
                                                ^^^^ Remove redundant sort.
                "#},
                "Dir.glob('*.rb', base: '.')\n",
            )
            .expect_correction(
                indoc! {r#"
                    Dir['*.rb', base: '.'].sort
                                           ^^^^ Remove redundant sort.
                "#},
                "Dir['*.rb', base: '.']\n",
            );
    }

    #[test]
    fn accepts_dir_glob_sort_with_splat_argument() {
        test::<RedundantDirGlobSort>()
            .expect_no_offenses("Dir.glob(*patterns).sort\n")
            .expect_no_offenses("Dir.glob('*.rb', sort: false).sort\n");
    }

    #[test]
    fn accepts_sort_with_comparator_block() {
        test::<RedundantDirGlobSort>()
            .expect_no_offenses("Dir.glob('*.rb').sort { |a, b| b <=> a }\n");
    }
}
