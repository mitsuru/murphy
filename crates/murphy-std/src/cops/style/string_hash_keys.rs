//! `Style/StringHashKeys` — prefer symbols instead of strings as hash keys.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringHashKeys
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Enabled: false by default (matches RuboCop). Safe: false (unsafe cop) —
//!   Murphy expresses this via default_enabled = false only; the plugin ABI
//!   has no separate Safe flag (no-op: the cop won't fire unless users opt-in).
//!
//!   receive_environments_method? exclusions are implemented: gsub/gsub!,
//!   Kernel/nil.spawn/system, IO.popen, Open3.{capture*,popen*}, and
//!   Open3.{pipeline*} (three ancestor levels: pair -> hash -> arg-of-hash -> call).
//!
//!   valid_encoding? is a no-op in Murphy: string content arrives as valid UTF-8
//!   from prism, so no check is needed.
//!
//!   Autocorrect: string key -> symbol using Ruby's inspect convention:
//!   simple identifier (alphanumeric + _ + optional trailing ?/!) -> :name
//!   otherwise -> :"<key_content>" (double-quoted, with embedded \ and " escaped).
//! ```
//!
//! Subscribes to `NodeKind::Pair` and fires on pairs whose key is a plain string
//! (`NodeKind::Str`). Skips pairs that are arguments to environment-accepting
//! methods.

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct StringHashKeys;

const MSG: &str = "Prefer symbols instead of strings as hash keys.";

#[cop(
    name = "Style/StringHashKeys",
    description = "Prefer symbols instead of strings as hash keys.",
    default_severity = "warning",
    default_enabled = false
)]
impl StringHashKeys {
    #[on_node(kind = "pair")]
    fn check_pair(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Pair { key, .. } = *cx.kind(node) else {
            return;
        };
        // Only flag when the key is a plain string literal.
        let NodeKind::Str(_) = *cx.kind(key) else {
            return;
        };

        // Check exclusions: environments-accepting methods.
        if receive_environments_method(node, cx) {
            return;
        }

        let key_range = cx.range(key);

        // Always emit the offense: string keys are always a style violation.
        cx.emit_offense(key_range, MSG, None);

        // Autocorrect is best-effort. parse_string_content returns the string
        // body as a &str slice plus a `safe` flag. If safe is false (the body
        // contains backslash escapes), we skip the edit to avoid producing a
        // symbol with a different value (e.g. :a\n vs :"a" + newline).
        let raw = cx.raw_source(key_range);
        if let Some((body, safe)) = parse_string_content(raw)
            && safe {
                let symbol_text = symbol_inspect(body);
                cx.emit_edit(key_range, &symbol_text);
            }
    }
}

/// Returns `true` if the pair is inside a call that conventionally accepts
/// an environment hash (and thus string keys are required).
///
/// Mirrors RuboCop's `receive_environments_method?` node matcher.
fn receive_environments_method(pair: NodeId, cx: &Cx<'_>) -> bool {
    // pair -> hash -> send
    let Some(hash_node) = cx.parent(pair).get() else {
        return false;
    };
    let Some(send_node) = cx.parent(hash_node).get() else {
        return false;
    };

    // gsub / gsub! with any receiver
    if is_send_named(send_node, cx, None, &["gsub", "gsub!"]) {
        return true;
    }

    // Kernel/nil.spawn / Kernel/nil.system
    if is_send_named(
        send_node,
        cx,
        Some(ReceiverFilter::KernelOrNil),
        &["spawn", "system"],
    ) {
        return true;
    }

    // IO.popen
    if is_send_named(send_node, cx, Some(ReceiverFilter::Const("IO")), &["popen"]) {
        return true;
    }

    // Open3.{capture2,capture2e,capture3,popen2,popen2e,popen3}
    if is_send_named(
        send_node,
        cx,
        Some(ReceiverFilter::Const("Open3")),
        &[
            "capture2",
            "capture2e",
            "capture3",
            "popen2",
            "popen2e",
            "popen3",
        ],
    ) {
        return true;
    }

    // Open3.{pipeline,pipeline_r,pipeline_rw,pipeline_start,pipeline_w}
    // These are `^^^` in RuboCop -- the env hash is one level deeper:
    // pair -> hash -> array -> arg-of-send
    let Some(outer_send_node) = cx.parent(send_node).get() else {
        return false;
    };
    if is_send_named(
        outer_send_node,
        cx,
        Some(ReceiverFilter::Const("Open3")),
        &[
            "pipeline",
            "pipeline_r",
            "pipeline_rw",
            "pipeline_start",
            "pipeline_w",
        ],
    ) {
        return true;
    }

    false
}

