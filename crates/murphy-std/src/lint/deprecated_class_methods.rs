use murphy_plugin_api::{
    Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, Range, Severity,
};

#[derive(Default)]
pub struct DeprecatedClassMethods;

impl Cop for DeprecatedClassMethods {
    type Options = NoOptions;
    const NAME: &'static str = "Lint/DeprecatedClassMethods";
    const DESCRIPTION: &'static str = "Flag deprecated class method calls with safe replacements.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

const SEND_TAG: NodeKindTag = NodeKindTag(17);

impl NodeCop for DeprecatedClassMethods {
    const KINDS: &'static [NodeKindTag] = &[SEND_TAG];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if cx.symbol_str(method) != "exists?" {
            return;
        }
        let Some(receiver) = receiver.get() else {
            return;
        };
        if !matches_const(cx, receiver, &["File"]) && !matches_const(cx, receiver, &["FileTest"]) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `exist?` instead of deprecated `exists?`",
            None,
        );
        if let Some(range) = selector_range(cx, node, "exists?") {
            cx.emit_edit(range, "exist?");
        }
    }
}

fn matches_const(cx: &Cx<'_>, node: NodeId, names: &[&str]) -> bool {
    let mut out = Vec::new();
    let mut cur = Some(node);
    while let Some(id) = cur {
        match *cx.kind(id) {
            NodeKind::Const { scope, name } => {
                out.push(cx.symbol_str(name));
                cur = scope.get();
            }
            _ => return false,
        }
    }
    out.reverse();
    out == names
}

fn selector_range(cx: &Cx<'_>, node: NodeId, selector: &str) -> Option<Range> {
    let range = cx.range(node);
    let raw = cx.raw_source(range);
    let pos = raw.find(selector)? as u32;
    Some(Range {
        start: range.start + pos,
        end: range.start + pos + selector.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::DeprecatedClassMethods;
    use murphy_plugin_api::{
        Range,
        test_support::{expect_no_offenses, expect_offense, indoc, run_cop_with_edits},
    };

    #[test]
    fn flags_file_exists_and_filetest_exists() {
        expect_offense!(
            DeprecatedClassMethods,
            indoc! {r#"
            File.exists?(path)
            ^^^^^^^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
            FileTest.exists?(path)
            ^^^^^^^^^^^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
        "#}
        );
    }

    #[test]
    fn autocorrects_selector_only_and_reaches_fixpoint() {
        let run = run_cop_with_edits::<DeprecatedClassMethods>("File.exists?(path)\n");
        assert_eq!(run.edits[0].range, Range { start: 5, end: 12 });
        assert_eq!(run.edits[0].replacement, "exist?");
        expect_no_offenses!(
            DeprecatedClassMethods,
            "File.exist?(path)\n名前 = File.exist?(path)\n"
        );
    }
}
