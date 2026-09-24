//! `Lint/ErbNewArguments` — flag deprecated positional arguments to `ERB.new`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ErbNewArguments
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop 1.87 `erb_new_with_non_keyword_arguments`,
//!   `correct_arguments?`, `build_kwargs`, and `override_by_legacy_args`:
//!   ERB.new/::ERB.new positional safe_level, trim_mode, and eoutvar offenses,
//!   with autocorrect that preserves existing trim_mode/eoutvar keywords from
//!   the trailing hash and lets legacy positional trim_mode/eoutvar override
//!   them. Gated at TargetRubyVersion 2.6 via `minimum_target_ruby_version`
//!   (registry) plus a `Cx::target_ruby_version()` runtime guard so direct
//!   invocations (tests) also stay silent on Ruby <= 2.5.
//!
//!   One deliberate hardening vs upstream: `override_by_legacy_args` guards
//!   `arguments[2]` against hash_type (upstream only guards `arguments[3]`).
//!   Without it `ERB.new(str, nil, trim_mode: 'foo')` expands to the invalid
//!   `ERB.new(str, trim_mode: trim_mode: 'foo')` (verified against RuboCop
//!   1.87.0). All spec-covered shapes are unaffected; the guard only fixes
//!   that untested invalid-output edge.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RubyVersion, cop};

#[derive(Default)]
pub struct ErbNewArguments;

#[cop(
    name = "Lint/ErbNewArguments",
    description = "Use trim_mode and eoutvar keyword arguments to ERB.new.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "2.6",
    options = NoOptions
)]
impl ErbNewArguments {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `minimum_target_ruby_version 2.6` — registry gates production runs;
        // this runtime guard keeps direct invocations (unit tests) consistent.
        // `None` (unset) resolves to Murphy's default floor (3.1) and fires.
        if cx
            .target_ruby_version()
            .is_some_and(|v| v < RubyVersion::new(2, 6))
        {
            return;
        }
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else { return; };
        let Some(receiver) = receiver.get() else { return; };
        if !is_erb_const(receiver, cx) {
            return;
        }
        let args = cx.list(args);
        if args.is_empty() || correct_arguments(args, cx) {
            return;
        }
        let mut offense_count = 0;
        for (idx, &arg) in args.iter().enumerate().take(4).skip(1) {
            if matches!(cx.kind(arg), NodeKind::Hash(_)) {
                continue;
            }
            cx.emit_offense(cx.range(arg), &message(idx, arg, cx), None);
            offense_count += 1;
        }
        if offense_count > 0
            && let Some(range) = arguments_range(args, cx)
        {
            cx.emit_edit(range, &corrected_arguments(args, cx));
        }
    }
}

fn is_erb_const(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Const { scope, name } if cx.symbol_str(name) == "ERB" && scope.get().is_none_or(|s| matches!(cx.kind(s), NodeKind::Cbase)))
}