#[derive(Clone, Copy)]
enum ReceiverFilter<'a> {
    KernelOrNil,
    Const(&'a str),
}

fn is_send_named(
    node: NodeId,
    cx: &Cx<'_>,
    receiver_filter: Option<ReceiverFilter<'_>>,
    methods: &[&str],
) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return false;
    };
    let method_name = cx.symbol_str(method);
    if !methods.contains(&method_name) {
        return false;
    }
    match receiver_filter {
        None => true,
        Some(ReceiverFilter::KernelOrNil) => match receiver.get() {
            None => true, // nil receiver (implicit)
            Some(recv) => {
                // Kernel (with or without cbase)
                matches!(*cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Kernel")
            }
        },
        Some(ReceiverFilter::Const(name)) => match receiver.get() {
            None => false,
            Some(recv) => {
                matches!(*cx.kind(recv), NodeKind::Const { name: n, .. } if cx.symbol_str(n) == name)
            }
        },
    }
}

/// Extracts the string body and an autocorrect-safety flag from `'...'` or `"..."`.
///
/// Returns `Some((body, safe))` where:
/// - `body` is the slice between the surrounding quotes (UTF-8 clean).
/// - `safe` is `true` when the body contains no backslash escapes at all;
///   `false` suppresses the autocorrect edit to avoid emitting a symbol whose
///   name differs from the runtime string value.
///
/// Returns `None` for heredocs, `%q`, char literals, or malformed sources.
fn parse_string_content(src: &str) -> Option<(&str, bool)> {
    let bytes = src.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let (quote, body) = match bytes[0] {
        b'\'' => (b'\'', &src[1..src.len() - 1]),
        b'"' => (b'"', &src[1..src.len() - 1]),
        _ => return None,
    };
    if bytes[bytes.len() - 1] != quote {
        return None;
    }
    // Any backslash in the body means we cannot safely use the raw source bytes
    // as the symbol name. Single-quoted \\=literal-backslash and \'=single-quote
    // are valid escapes but produce a name containing a backslash, which itself
    // would need quoting differently. Double-quoted bodies may have \n, \t, etc.
    // In both cases, suppressing the edit is the safest policy.
    let safe = !body.contains('\\');
    Some((body, safe))
}

/// Converts a string content to its Ruby symbol inspect representation.
///
/// Mirrors Ruby's `Symbol#inspect`:
/// - Simple symbols (identifier with optional trailing `?` or `!`) -> `:name`
/// - Everything else -> `:"escaped_content"`
pub(crate) fn symbol_inspect(content: &str) -> String {
    if is_simple_symbol(content) {
        format!(":{content}")
    } else {
        let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
        format!(":\"{escaped}\"")
    }
}

