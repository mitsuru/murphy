use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct EmptyWhen;

#[cop(
    name = "Lint/EmptyWhen",
    description = "Flag when branches without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyWhen {
    #[on_node(kind = "when")]
    fn check_when(&self, node: NodeId, cx: &Cx<'_>) {
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
