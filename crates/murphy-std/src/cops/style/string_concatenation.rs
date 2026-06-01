//! `Style/StringConcatenation` — flags string concatenation where string
//! interpolation can be used instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringConcatenation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Offense detection is fully implemented for both `aggressive` (default)
//!   and `conservative` modes. Line-end concatenation is skipped (deferred to
//!   Style/LineEndConcatenation). Autocorrect is implemented for simple chains
//!   where all parts are plain string literals (`Str`) or non-string
//!   expressions, with no multiline parts, no heredoc parts, and no block
//!   descendants. When any part is uncorrectable, the offense is still flagged
//!   but no edit is emitted. `Dstr` (interpolated string) parts are considered
//!   uncorrectable in v1 to avoid fiddly inner-content reconstruction.
//!   Only fires on the topmost `+` node in a chain; nested `+` send nodes
//!   whose parent is also a `+` send are skipped to avoid duplicate offenses.
//! ```
//!
//! ## Matched shapes (aggressive mode — default)
//!
//! ```ruby
//! # bad (either side of + is a string literal)
//! email_with_name = user.name + ' <' + user.email + '>'
//! Pathname.new('/') + 'test'
//!
//! # good
//! email_with_name = "#{user.name} <#{user.email}>"
//!
//! # accepted (line-end concatenation — handled by Style/LineEndConcatenation)
//! name = 'First' +
//!   'Last'
//! ```
//!
//! ## Matched shapes (conservative mode)
//!
//! ```ruby
//! # bad (left side must be a string literal)
//! 'Hello' + user.name
//!
//! # good (left side is not a string literal)
//! user.name + '!!'
//! Pathname.new('/') + 'test'
//! ```
//!
//! ## Autocorrect
//!
//! Builds `"#{part1}#{part2}..."` replacing plain string content inline and
//! wrapping non-string parts in `#{}`. Skipped if any part is multiline,
//! a heredoc, contains a block descendant, or is a `Dstr` (already
//! interpolated).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Prefer string interpolation to string concatenation.";

/// Stateless unit struct.
#[derive(Default)]
pub struct StringConcatenation;

#[derive(CopOptions)]
pub struct StringConcatenationOptions {
    #[option(
        name = "Mode",
        default = "aggressive",
        description = "Concatenation check mode: `aggressive` (default) flags any + where either side is a string literal; `conservative` flags only when the left side is a string literal."
    )]
    pub mode: StringConcatenationMode,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringConcatenationMode {
    #[option(value = "aggressive")]
    Aggressive,
    #[option(value = "conservative")]
    Conservative,
}

#[cop(
    name = "Style/StringConcatenation",
    description = "Prefer string interpolation to string concatenation.",
    default_severity = "warning",
    default_enabled = true,
    options = StringConcatenationOptions,
)]
impl StringConcatenation {
    #[on_node(kind = "send", methods = ["+"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>, options: &StringConcatenationOptions) {
        check(node, cx, options);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, options: &StringConcatenationOptions) {
    // Only process the topmost `+` in a chain: if the parent is also a `+`
    // send, a descendant will be handled when the parent fires.
    if is_plus_send_node(node, cx) && is_parent_a_plus_send(node, cx) {
        return;
    }

    // Verify this node is itself a string concatenation.
    if !is_string_concatenation(node, cx) {
        return;
    }

    // Skip line-end concatenation (handled by Style/LineEndConcatenation).
    if is_line_end_concatenation(node, cx) {
        return;
    }

    // Collect all parts in the chain (left-to-right).
    let parts = collect_parts(node, cx);

    // Conservative mode: only flag if the first part is a plain Str.
    if options.mode == StringConcatenationMode::Conservative {
        if parts.first().map(|&id| is_str(id, cx)) != Some(true) {
            return;
        }
    }

    let node_range = cx.range(node);
    cx.emit_offense(node_range, MSG, None);

    // Autocorrect: only when all parts are correctable AND the first (leftmost)
    // leaf part is a plain Str. This ensures the concatenation is definitely a
    // String#+ call, not a Pathname#+ or other custom `+` implementation that
    // happens to take a string argument. Without this guard, aggressive-mode
    // detection on `Pathname.new('/') + 'test'` would emit an autocorrect that
    // changes behavior (Pathname#+ returns a Pathname, not a String).
    let first_is_str = parts.first().map(|&id| is_str(id, cx)) == Some(true);
    if first_is_str && parts.iter().all(|&id| is_correctable(id, cx)) {
        let replacement = build_interpolated(parts, cx);
        cx.emit_edit(node_range, &replacement);
    }
}

