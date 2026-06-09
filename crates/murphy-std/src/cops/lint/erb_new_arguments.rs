//! `Lint/ErbNewArguments` — flag deprecated positional arguments to `ERB.new`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ErbNewArguments
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-i1hc
//! notes: >
//!   Covers ERB.new/::ERB.new positional safe_level, trim_mode, and eoutvar
//!   arguments and autocorrects the common legacy positional form. Existing
//!   duplicate keyword override handling is deferred for v1.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct ErbNewArguments;

#[cop(
    name = "Lint/ErbNewArguments",
    description = "Use trim_mode and eoutvar keyword arguments to ERB.new.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ErbNewArguments {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else { return; };
        let Some(receiver) = receiver.get() else { return; };
        if !is_erb_const(receiver, cx) {
            return;
        }
        let args = cx.list(args);
        if args.len() <= 1 || (args.len() == 2 && matches!(cx.kind(args[1]), NodeKind::Hash(_))) {
            return;
        }
        for (idx, &arg) in args.iter().enumerate().take(4).skip(1) {
            if matches!(cx.kind(arg), NodeKind::Hash(_)) {
                continue;
            }
            cx.emit_offense(cx.range(arg), &message(idx, arg, cx), None);
        }
        if let Some(range) = arguments_range(args, cx) {
            cx.emit_edit(range, &corrected_arguments(args, cx));
        }
    }
}

fn is_erb_const(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Const { scope, name } if cx.symbol_str(name) == "ERB" && scope.get().is_none_or(|s| matches!(cx.kind(s), NodeKind::Cbase)))
}

fn message(index: usize, arg: NodeId, cx: &Cx<'_>) -> String {
    match index {
        1 => "Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.".to_string(),
        2 => format!("Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: {})` instead.", cx.raw_source(cx.range(arg))),
        _ => format!("Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: {})` instead.", cx.raw_source(cx.range(arg))),
    }
}

fn arguments_range(args: &[NodeId], cx: &Cx<'_>) -> Option<Range> {
    let first = args.first()?;
    let last = args.last()?;
    Some(Range { start: cx.range(*first).start, end: cx.range(*last).end })
}

fn corrected_arguments(args: &[NodeId], cx: &Cx<'_>) -> String {
    let mut parts = vec![cx.raw_source(cx.range(args[0])).to_string()];
    if let Some(&trim_mode) = args.get(2) {
        parts.push(format!("trim_mode: {}", cx.raw_source(cx.range(trim_mode))));
    }
    if let Some(&eoutvar) = args.get(3)
        && !matches!(cx.kind(eoutvar), NodeKind::Hash(_))
    {
        parts.push(format!("eoutvar: {}", cx.raw_source(cx.range(eoutvar))));
    }
    parts.join(", ")
}

murphy_plugin_api::submit_cop!(ErbNewArguments);

#[cfg(test)]
mod tests {
    use super::ErbNewArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_legacy_positional_arguments() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil, '-', '@output_buffer')
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                  ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
                                       ^^^^^^^^^^^^^^^^ Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: '@output_buffer')` instead.
            "#},
            "ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n",
        );
    }

    #[test]
    fn accepts_keyword_arguments() {
        test::<ErbNewArguments>().expect_no_offenses("ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n");
    }
}
