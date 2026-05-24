use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, Severity};

#[derive(Default)]
pub struct EmptyWhen;

impl Cop for EmptyWhen {
    type Options = NoOptions;
    const NAME: &'static str = "Lint/EmptyWhen";
    const DESCRIPTION: &'static str = "Flag when branches without a body.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

const WHEN_TAG: NodeKindTag = NodeKindTag(27);

impl NodeCop for EmptyWhen {
    const KINDS: &'static [NodeKindTag] = &[WHEN_TAG];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::When { body, .. } = *cx.kind(node) else {
            return;
        };
        if body.is_none() {
            cx.emit_offense(cx.range(node), "Avoid empty when branches", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyWhen;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    #[test]
    fn flags_empty_when() {
        expect_offense!(
            EmptyWhen,
            indoc! {r#"
            case value
            when 1
            ^^^^^^ Avoid empty when branches
            when 2
              :ok
            end
        "#}
        );
    }

    #[test]
    fn ignores_non_empty_when_with_multibyte_body() {
        expect_no_offenses!(EmptyWhen, "case x\nwhen 1\n  名前\nend\n");
    }
}
