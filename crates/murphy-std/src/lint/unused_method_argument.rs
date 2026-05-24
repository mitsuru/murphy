use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct UnusedMethodArgument;

#[cop(
    name = "Lint/UnusedMethodArgument",
    description = "Flag method parameters that are never read.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnusedMethodArgument {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { args, body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body) = body.get() else {
            return;
        };
        let reads = lvar_reads(cx, body);
        let NodeKind::Args(list) = *cx.kind(args) else {
            return;
        };
        for param in cx.list(list) {
            let Some((name, range)) = param_name_and_range(cx, *param) else {
                continue;
            };
            let name_str = cx.symbol_str(name);
            if name_str.is_empty() || name_str.starts_with('_') || reads.contains(name_str) {
                continue;
            }
            cx.emit_offense(range, "Unused method argument", None);
            cx.emit_edit(
                Range {
                    start: range.start,
                    end: range.start,
                },
                "_",
            );
        }
    }
}

fn lvar_reads<'a>(cx: &Cx<'a>, body: NodeId) -> HashSet<&'a str> {
    std::iter::once(body)
        .chain(cx.descendants(body))
        .filter_map(|id| match *cx.kind(id) {
            NodeKind::Lvar(s) => Some(cx.symbol_str(s)),
            _ => None,
        })
        .collect()
}

fn param_name_and_range(cx: &Cx<'_>, node: NodeId) -> Option<(Symbol, Range)> {
    let name = match *cx.kind(node) {
        NodeKind::Arg(s)
        | NodeKind::Restarg(s)
        | NodeKind::Kwarg(s)
        | NodeKind::Kwrestarg(s)
        | NodeKind::Blockarg(s) => s,
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => name,
        _ => return None,
    };
    let raw = cx.raw_source(cx.range(node));
    let text = cx.symbol_str(name);
    let start = raw.find(text).unwrap_or(0) as u32 + cx.range(node).start;
    Some((
        name,
        Range {
            start,
            end: start + text.len() as u32,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::UnusedMethodArgument;
    use murphy_plugin_api::{
        Range,
        test_support::{expect_no_offenses, expect_offense, indoc, run_cop_with_edits},
    };

    #[test]
    fn flags_unused_method_arguments() {
        expect_offense!(
            UnusedMethodArgument,
            indoc! {r#"
            def call(used, unused, _ignored)
                           ^^^^^^ Unused method argument
              used
            end
        "#}
        );
    }

    #[test]
    fn autocorrects_by_prefixing_underscore_and_reaches_fixpoint() {
        let run = run_cop_with_edits::<UnusedMethodArgument>("def 名前(foo)\n  1\nend\n");
        assert_eq!(run.edits[0].range, Range { start: 11, end: 11 });
        assert_eq!(run.edits[0].replacement, "_");
        expect_no_offenses!(UnusedMethodArgument, "def 名前(_foo)\n  1\nend\n");
    }
}
