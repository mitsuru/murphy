//! `Lint/ToJSON` — check that `#to_json` requires an optional argument to
//! be parsable via `JSON.generate(obj)`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ToJSON
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/ToJSON cop: a `def to_json` with no parameters
//!   is flagged and autocorrected to `def to_json(*_args)`.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, cop};

#[derive(Default)]
pub struct ToJSON;

#[cop(
    name = "Lint/ToJSON",
    description = "`#to_json` requires an optional argument.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ToJSON {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { name, args, .. } = *cx.kind(node) else { return; };
        if cx.symbol_str(name) != "to_json" { return; }
        let NodeKind::Args(list) = *cx.kind(args) else { return; };
        if !cx.list(list).is_empty() { return; }
        let node_range = cx.range(node);
        let name_str = cx.symbol_str(name);
        let src = cx.raw_source(node_range);
        let first_line = src.lines().next().unwrap_or(src);
        let name_pos = first_line.find(name_str).unwrap_or(0);
        let name_end = first_line[..name_pos + name_str.len()].len();
        let name_range = Range {
            start: node_range.start,
            end: node_range.start + name_end as u32,
        };
        cx.emit_offense(
            name_range,
            "`#to_json` requires an optional argument to be parsable via JSON.generate(obj).",
            None,
        );
        cx.emit_edit(
            Range { start: name_range.end, end: name_range.end },
            "(*_args)",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ToJSON;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_json_without_args() {
        test::<ToJSON>().expect_correction(
            indoc! {r#"
                def to_json
                ^^^^^^^^^^^ `#to_json` requires an optional argument to be parsable via JSON.generate(obj).
                end
            "#},
            "def to_json(*_args)\nend\n",
        );
    }

    #[test]
    fn ignores_to_json_with_args() {
        test::<ToJSON>().expect_no_offenses("def to_json(*_args)\nend\n");
    }

    #[test]
    fn ignores_non_to_json_methods() {
        test::<ToJSON>().expect_no_offenses("def foo\nend\n");
    }

    #[test]
    fn ignores_to_json_with_required_arg() {
        test::<ToJSON>().expect_no_offenses("def to_json(opts)\nend\n");
    }
}
murphy_plugin_api::submit_cop!(ToJSON);