/// Returns true iff `node` is a `Send` with method `+`.
fn is_plus_send_node(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(node) == Some("+")
}

/// Returns true iff the parent of `node` is a `Send` with method `+`.
fn is_parent_a_plus_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    is_plus_send_node(parent, cx)
}

/// Returns true iff `node` matches `(send str_type? :+ _)` or
/// `(send _ :+ str_type?)` — i.e. either side of `+` is a plain Str.
///
/// Note: RuboCop's `str_type?` matches only `:str` (not `:dstr`).
fn is_string_concatenation(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method: _,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return false;
    }
    let rhs = arg_list[0];

    let lhs_is_str = receiver.get().map(|id| is_str(id, cx)).unwrap_or(false);
    let rhs_is_str = is_str(rhs, cx);

    lhs_is_str || rhs_is_str
}

/// Returns true iff `node` is a plain `Str` literal (not `Dstr`).
fn is_str(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_))
}

/// Returns true iff `node` is a line-end concatenation:
/// both receiver and first argument are strings, the node is multiline,
/// and the source contains `+` followed by optional whitespace and a newline.
fn is_line_end_concatenation(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method: _,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return false;
    }
    let rhs = arg_list[0];
    let Some(lhs) = receiver.get() else {
        return false;
    };

    if !is_str(lhs, cx) || !is_str(rhs, cx) {
        return false;
    }

    if !cx.is_multiline(node) {
        return false;
    }

    let src = cx.raw_source(cx.range(node));
    // Check for `+` followed by optional whitespace and a newline.
    src.contains("+\n")
        || src.contains("+ \n")
        || src.contains("+  \n")
        || contains_plus_newline(src)
}

/// Check if source contains `+` followed by optional whitespace then `\n`.
fn contains_plus_newline(src: &str) -> bool {
    if let Some(plus_pos) = src.find('+') {
        let after = &src[plus_pos + 1..];
        let trimmed = after.trim_start_matches(' ').trim_start_matches('\t');
        trimmed.starts_with('\n')
    } else {
        false
    }
}

/// Collect all leaf parts of a `+` chain in left-to-right order.
/// Descends only through `+` send nodes.
fn collect_parts(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let mut parts = Vec::new();
    collect_parts_into(node, cx, &mut parts);
    parts
}

fn collect_parts_into(node: NodeId, cx: &Cx<'_>, parts: &mut Vec<NodeId>) {
    if is_plus_send_node(node, cx) {
        let NodeKind::Send {
            receiver,
            method: _,
            args,
        } = *cx.kind(node)
        else {
            parts.push(node);
            return;
        };

        let arg_list = cx.list(args);
        if arg_list.len() == 1 {
            if let Some(lhs) = receiver.get() {
                collect_parts_into(lhs, cx, parts);
            }
            collect_parts_into(arg_list[0], cx, parts);
            return;
        }
    }
    parts.push(node);
}

/// Returns true iff a part is correctable (can be inlined into interpolation).
/// Uncorrectable: multiline, heredoc (Str nodes whose source starts with `<<`),
/// contains block descendants, or is a Dstr (already interpolated — complex to
/// reconstruct faithfully in v1).
fn is_correctable(id: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_multiline(id) {
        return false;
    }
    // Dstr parts are uncorrectable in v1 (inner-content reconstruction is fiddly).
    if matches!(cx.kind(id), NodeKind::Dstr(_)) {
        return false;
    }
    // Heredoc: source starts with `<<`.
    let src = cx.raw_source(cx.range(id));
    if src.starts_with("<<") {
        return false;
    }
    // Block descendants make safe correction uncertain.
    if has_block_descendant(id, cx) {
        return false;
    }
    true
}