/// Returns `true` if the symbol name needs no quoting in Ruby symbol literal form.
fn is_simple_symbol(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let (first, rest) = bytes.split_first().unwrap();
    if !matches!(first, b'a'..=b'z' | b'A'..=b'Z' | b'_') {
        return false;
    }
    let body = match rest.last() {
        Some(b'?' | b'!') => &rest[..rest.len() - 1],
        _ => rest,
    };
    body.iter()
        .all(|&b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- symbol_inspect helpers ---

    #[test]
    fn symbol_inspect_simple_identifier() {
        assert_eq!(symbol_inspect("foo"), ":foo");
    }

    #[test]
    fn symbol_inspect_predicate_trailing_question() {
        assert_eq!(symbol_inspect("valid?"), ":valid?");
    }

    #[test]
    fn symbol_inspect_bang_trailing() {
        assert_eq!(symbol_inspect("foo!"), ":foo!");
    }

    #[test]
    fn symbol_inspect_spaces_uses_quoted_form() {
        assert_eq!(symbol_inspect("foo bar"), ":\"foo bar\"");
    }

    #[test]
    fn symbol_inspect_hyphens_uses_quoted_form() {
        assert_eq!(symbol_inspect("foo-bar"), ":\"foo-bar\"");
    }

    #[test]
    fn symbol_inspect_empty_uses_quoted_form() {
        assert_eq!(symbol_inspect(""), ":\"\"");
    }

    #[test]
    fn symbol_inspect_double_quote_in_content_is_escaped() {
        assert_eq!(symbol_inspect("say \"hi\""), ":\"say \\\"hi\\\"\"");
    }

    // --- offense detection ---

    #[test]
    fn flags_single_quoted_string_hash_key() {
        test::<StringHashKeys>().expect_offense(indoc! {r#"
            x = { 'one' => 1 }
                  ^^^^^ Prefer symbols instead of strings as hash keys.
        "#});
    }

    #[test]
    fn flags_double_quoted_string_hash_key() {
        test::<StringHashKeys>().expect_offense(indoc! {r#"
            x = { "two" => 2 }
                  ^^^^^ Prefer symbols instead of strings as hash keys.
        "#});
    }

    #[test]
    fn flags_multiple_string_keys() {
        test::<StringHashKeys>().expect_offense(indoc! {r#"
            x = { 'one' => 1, 'two' => 2 }
                  ^^^^^ Prefer symbols instead of strings as hash keys.
                              ^^^^^ Prefer symbols instead of strings as hash keys.
        "#});
    }

    #[test]
    fn no_offense_for_symbol_key() {
        test::<StringHashKeys>().expect_no_offenses("x = { one: 1 }\n");
    }

    #[test]
    fn no_offense_for_rocket_symbol_key() {
        test::<StringHashKeys>().expect_no_offenses("x = { :one => 1 }\n");
    }

    // --- autocorrect ---

    #[test]
    fn corrects_simple_identifier_key_to_symbol() {
        test::<StringHashKeys>().expect_correction(
            indoc! {r#"
                x = { 'foo' => 1 }
                      ^^^^^ Prefer symbols instead of strings as hash keys.
            "#},
            "x = { :foo => 1 }\n",
        );
    }

    #[test]
    fn corrects_double_quoted_key_to_symbol() {
        test::<StringHashKeys>().expect_correction(
            indoc! {r#"
                x = { "foo" => 1 }
                      ^^^^^ Prefer symbols instead of strings as hash keys.
            "#},
            "x = { :foo => 1 }\n",
        );
    }

    #[test]
    fn corrects_key_with_spaces_to_quoted_symbol() {
        test::<StringHashKeys>().expect_correction(
            indoc! {r#"
                x = { 'foo bar' => 1 }
                      ^^^^^^^^^ Prefer symbols instead of strings as hash keys.
            "#},
            "x = { :\"foo bar\" => 1 }\n",
        );
    }

    #[test]
    fn corrects_key_with_hyphens_to_quoted_symbol() {
        test::<StringHashKeys>().expect_correction(
            indoc! {r#"
                x = { 'foo-bar' => 1 }
                      ^^^^^^^^^ Prefer symbols instead of strings as hash keys.
            "#},
            "x = { :\"foo-bar\" => 1 }\n",
        );
    }

    // --- exclusions: receive_environments_method? ---

    #[test]
    fn no_offense_for_gsub_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("x = 'hello'.gsub('hello', 'one' => 'two')\n");
    }

    #[test]
    fn no_offense_for_system_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("system('ls', 'HOME' => '/tmp')\n");
    }

    #[test]
    fn no_offense_for_spawn_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("spawn('ls', 'HOME' => '/tmp')\n");
    }

    #[test]
    fn no_offense_for_io_popen_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("IO.popen('ls', 'HOME' => '/tmp')\n");
    }

    #[test]
    fn no_offense_for_open3_capture2_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("Open3.capture2('ls', 'HOME' => '/tmp')\n");
    }

    #[test]
    fn no_offense_for_open3_popen3_with_string_hash() {
        test::<StringHashKeys>().expect_no_offenses("Open3.popen3('ls', 'HOME' => '/tmp')\n");
    }
    #[test]
    fn parse_string_content_simple_returns_safe() {
        // "foo" -> body "foo", safe=true (no backslashes)
        let (body, safe) = parse_string_content(r#""foo""#).unwrap();
        assert_eq!(body, "foo");
        assert!(safe);
    }

    #[test]
    fn parse_string_content_with_backslash_returns_unsafe() {
        // A double-quoted body containing a backslash should be unsafe.
        // Feed the Ruby source as a raw string to avoid Rust escape issues.
        let result = parse_string_content(r#""a\n""#);
        assert!(result.is_some());
        let (_, safe) = result.unwrap();
        assert!(!safe, "body with backslash escape should be unsafe");
    }

    #[test]
    fn parse_string_content_single_quoted_no_backslash_is_safe() {
        let (body, safe) = parse_string_content("'hello'").unwrap();
        assert_eq!(body, "hello");
        assert!(safe);
    }

    #[test]
    fn parse_string_content_returns_none_for_non_string() {
        assert!(parse_string_content(": foo").is_none()); // not a quote-delimited string
        assert!(parse_string_content("x").is_none()); // too short
    }
}
murphy_plugin_api::submit_cop!(StringHashKeys);
