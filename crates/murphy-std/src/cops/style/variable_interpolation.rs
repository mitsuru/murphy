//! `Style/VariableInterpolation` — flags direct variable interpolation like
//! `"#$global"`, `"#@ivar"`, `"#@@cvar"` and suggests the explicit brace form
//! `"#{$global}"`, `"#{@ivar}"`, `"#{@@cvar}"`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/VariableInterpolation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches gvar, ivar, cvar, nth_ref, and back_ref direct children of
//!   dstr, dsym, xstr, and regexp nodes -- mirroring RuboCop's
//!   Interpolation mixin and `var_nodes` helper.
//! ```
//!
//! ## Matched shapes
//!
//! Variable/reference nodes that appear as **direct** children of an
//! interpolated string/symbol/regexp/backtick-string. When wrapped in `Begin`
//! (i.e. `#{expr}`), they are not direct children, so they are not flagged.
//!
//! - `"#$name"` -- direct `gvar` child of `dstr` -- offense
//! - `"#@ivar"` -- direct `ivar` child of `dstr` -- offense
//! - `"#@@cvar"` -- direct `cvar` child of `dstr` -- offense
//! - `"#$1"` -- direct `nth_ref` child of `dstr` -- offense
//! - `"#$&"` -- direct `back_ref` child of `dstr` -- offense
//! - `"#{$name}"` -- `begin(gvar)` child -- not flagged
//!
//! ## Autocorrect
//!
//! Wraps the variable source in `{...}`: e.g. `$name` -> `{$name}`. Since the
//! `#` is the delimiter owned by the parent string node (not the variable
//! node), replacing the variable range with `{$name}` produces `#{$name}`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Replace interpolated variable `%s` with expression `#{%s}`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct VariableInterpolation;

#[cop(
    name = "Style/VariableInterpolation",
    description = "Don't interpolate global, instance and class variables directly in strings.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl VariableInterpolation {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dstr(children) = *cx.kind(node) else {
            return;
        };
        check_children(cx.list(children), cx);
    }

    #[on_node(kind = "dsym")]
    fn check_dsym(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dsym(children) = *cx.kind(node) else {
            return;
        };
        check_children(cx.list(children), cx);
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Xstr(children) = *cx.kind(node) else {
            return;
        };
        check_children(cx.list(children), cx);
    }

    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
            return;
        };
        check_children(cx.list(parts), cx);
    }
}

fn is_var_node(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Gvar(_)
            | NodeKind::Ivar(_)
            | NodeKind::Cvar(_)
            | NodeKind::NthRef(_)
            | NodeKind::BackRef(_)
    )
}

fn check_children(children: &[NodeId], cx: &Cx<'_>) {
    for &child in children {
        if is_var_node(cx.kind(child)) {
            let range = cx.range(child);
            let var_src = cx.raw_source(range);
            let message = MSG.replacen("%s", var_src, 2);
            cx.emit_offense(range, &message, None);
            // Autocorrect: wrap variable in braces so `#<var>` becomes `#{<var>}`.
            // The `#` is part of the parent string node, not the variable node,
            // so replacing the variable range with `{<var>}` suffices.
            let replacement = format!("{{{var_src}}}");
            cx.emit_edit(range, &replacement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VariableInterpolation;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- gvar -----

    #[test]
    fn flags_gvar_in_dstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                "His name is #$name"
                              ^^^^^ Replace interpolated variable `$name` with expression `#{$name}`.
            "#},
            "\"His name is #{$name}\"\n",
        );
    }

    #[test]
    fn flags_ivar_in_dstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                "Let's go to the #@store"
                                  ^^^^^^ Replace interpolated variable `@store` with expression `#{@store}`.
            "#},
            "\"Let's go to the #{@store}\"\n",
        );
    }

    #[test]
    fn flags_cvar_in_dstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                "Value is #@@count"
                           ^^^^^^^ Replace interpolated variable `@@count` with expression `#{@@count}`.
            "#},
            "\"Value is #{@@count}\"\n",
        );
    }

    #[test]
    fn flags_nth_ref_in_dstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                "Match is #$1"
                           ^^ Replace interpolated variable `$1` with expression `#{$1}`.
            "#},
            "\"Match is #{$1}\"\n",
        );
    }

    #[test]
    fn flags_back_ref_in_dstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                "Match is #$&"
                           ^^ Replace interpolated variable `$&` with expression `#{$&}`.
            "#},
            "\"Match is #{$&}\"\n",
        );
    }

    #[test]
    fn flags_two_vars_in_same_string() {
        test::<VariableInterpolation>().expect_offense(indoc! {r##"
                "#$a #$b"
                  ^^ Replace interpolated variable `$a` with expression `#{$a}`.
                      ^^ Replace interpolated variable `$b` with expression `#{$b}`.
            "##});
    }

    #[test]
    fn flags_gvar_in_regexp() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                /check #$pattern/
                        ^^^^^^^^ Replace interpolated variable `$pattern` with expression `#{$pattern}`.
            "#},
            "/check #{$pattern}/\n",
        );
    }

    #[test]
    fn flags_ivar_in_regexp() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                /#@store/
                  ^^^^^^ Replace interpolated variable `@store` with expression `#{@store}`.
            "#},
            "/#{@store}/\n",
        );
    }

    #[test]
    fn flags_gvar_in_dsym() {
        // dsym with a direct gvar interpolation (:"#$x" syntax)
        test::<VariableInterpolation>().expect_offense(indoc! {r##"
            :"#$x"
               ^^ Replace interpolated variable `$x` with expression `#{$x}`.
        "##});
    }

    #[test]
    fn flags_gvar_in_xstr() {
        test::<VariableInterpolation>().expect_correction(
            indoc! {r#"
                `echo #$x`
                       ^^ Replace interpolated variable `$x` with expression `#{$x}`.
            "#},
            "`echo #{$x}`\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_braced_interpolation() {
        test::<VariableInterpolation>().expect_no_offenses("\"#{$name}\"\n");
    }

    #[test]
    fn accepts_braced_ivar_interpolation() {
        test::<VariableInterpolation>().expect_no_offenses("\"#{@store}\"\n");
    }

    #[test]
    fn accepts_plain_string() {
        test::<VariableInterpolation>().expect_no_offenses("\"plain string\"\n");
    }

    #[test]
    fn accepts_braced_regexp_interpolation() {
        test::<VariableInterpolation>().expect_no_offenses("/check #{$pattern}/\n");
    }
}
murphy_plugin_api::submit_cop!(VariableInterpolation);