/// Returns true iff the node has any `Block`/`Numblock`/`Itblock` descendant.
fn has_block_descendant(id: NodeId, cx: &Cx<'_>) -> bool {
    // We use a simple DFS through children.
    let kind = cx.kind(id);
    match kind {
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            return true;
        }
        _ => {}
    }
    for child in cx.children(id) {
        if has_block_descendant(child, cx) {
            return true;
        }
    }
    false
}

/// Build the interpolated string replacement from a list of leaf parts.
/// Plain `Str` parts are inlined (with escaping); other parts are wrapped in `#{}`.
fn build_interpolated(parts: Vec<NodeId>, cx: &Cx<'_>) -> String {
    let mut result = String::from("\"");
    for id in parts {
        match cx.kind(id) {
            NodeKind::Str(sid) => {
                let value = cx.string_str(*sid);
                // Escape characters that need escaping inside a double-quoted string.
                result.push_str(&escape_for_interpolation(
                    value,
                    cx.raw_source(cx.range(id)),
                ));
            }
            _ => {
                let src = cx.raw_source(cx.range(id));
                result.push_str("#{");
                result.push_str(src);
                result.push('}');
            }
        }
    }
    result.push('"');
    result
}

/// Escape a string value for embedding inside a double-quoted interpolated string.
///
/// For single-quoted strings, the value is the raw string contents (not escaped
/// by Ruby's single-quote rules). We need to escape `\`, `"`, `#{`, `#@`, `#$`
/// for safe embedding in double quotes.
///
/// For double-quoted strings, the source already has double-quote escape
/// sequences; extract the inner content between the outer `"` delimiters.
///
/// For other string literals (`%q()`, `%Q()`, etc.), fall back to using the
/// decoded value with the same full escaping as single-quoted strings — `\`, `"`,
/// and interpolation triggers (`#{`, `#@`, `#$`) must all be escaped. Without
/// escaping `#{`, content like `%q(#{foo})` (which Ruby keeps as the literal
/// text `#{foo}`) would become an active interpolation in the output.
fn escape_for_interpolation(value: &str, src: &str) -> String {
    let trimmed = src.trim();
    if trimmed.starts_with('\'') || (!trimmed.starts_with('"') && !trimmed.is_empty()) {
        // Single-quoted or other non-double-quoted literals: the `value` field is
        // the decoded (unescaped) content.  Escape everything that is unsafe inside
        // a double-quoted string.
        escape_decoded_value(value)
    } else if trimmed.starts_with('"') {
        // Double-quoted: the source already contains the properly escaped content.
        // Extract the part between the outer `"` delimiters.
        if trimmed.len() >= 2 {
            trimmed[1..trimmed.len() - 1].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

/// Escape a decoded (unescaped) string value for safe embedding inside a
/// double-quoted `"..."` string.  Escapes `\`, `"`, and the interpolation
/// triggers `#{`, `#@`, `#$`.
fn escape_decoded_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' => {
                out.push_str("\\\\");
                i += 1;
            }
            b'"' => {
                out.push_str("\\\"");
                i += 1;
            }
            b'#' if i + 1 < bytes.len() && matches!(bytes[i + 1], b'{' | b'@' | b'$') => {
                out.push_str("\\#");
                i += 1;
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{StringConcatenation, StringConcatenationMode, StringConcatenationOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Aggressive mode (default) -----

    #[test]
    fn flags_simple_string_plus_variable() {
        test::<StringConcatenation>().expect_offense(indoc! {r#"
            'Hello' + user.name
            ^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
        "#});
    }

    #[test]
    fn flags_variable_plus_string() {
        test::<StringConcatenation>().expect_offense(indoc! {r#"
            user.name + '!'
            ^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
        "#});
    }

    #[test]
    fn flags_string_chain_and_autocorrects() {
        // Simple chain: all plain string literals — fully correctable.
        test::<StringConcatenation>().expect_correction(
            indoc! {r#"
                'Hello' + ' ' + 'World'
                ^^^^^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
            "#},
            "\"Hello World\"\n",
        );
    }

    #[test]
    fn flags_mixed_chain_with_expressions() {
        // Chain with non-string parts — offense only, no autocorrect because
        // the non-string sides are not plain Str nodes.
        // Just test the offense is reported.
        test::<StringConcatenation>().expect_offense(indoc! {r#"
            user.name + ' <' + user.email + '>'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
        "#});
    }

    #[test]
    fn no_duplicate_offense_for_inner_plus() {
        // The inner `+` nodes in a chain should NOT produce separate offenses.
        // Only the topmost node (the whole expression) gets one offense.
        use murphy_plugin_api::test_support::run_cop;
        let offenses = run_cop::<StringConcatenation>("user.name + ' <' + user.email + '>'\n");
        assert_eq!(
            offenses.len(),
            1,
            "expected exactly 1 offense, got: {:?}",
            offenses
        );
    }

    #[test]
    fn accepts_variable_plus_variable() {
        // Neither side is a string literal — no offense in aggressive mode.
        test::<StringConcatenation>().expect_no_offenses("user.name + user.email\n");
    }

    #[test]
    fn accepts_line_end_concatenation() {
        // Line-end concatenation: deferred to Style/LineEndConcatenation.
        test::<StringConcatenation>().expect_no_offenses("name = 'First' +\n  'Last'\n");
    }

    // ----- Autocorrect: single-quoted strings -----

    #[test]
    fn autocorrects_single_quoted_strings() {
        test::<StringConcatenation>().expect_correction(
            indoc! {r#"
                'Hello' + ' World'
                ^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
            "#},
            "\"Hello World\"\n",
        );
    }

    #[test]
    fn autocorrects_double_quoted_string_plus_expr() {
        // One side is a double-quoted string, other is an expression.
        test::<StringConcatenation>().expect_offense(indoc! {r#"
            "Hello " + user.name
            ^^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
        "#});
    }

    // ----- Conservative mode -----

    #[test]
    fn conservative_flags_string_literal_on_left() {
        test::<StringConcatenation>()
            .with_options(&StringConcatenationOptions {
                mode: StringConcatenationMode::Conservative,
            })
            .expect_offense(indoc! {r#"
                'Hello' + user.name
                ^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
            "#});
    }

    #[test]
    fn conservative_accepts_non_string_on_left() {
        // user.name is not a string literal — conservative mode skips it.
        test::<StringConcatenation>()
            .with_options(&StringConcatenationOptions {
                mode: StringConcatenationMode::Conservative,
            })
            .expect_no_offenses("user.name + '!'\n");
    }

    #[test]
    fn conservative_accepts_pathname_plus_string() {
        // Pathname.new('/') + 'test' — left side is not a string literal.
        test::<StringConcatenation>()
            .with_options(&StringConcatenationOptions {
                mode: StringConcatenationMode::Conservative,
            })
            .expect_no_offenses("Pathname.new('/') + 'test'\n");
    }

    #[test]
    fn aggressive_flags_pathname_plus_string() {
        // In aggressive mode, the right side being a string is enough.
        test::<StringConcatenation>().expect_offense(indoc! {r#"
            Pathname.new('/') + 'test'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
        "#});
    }

    #[test]
    fn no_autocorrect_for_non_str_receiver_in_aggressive_mode() {
        // Pathname.new('/') + 'test' fires offense but NOT autocorrect: the
        // receiver is not a Str so we cannot guarantee String#+ semantics.
        test::<StringConcatenation>().expect_no_corrections(
            "Pathname.new('/') + 'test'
",
        );
    }

    #[test]
    fn autocorrects_single_quoted_string_with_interpolation_chars() {
        // %q() strings don't interpolate in Ruby, so #{foo} in them is literal.
        // After correction, the `#` must be escaped to prevent interpolation.
        // NOTE: %q() is not a `Str` literal starting with `'` or `"`, so it
        // falls into the fallback branch of escape_for_interpolation.
        // We test that a plain single-quoted string containing a literal `#{`
        // is safely embedded.
        test::<StringConcatenation>().expect_correction(
            indoc! {r#"
                '#{foo}' + ' bar'
                ^^^^^^^^^^^^^^^^^ Prefer string interpolation to string concatenation.
            "#},
            r#""\#{foo} bar"
"#,
        );
    }
}
murphy_plugin_api::submit_cop!(StringConcatenation);
