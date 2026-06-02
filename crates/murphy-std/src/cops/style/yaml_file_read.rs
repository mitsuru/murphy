//! `Style/YAMLFileRead` — flags `YAML.load(File.read(path))`,
//! `YAML.safe_load(File.read(path))`, and `YAML.parse(File.read(path))`
//! in favor of the `*_file` variants.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/YAMLFileRead
//! upstream_version_checked: 1.86.2
//! status: complete
//! gap_issues: []
//! notes: >
//!   Murphy v1 handles the core case matching YAML.load/safe_load/parse
//!   with File.read as the first argument, with optional additional arguments.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! YAML.load(File.read(path))
//! YAML.parse(File.read(path))
//! YAML.safe_load(File.read(path))
//! YAML.load(File.read(path), symbolize_names: true)
//!
//! # good
//! YAML.load_file(path)
//! YAML.parse_file(path)
//! YAML.safe_load_file(path)
//! YAML.load_file(path, symbolize_names: true)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `YAML.<method>(File.read(<path>)<rest>)` with
//! `YAML.<method>_file(<path><rest>)`.
//!
//! This is a structural rearrangement (whole-range replacement from the
//! selector to the end of the expression).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use `%s` instead.";

/// Stateless unit struct.
#[derive(Default)]
pub struct YAMLFileRead;

#[cop(
    name = "Style/YAMLFileRead",
    description = "Use `YAML.*_file` instead of `YAML.*(File.read(path))`.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "3.1",
    options = NoOptions,
)]
impl YAMLFileRead {
    #[on_node(kind = "send", methods = ["load", "safe_load", "parse"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `Some((yaml_recv, method_str, file_path_node, rest_args_slice))`
/// if `node` matches `YAML.<method>(File.read(<path>) <rest...>)`.
fn match_yaml_file_read<'a>(
    node: NodeId,
    cx: &Cx<'a>,
) -> Option<(NodeId, &'a str, NodeId, &'a [NodeId])> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };

    // Receiver must be YAML (qualified or top-level).
    let recv = receiver.get()?;
    if !cx.is_global_const(recv, "YAML") {
        return None;
    }

    let method_str = cx.symbol_str(method);
    if !matches!(method_str, "load" | "safe_load" | "parse") {
        return None;
    }

    let arg_list = cx.list(args);
    if arg_list.is_empty() {
        return None;
    }

    // First argument must be `File.read(<path>)`.
    let first_arg = arg_list[0];
    let file_path = extract_file_read_arg(first_arg, cx)?;

    let rest = &arg_list[1..];
    Some((recv, method_str, file_path, rest))
}

/// If `node` is `File.read(<path>)`, return the `<path>` node.
fn extract_file_read_arg(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };

    // Receiver must be File (qualified or top-level).
    let recv = receiver.get()?;
    if !cx.is_global_const(recv, "File") {
        return None;
    }

    if cx.symbol_str(method) != "read" {
        return None;
    }

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        // File.read with wrong number of args — skip.
        return None;
    }

    Some(arg_list[0])
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some((_recv, method_str, file_path, rest)) = match_yaml_file_read(node, cx) else {
        return;
    };

    // Build the preferred method call string.
    let path_src = cx.raw_source(cx.range(file_path));
    let rest_src: String = if rest.is_empty() {
        String::new()
    } else {
        let parts: Vec<&str> = rest.iter().map(|&n| cx.raw_source(cx.range(n))).collect();
        format!(", {}", parts.join(", "))
    };
    let prefer = format!("{method_str}_file({path_src}{rest_src})");

    // Offense range: from the selector (method name) to the end of the call.
    let selector = cx.selector(node);
    let node_range = cx.range(node);
    let offense_range = if selector != Range::ZERO {
        Range {
            start: selector.start,
            end: node_range.end,
        }
    } else {
        node_range
    };

    let message = MSG.replacen("%s", &prefer, 1);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: skip when the call contains inline comments — whole-range
    // reconstruction would silently drop them (there is no faithful
    // comment-preserving rewrite path in v1).
    if cx.comments_for_node(node).is_empty() {
        cx.emit_edit(offense_range, &prefer);
    }
}

#[cfg(test)]
mod tests {
    use super::YAMLFileRead;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_yaml_load_file_read() {
        test::<YAMLFileRead>().expect_correction(
            indoc! {r#"
                YAML.load(File.read(path))
                     ^^^^^^^^^^^^^^^^^^^^^ Use `load_file(path)` instead.
            "#},
            "YAML.load_file(path)\n",
        );
    }

    #[test]
    fn flags_yaml_safe_load_file_read() {
        test::<YAMLFileRead>().expect_correction(
            indoc! {r#"
                YAML.safe_load(File.read(path))
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `safe_load_file(path)` instead.
            "#},
            "YAML.safe_load_file(path)\n",
        );
    }

    #[test]
    fn flags_yaml_parse_file_read() {
        test::<YAMLFileRead>().expect_correction(
            indoc! {r#"
                YAML.parse(File.read(path))
                     ^^^^^^^^^^^^^^^^^^^^^^ Use `parse_file(path)` instead.
            "#},
            "YAML.parse_file(path)\n",
        );
    }

    #[test]
    fn flags_yaml_load_file_read_with_extra_args() {
        test::<YAMLFileRead>().expect_correction(
            indoc! {r#"
                YAML.load(File.read(path), symbolize_names: true)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `load_file(path, symbolize_names: true)` instead.
            "#},
            "YAML.load_file(path, symbolize_names: true)\n",
        );
    }

    #[test]
    fn accepts_yaml_load_file_directly() {
        test::<YAMLFileRead>().expect_no_offenses("YAML.load_file(path)\n");
    }

    #[test]
    fn accepts_yaml_load_non_file_read() {
        test::<YAMLFileRead>().expect_no_offenses("YAML.load(yaml_string)\n");
    }

    #[test]
    fn accepts_yaml_load_without_args() {
        test::<YAMLFileRead>().expect_no_offenses("YAML.load\n");
    }

    #[test]
    fn accepts_other_class_load_file_read() {
        // Only YAML.* is flagged, not some other class.
        test::<YAMLFileRead>().expect_no_offenses("JSON.load(File.read(path))\n");
    }

    #[test]
    fn accepts_yaml_load_other_receiver_read() {
        // Only File.read is the trigger, not other receivers.
        test::<YAMLFileRead>().expect_no_offenses("YAML.load(IO.read(path))\n");
    }

    #[test]
    fn flags_qualified_yaml_load() {
        // ::YAML.load(File.read(path)) — fully qualified.
        test::<YAMLFileRead>().expect_correction(
            indoc! {r#"
                ::YAML.load(::File.read(path))
                       ^^^^^^^^^^^^^^^^^^^^^^^ Use `load_file(path)` instead.
            "#},
            "::YAML.load_file(path)\n",
        );
    }

    #[test]
    fn flags_but_no_autocorrect_with_inline_comment() {
        // When the call contains inline comments, the offense is still flagged
        // but autocorrect is skipped to avoid dropping the comment.
        test::<YAMLFileRead>().expect_offense(indoc! {r#"
            YAML.load(File.read(path)) # important note
                 ^^^^^^^^^^^^^^^^^^^^^ Use `load_file(path)` instead.
        "#});
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <YAMLFileRead as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(3, 1)),
        );
    }
}
murphy_plugin_api::submit_cop!(YAMLFileRead);
