//! `Rails/RootPublicPath` — favor `Rails.public_path` over `Rails.root` with `'public'`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RootPublicPath
//! upstream_version_checked: 2.35.0
//! version_added: "2.15"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `Rails.root.join` whose first argument is
//!   a str starting with `public/` or equal to `public` (the anchored
//!   `public(/|end)` shape; sym/dstr first args never match). Whole-node
//!   offense with `Rails.public_path[.join(...)]` replacement preserving the
//!   remaining args by source. `::Rails` keeps its `::` prefix via the
//!   captured const source. Upstream ships `Enabled: pending`; Murphy maps
//!   that to `default_enabled = false`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RootPublicPath;

#[cop(
    name = "Rails/RootPublicPath",
    description = "Favor `Rails.public_path` over `Rails.root` with `'public'`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RootPublicPath {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[join]`.
    #[on_node(kind = "send", methods = ["join"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    // Upstream pattern `(send (send $(const {nil? cbase} :Rails) :root) :join ...)`:
    // the receiver must be a zero-arg `Rails.root`.
    let Some(rails_const) = rails_root_const(cx, recv) else {
        return;
    };
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return;
    };
    // Upstream `(str $#public_path?)` — only str first args can match.
    let NodeKind::Str(id) = *cx.kind(first) else {
        return;
    };
    let value = cx.string_str(id).to_owned();
    let Some(stripped) = strip_public_prefix(&value) else {
        return;
    };
    // The arena send range covers an attached block; upstream reports and
    // corrects the `join` send alone.
    let send_range = send_only_range(cx, node);
    cx.emit_offense(send_range, "Use `Rails.public_path`.", None);
    let rails_src = cx.raw_source(cx.range(rails_const)).to_owned();
    let mut rest: Vec<String> = Vec::new();
    if !stripped.is_empty() {
        rest.push(format!("'{stripped}'"));
    }
    for &arg in &args[1..] {
        rest.push(cx.raw_source(cx.range(arg)).to_owned());
    }
    let mut replacement = format!("{rails_src}.public_path");
    if !rest.is_empty() {
        replacement.push_str(&format!(".join({})", rest.join(", ")));
    }
    cx.emit_edit(send_range, &replacement);
}


/// Send-only range: the arena send range covers an attached block, while
/// upstream's offense and correction span the `join` send alone.
fn send_only_range(cx: &Cx<'_>, node: NodeId) -> murphy_plugin_api::Range {
    let range = cx.range(node);
    if cx.block_node(node).get().is_none() {
        return range;
    }
    if cx.is_parenthesized(node) {
        if let Some(paren_end) = matching_close_paren(cx, node) {
            return murphy_plugin_api::Range {
                start: range.start,
                end: paren_end,
            };
        }
        return range;
    }
    match cx.call_arguments(node).last() {
        Some(&last) => murphy_plugin_api::Range {
            start: range.start,
            end: cx.range(last).end,
        },
        None => murphy_plugin_api::Range {
            start: range.start,
            end: cx.loc(node).name.end,
        },
    }
}

/// Closing paren matching the call's open paren. Depth-counted so nested
/// calls resolve to the outer `)`.
fn matching_close_paren(cx: &Cx<'_>, node: NodeId) -> Option<u32> {
    let toks = cx.tokens_in(cx.range(node));
    let sel_end = cx.selector(node).end;
    let open = toks
        .iter()
        .position(|t| t.kind == SourceTokenKind::LeftParen && t.range.start >= sel_end)?;
    let mut depth = 0i32;
    for tok in &toks[open..] {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    None
}

/// Zero-arg `Rails`/`::Rails` `.root` receiver; returns the const node whose
/// source (`Rails` / `::Rails`) seeds the replacement.
fn rails_root_const(cx: &Cx<'_>, id: NodeId) -> Option<NodeId> {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(id) != Some("root") {
        return None;
    }
    if !cx.call_arguments(id).is_empty() {
        return None;
    }
    let recv = cx.call_receiver(id).get()?;
    if cx.const_name(recv).as_deref() != Some("Rails") {
        return None;
    }
    Some(recv)
}

/// Upstream `%r{\Apublic(/|\z)}` match plus `gsub` strip: `'public'` maps
/// to `""`, `'public/...'` maps to the remainder, anything else misses.
fn strip_public_prefix(value: &str) -> Option<String> {
    if value == "public" {
        return Some(String::new());
    }
    value.strip_prefix("public/").map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::RootPublicPath;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_public() {
        test::<RootPublicPath>().expect_correction(
            indoc! {r#"
                Rails.root.join('public')
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path`.
            "#},
            "Rails.public_path\n",
        );
    }

    #[test]
    fn flags_public_slash_path() {
        test::<RootPublicPath>().expect_correction(
            indoc! {r#"
                Rails.root.join('public/file.pdf')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path`.
            "#},
            "Rails.public_path.join('file.pdf')\n",
        );
    }

    #[test]
    fn flags_public_plus_arg() {
        test::<RootPublicPath>().expect_correction(
            indoc! {r#"
                Rails.root.join('public', 'file.pdf')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path`.
            "#},
            "Rails.public_path.join('file.pdf')\n",
        );
    }

    #[test]
    fn keeps_cbase_prefix() {
        test::<RootPublicPath>().expect_correction(
            indoc! {r#"
                ::Rails.root.join('public', 'x')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path`.
            "#},
            "::Rails.public_path.join('x')\n",
        );
    }

    #[test]
    fn keeps_block_with_send_only_offense() {
        test::<RootPublicPath>().expect_correction(
            indoc! {r#"
                Rails.root.join('public', 'x') { |y| puts y }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.public_path`.
            "#},
            "Rails.public_path.join('x') { |y| puts y }\n",
        );
    }

    #[test]
    fn allows_publicity() {
        test::<RootPublicPath>().expect_no_offenses("Rails.root.join('publicity')\n");
    }

    #[test]
    fn allows_sym_public() {
        test::<RootPublicPath>().expect_no_offenses("Rails.root.join(:public)\n");
    }

    #[test]
    fn allows_non_root_join() {
        test::<RootPublicPath>().expect_no_offenses("foo.join('public')\n");
    }

    #[test]
    fn allows_public_path_join() {
        test::<RootPublicPath>()
            .expect_no_offenses("Rails.public_path.join('file.pdf')\n");
    }
}
murphy_plugin_api::submit_cop!(RootPublicPath);