fn correct_arguments(args: &[NodeId], cx: &Cx<'_>) -> bool {
    args.len() == 1 || (args.len() == 2 && matches!(cx.kind(args[1]), NodeKind::Hash(_)))
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

/// Upstream `build_kwargs`: extract `trim_mode:` / `eoutvar:` values from the
/// trailing hash, if present. Last matching pair wins (mirrors upstream's
/// overwrite loop). Keys are `Sym` nodes named `trim_mode` / `eoutvar`, which
/// covers both `trim_mode: ...` and `:trim_mode => ...` spellings; other keys
/// (including `**` kwsplats, which are not `Pair`s) are ignored.
fn build_kwargs(args: &[NodeId], cx: &Cx<'_>) -> [Option<String>; 2] {
    let Some(&last) = args.last() else {
        return [None, None];
    };
    if !matches!(cx.kind(last), NodeKind::Hash(_)) {
        return [None, None];
    }
    let mut trim_mode: Option<String> = None;
    let mut eoutvar: Option<String> = None;
    for &pair in cx.hash_pairs(last).iter() {
        let Some(key) = cx.pair_key(pair).get() else {
            continue;
        };
        let NodeKind::Sym(sym) = *cx.kind(key) else {
            continue;
        };
        let name = cx.symbol_str(sym);
        let Some(value) = cx.pair_value(pair).get() else {
            continue;
        };
        let src = cx.raw_source(cx.range(value)).to_string();
        if name == "trim_mode" {
            trim_mode = Some(format!("trim_mode: {src}"));
        } else if name == "eoutvar" {
            eoutvar = Some(format!("eoutvar: {src}"));
        }
    }
    [trim_mode, eoutvar]
}

/// Upstream `override_by_legacy_args`: legacy positional `arguments[2]`
/// (trim_mode) and `arguments[3]` (eoutvar, when not a hash) override the
/// preserved keywords. Unlike upstream, trim_mode is also guarded against
/// hash_type to avoid emitting the invalid `trim_mode: trim_mode: ...` for
/// `ERB.new(str, nil, trim_mode: ...)` (see module docs).
fn override_by_legacy_args(
    mut kwargs: [Option<String>; 2],
    args: &[NodeId],
    cx: &Cx<'_>,
) -> [Option<String>; 2] {
    if let Some(&trim_arg) = args.get(2)
        && !matches!(cx.kind(trim_arg), NodeKind::Hash(_))
    {
        kwargs[0] = Some(format!("trim_mode: {}", cx.raw_source(cx.range(trim_arg))));
    }
    if let Some(&eoutvar_arg) = args.get(3)
        && !matches!(cx.kind(eoutvar_arg), NodeKind::Hash(_))
    {
        kwargs[1] = Some(format!("eoutvar: {}", cx.raw_source(cx.range(eoutvar_arg))));
    }
    kwargs
}

fn corrected_arguments(args: &[NodeId], cx: &Cx<'_>) -> String {
    let mut parts = vec![cx.raw_source(cx.range(args[0])).to_string()];
    let kwargs = override_by_legacy_args(build_kwargs(args, cx), args, cx);
    for kw in kwargs.into_iter().flatten() {
        parts.push(kw);
    }
    parts.join(", ")
}

murphy_plugin_api::submit_cop!(ErbNewArguments);

#[cfg(test)]
mod tests {
    use super::ErbNewArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <ErbNewArguments as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(2, 6)),
        );
    }

    #[test]
    fn no_offense_on_ruby_2_5() {
        test::<ErbNewArguments>()
            .with_target_ruby_version(2, 5)
            .expect_no_offenses("ERB.new(str, nil, '-', '@output_buffer')\n");
    }

    #[test]
    fn flags_and_corrects_second_argument_only() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil)
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
            "#},
            "ERB.new(str)\n",
        );
    }

    #[test]
    fn flags_and_corrects_second_and_third_arguments() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil, '-')
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                  ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
            "#},
            "ERB.new(str, trim_mode: '-')\n",
        );
    }

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
    fn flags_and_corrects_five_arguments_with_trailing_keywords() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil, '-', '@output_buffer', trim_mode: '-', eoutvar: '@output_buffer')
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                  ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
                                       ^^^^^^^^^^^^^^^^ Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: '@output_buffer')` instead.
            "#},
            "ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n",
        );
    }

    #[test]
    fn flags_and_corrects_positional_override_preserving_trailing_keywords() {
        // `arguments[2]` ('-') overrides the trailing `trim_mode:`, and the
        // trailing `eoutvar:` is preserved (no positional eoutvar).
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil, '-', trim_mode: '-', eoutvar: '@output_buffer')
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                  ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
            "#},
            "ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n",
        );
    }

    #[test]
    fn flags_and_corrects_cbase_receiver() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ::ERB.new(str, nil, '-', '@output_buffer')
                               ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                    ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
                                         ^^^^^^^^^^^^^^^^ Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: '@output_buffer')` instead.
            "#},
            "::ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n",
        );
    }

    #[test]
    fn preserves_trailing_eoutvar_when_only_trim_positional() {
        test::<ErbNewArguments>().expect_correction(
            indoc! {r#"
                ERB.new(str, nil, '-', eoutvar: '@out')
                             ^^^ Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.
                                  ^^^ Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: '-')` instead.
            "#},
            "ERB.new(str, trim_mode: '-', eoutvar: '@out')\n",
        );
    }

    #[test]
    fn accepts_keyword_arguments() {
        test::<ErbNewArguments>().expect_no_offenses("ERB.new(str, trim_mode: '-', eoutvar: '@output_buffer')\n");
    }

    #[test]
    fn accepts_single_argument() {
        test::<ErbNewArguments>().expect_no_offenses("ERB.new(str)\n");
    }

    #[test]
    fn accepts_no_arguments() {
        test::<ErbNewArguments>().expect_no_offenses("ERB.new\n");
    }
}
