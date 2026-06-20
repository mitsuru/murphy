//! `Security/Open` — flag `Kernel#open` / `URI.open` with an argument that
//! could start with a pipe (command injection risk).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/Open
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `def_node_matcher :open?`:
//!   `(send ${nil? (const {nil? cbase} :URI)} :open $_ ...)`. The cop fires on
//!   `open(x)` (implicit receiver → `Kernel#open`) and `URI.open(x)` /
//!   `::URI.open(x)`. `is_global_const(receiver, "URI")` matches both `URI`
//!   and `::URI` (Murphy normalises `::URI` to a scope-less `Const`).
//!   The first argument is checked by RuboCop's `safe?`:
//!     - `str` literal: safe iff the *decoded* content is non-empty AND does
//!       not start with `|` (`safe_argument?`). Read via `cx.string_str`, not
//!       `raw_source`, so the quote delimiters do not leak into the check.
//!     - `dstr` (interpolation) OR `str + x` concatenation (`composite_string?`):
//!       recurse into `children.first` — the first dstr segment, or the `+`
//!       receiver. So `"foo#{x}"` / `"foo" + x` are safe, while `"#{x}"`
//!       (first child is an interpolation `begin`), `"|#{x}"`, and `x + "foo"`
//!       (first child is a non-str send) are flagged.
//!     - anything else (variables, method calls, `x + "foo"`): flagged.
//!   The offense highlights the `open` selector (`loc.name`), matching
//!   `node.loc.selector`. Parenthesised arguments are NOT unwrapped, matching
//!   RuboCop (`open(("foo"))` is a `begin` → flagged). No autocorrect (parity).
//!   NOTE: `IO.read`/`IO.write`/etc. belong to the separate `Security/IoMethods`
//!   cop, not this one — deliberately out of scope here.
//! ```
//!
//! ## Matched shapes
//!
//! - **Implicit receiver**: `open(something)` → `Kernel#open`
//! - **`URI` receiver**: `URI.open(something)` / `::URI.open(something)`
//!
//! ## Accepted (not flagged)
//!
//! - `open("foo")` — non-empty string literal not starting with `|`
//! - `open("foo#{x}")` — first dstr segment is a safe string literal
//! - `open("foo" + x)` — concatenation receiver is a safe string literal
//! - `File.open(something)` — receiver is not `nil`/`URI`
//!
//! ## Message
//!
//! `` The use of `Kernel#open` is a serious security risk. `` (implicit) or
//! `` The use of `URI.open` is a serious security risk. `` (matches RuboCop).

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct Open;

#[cop(
    name = "Security/Open",
    description = "The use of Kernel#open and URI.open represent a serious security risk.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Open {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) != Some("open") {
            return;
        }
        // `${nil? (const {nil? cbase} :URI)}` — receiver must be implicit
        // (`Kernel#open`) or the `URI` constant.
        let receiver = cx.call_receiver(node).get();
        let receiver_label = match receiver {
            None => "Kernel#".to_owned(),
            Some(recv) if cx.is_global_const(recv, "URI") => {
                format!("{}.", cx.raw_source(cx.range(recv)))
            }
            Some(_) => return,
        };
        // `$_` — the first positional argument is captured and checked.
        let args = cx.call_arguments(node);
        let Some(&code) = args.first() else {
            return;
        };
        if is_safe_argument(code, cx) {
            return;
        }
        let message = format!("The use of `{receiver_label}open` is a serious security risk.");
        cx.emit_offense(cx.node(node).loc.name, &message, None);
    }
}

/// Mirrors RuboCop's `safe?`: a string literal is safe when its decoded
/// content is non-empty and does not begin with a pipe; a composite string
/// (`dstr` or `str + x`) recurses into its first child; everything else is
/// unsafe (flagged).
fn is_safe_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        // `simple_string?` → `safe_argument?(node.str_content)`.
        NodeKind::Str(sid) => {
            let content = cx.string_str(sid);
            !content.is_empty() && !content.starts_with('|')
        }
        // `interpolated_string?` → recurse into `children.first` (first segment).
        NodeKind::Dstr(list) => match cx.list(list).first() {
            Some(&first) => is_safe_argument(first, cx),
            None => false,
        },
        // `concatenated_string?` → `node.method?(:+) && node.receiver.str_type?`,
        // recursing into `children.first` (the receiver).
        NodeKind::Send { .. } if cx.method_name(node) == Some("+") => {
            match cx.call_receiver(node).get() {
                Some(recv) if matches!(*cx.kind(recv), NodeKind::Str(_)) => {
                    is_safe_argument(recv, cx)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(Open);

#[cfg(test)]
mod tests {
    use super::Open;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_implicit_receiver_variable() {
        test::<Open>().expect_offense(indoc! {r#"
            open(foo)
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_uri_open() {
        test::<Open>().expect_offense(indoc! {r#"
            URI.open(foo)
                ^^^^ The use of `URI.open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_cbase_uri_open() {
        test::<Open>().expect_offense(indoc! {r#"
            ::URI.open(foo)
                  ^^^^ The use of `::URI.open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_empty_string() {
        test::<Open>().expect_offense(indoc! {r#"
            open("")
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_pipe_prefixed_string() {
        test::<Open>().expect_offense(indoc! {r#"
            open("|cmd")
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_pipe_prefixed_interpolation() {
        test::<Open>().expect_offense(indoc! {r##"
            open("|#{cmd}")
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "##});
    }

    #[test]
    fn flags_leading_interpolation() {
        // First dstr child is an interpolation `begin`, not a `str` → unsafe.
        test::<Open>().expect_offense(indoc! {r##"
            open("#{cmd}")
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "##});
    }

    #[test]
    fn flags_variable_concatenation() {
        // `x + "foo"` — first child is a non-str send → unsafe.
        test::<Open>().expect_offense(indoc! {r#"
            open(x + "foo")
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_chained_concatenation() {
        // `"a" + "b" + c` — outer `+` receiver is a `send` (not `str`), so
        // `concatenated_string?` is false → unsafe. Pins the chained-concat
        // boundary (RuboCop requires `node.receiver.str_type?`).
        test::<Open>().expect_offense(indoc! {r#"
            open("a" + "b" + c)
            ^^^^ The use of `Kernel#open` is a serious security risk.
        "#});
    }

    #[test]
    fn accepts_string_literal() {
        test::<Open>().expect_no_offenses("open(\"foo\")\n");
    }

    #[test]
    fn accepts_leading_string_interpolation() {
        // `"foo#{x}"` — first dstr segment is a safe string literal.
        test::<Open>().expect_no_offenses("open(\"foo#{x}\")\n");
    }

    #[test]
    fn accepts_string_concatenation() {
        // `"foo" + x` — concatenation receiver is a safe string literal.
        test::<Open>().expect_no_offenses("open(\"foo\" + x)\n");
    }

    #[test]
    fn accepts_uri_open_string_literal() {
        test::<Open>().expect_no_offenses("URI.open(\"https://example.com\")\n");
    }

    #[test]
    fn accepts_other_receiver() {
        // `File.open` — receiver is not `nil`/`URI`. Key no-FP guard.
        test::<Open>().expect_no_offenses("File.open(foo)\n");
    }

    #[test]
    fn accepts_non_open_method() {
        test::<Open>().expect_no_offenses("read(foo)\n");
    }

    #[test]
    fn accepts_open_no_arguments() {
        test::<Open>().expect_no_offenses("open\n");
    }
}
