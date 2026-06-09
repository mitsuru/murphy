//! `Lint/FormatParameterMismatch` — flag obvious format argument count mismatches.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FormatParameterMismatch
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-py1j
//! notes: >
//!   Covers static str/dstr format and sprintf calls plus String#% with array
//!   arguments for common unnumbered, numbered, named, and mixed format
//!   sequences. Full RuboCop::Cop::Utils::FormatString parity is deferred.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct FormatParameterMismatch;

#[cop(
    name = "Lint/FormatParameterMismatch",
    description = "Check format field and argument counts.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FormatParameterMismatch {
    #[on_node(kind = "send", methods = ["format", "sprintf", "%"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((method_name, format_node, passed_args)) = format_call(node, cx) else { return; };
        let source = cx.raw_source(cx.range(format_node));
        let parsed = parse_format(source);
        if parsed.mixed {
            cx.emit_offense(cx.node(node).loc.name, "Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.", None);
            return;
        }
        let Some(field_count) = parsed.field_count else { return; };
        if field_count != passed_args {
            let display_method = if method_name == "%" { "String#%" } else { method_name };
            cx.emit_offense(
                cx.node(node).loc.name,
                &format!("Number of arguments ({passed_args}) to `{display_method}` doesn't match the number of fields ({field_count})."),
                None,
            );
        }
    }
}

fn format_call<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<(&'a str, NodeId, usize)> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else { return None; };
    let method_name = cx.symbol_str(method);
    let args = cx.list(args);
    match method_name {
        "format" | "sprintf" => {
            let receiver_ok = receiver.get().is_none_or(|recv| matches!(*cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Kernel"));
            if !receiver_ok
                || args.is_empty()
                || !is_string(args[0], cx)
                || args.iter().skip(1).any(|&arg| matches!(cx.kind(arg), NodeKind::Splat(_)))
            {
                return None;
            }
            Some((method_name, args[0], args.len() - 1))
        }
        "%" => {
            let recv = receiver.get()?;
            if !is_string(recv, cx) || args.len() != 1 {
                return None;
            }
            let passed = match *cx.kind(args[0]) {
                NodeKind::Array(items) => cx.list(items).len(),
                NodeKind::Hash(_) => 1,
                _ => 1,
            };
            Some((method_name, recv, passed))
        }
        _ => None,
    }
}

fn is_string(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_))
}

struct ParsedFormat {
    field_count: Option<usize>,
    mixed: bool,
}

fn parse_format(src: &str) -> ParsedFormat {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut unnumbered = 0usize;
    let mut max_numbered = 0usize;
    let mut named = false;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() || bytes[i] == b'%' {
            i += 1;
            continue;
        }
        if bytes[i] == b'<' || bytes[i] == b'{' {
            named = true;
            while i < bytes.len() && bytes[i] != b'>' && bytes[i] != b'}' {
                i += 1;
            }
            i += 1;
            continue;
        }
        let digit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'$' && i > digit_start {
            if let Ok(n) = src[digit_start..i].parse::<usize>() {
                max_numbered = max_numbered.max(n);
            }
            i += 1;
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() && bytes[i] != b'%' {
                if bytes[i] == b'*' {
                    unnumbered += 1;
                }
                i += 1;
            }
            if i < bytes.len() && bytes[i] != b'%' {
                unnumbered += 1;
            }
            i += 1;
        }
    }
    let kinds = [unnumbered > 0, max_numbered > 0, named].into_iter().filter(|b| *b).count();
    let mixed = kinds > 1;
    let field_count = if named { Some(1) } else if max_numbered > 0 { Some(max_numbered) } else { Some(unnumbered) };
    ParsedFormat { field_count, mixed }
}

murphy_plugin_api::submit_cop!(FormatParameterMismatch);

#[cfg(test)]
mod tests {
    use super::FormatParameterMismatch;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_format_argument_count_mismatch() {
        test::<FormatParameterMismatch>()
            .expect_offense(indoc! {r#"
                format('A value: %s and another: %i', a_value)
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "#})
            .expect_offense(indoc! {r#"
                format('%s')
                ^^^^^^ Number of arguments (0) to `format` doesn't match the number of fields (1).
            "#});
    }

    #[test]
    fn flags_mixed_format_sequence_types() {
        test::<FormatParameterMismatch>().expect_offense(indoc! {r#"
            format('Unnumbered: %s and numbered: %2$s', a, b)
            ^^^^^^ Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.
        "#});
    }

    #[test]
    fn accepts_matching_format_and_percent_calls() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("format('A value: %s and another: %i', a, b)\n")
            .expect_no_offenses("'%s %s' % [a, b]\n")
            .expect_no_offenses("format('%% done')\n");
    }
}
