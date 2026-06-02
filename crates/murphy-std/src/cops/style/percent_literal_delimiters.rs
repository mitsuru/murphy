//! `Style/PercentLiteralDelimiters` — enforces consistent delimiters for
//! `%`-literal expressions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PercentLiteralDelimiters
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the common case: subscribe to array/%w/%W/%i/%I, regexp/%r,
//!   str/%/%Q/%q, sym/%s, and xstr/%x nodes; extract the delimiter from raw
//!   source; compare with the configured preferred delimiter; emit offense
//!   and autocorrect.
//!
//!   Implemented guards (mirroring RuboCop):
//!   - uses_preferred_delimiter?: skip if opening delimiter already matches.
//!   - contains_preferred_delimiter?: skip if literal body contains the
//!     preferred opening or closing delimiter character.
//!   - include_same_character_as_used_for_delimiter?: skip if %w/%i/%W/%I
//!     literal body contains the current opening/closing delimiter characters
//!     (only applies when the current delimiter is a matchpair).
//!
//!   Per-type PreferredDelimiters config is supported via hand-rolled
//!   CopOptions; the 'default' key sets the baseline for any unspecified type.
//!
//!   Known gap: bare `%<delim>...` (equivalent to `%Q`) is treated as type
//!   `%Q` for preferred-delimiter lookup (bare `%` falls back to 'default').
//! ```

use std::collections::BTreeMap;

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Default preferred delimiters, mirroring murphy-std's `config/default.yml`.
fn default_delimiters() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("default".to_string(), "()".to_string());
    m.insert("%i".to_string(), "[]".to_string());
    m.insert("%I".to_string(), "[]".to_string());
    m.insert("%r".to_string(), "{}".to_string());
    m.insert("%w".to_string(), "[]".to_string());
    m.insert("%W".to_string(), "[]".to_string());
    m
}

/// Cop options for [`PercentLiteralDelimiters`].
///
/// `PreferredDelimiters` is a `String → String` map (type → 2-char pair).
/// This requires a hand-rolled impl because `#[derive(CopOptions)]` does not
/// model nested maps.
#[derive(Clone, Debug)]
pub struct PercentLiteralDelimitersOptions {
    /// Map from `%`-literal type (e.g. `"%w"`, `"default"`) to a 2-char
    /// string `"<open><close>"` (e.g. `"[]"`, `"()"`, `"{}"`).
    pub preferred_delimiters: BTreeMap<String, String>,
}

impl Default for PercentLiteralDelimitersOptions {
    fn default() -> Self {
        Self {
            preferred_delimiters: default_delimiters(),
        }
    }
}

impl CopOptions for PercentLiteralDelimitersOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let Some(delims_value) = obj.get("PreferredDelimiters") else {
            return Ok(Self::default());
        };
        let delims_obj = delims_value
            .as_object()
            .ok_or_else(|| ConfigError::type_mismatch("PreferredDelimiters", "object"))?;

        // Start from defaults and overlay the user-specified entries, so that
        // a partial PreferredDelimiters config (e.g. only setting "%q") does not
        // lose the defaults for unspecified types.
        let mut map = default_delimiters();
        for (key, val) in delims_obj {
            let s = val.as_str().ok_or_else(|| {
                ConfigError::type_mismatch(
                    format!("PreferredDelimiters.{key}"),
                    "string",
                )
            })?;
            map.insert(key.clone(), s.to_string());
        }
        Ok(Self {
            preferred_delimiters: map,
        })
    }

    fn to_config_json(&self) -> String {
        let pairs: serde_json::Map<String, serde_json::Value> = self
            .preferred_delimiters
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let mut top = serde_json::Map::new();
        top.insert(
            "PreferredDelimiters".to_string(),
            serde_json::Value::Object(pairs),
        );
        serde_json::Value::Object(top).to_string()
    }
}

/// Stateless unit struct.
#[derive(Default)]
pub struct PercentLiteralDelimiters;

#[cop(
    name = "Style/PercentLiteralDelimiters",
    description = "Use `%`-literal delimiters consistently.",
    default_severity = "warning",
    default_enabled = true,
    options = PercentLiteralDelimitersOptions,
)]
impl PercentLiteralDelimiters {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip str nodes that are inner parts of a regexp or xstr — those
        // nodes inherit the outer node's range and would double-report.
        if cx.parent(node).get().is_some_and(|p| {
            matches!(cx.kind(p), NodeKind::Regexp { .. } | NodeKind::Xstr(_))
        }) {
            return;
        }
        check(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip dstr nodes inside regexp — they are parts, not top-level.
        if cx.parent(node).get().is_some_and(|p| {
            matches!(cx.kind(p), NodeKind::Regexp { .. })
        }) {
            return;
        }
        check(node, cx);
    }

    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the percent-literal type prefix (e.g. `%w`, `%i`, `%r`) from the
/// raw source, or `None` if the source is not a `%`-literal.
fn percent_type(src: &str) -> Option<&str> {
    if !src.starts_with('%') {
        return None;
    }
    let bytes = src.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let second = bytes[1] as char;
    if second.is_alphabetic() {
        // %w, %W, %i, %I, %q, %Q, %r, %s, %x — 2-char prefix.
        Some(&src[..2])
    } else {
        // Bare %( or %[ etc — single-char type.
        Some("%")
    }
}

/// Returns (preferred_open, preferred_close) for a given literal type prefix.
fn preferred_delimiters_for(
    ty: &str,
    opts: &PercentLiteralDelimitersOptions,
) -> Option<(char, char)> {
    let pair_str = opts
        .preferred_delimiters
        .get(ty)
        .or_else(|| opts.preferred_delimiters.get("default"))?;
    let mut chars = pair_str.chars();
    let open = chars.next()?;
    let close = chars.next()?;
    Some((open, close))
}

/// Returns the matching close character for a bracket-like delimiter.
fn matchpair_close(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c => c,
    }
}

fn is_matchpair(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '<')
}

/// Extracts the element source texts from a %w/%W/%i/%I literal.
fn array_element_sources<'a>(node: NodeId, cx: &'a Cx<'_>) -> Vec<&'a str> {
    let NodeKind::Array(list) = *cx.kind(node) else {
        return vec![];
    };
    cx.list(list)
        .iter()
        .map(|&child_id| cx.raw_source(cx.range(child_id)))
        .collect()
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let src = cx.raw_source(cx.range(node));
    let Some(ty) = percent_type(src) else {
        return;
    };

    let opts = cx.options_or_default::<PercentLiteralDelimitersOptions>();
    let Some((pref_open, pref_close)) = preferred_delimiters_for(ty, &opts) else {
        return;
    };

    let prefix_len = ty.len();
    let bytes = src.as_bytes();
    if bytes.len() <= prefix_len {
        return;
    }
    let used_open = bytes[prefix_len] as char;
    let used_close = matchpair_close(used_open);

    // uses_preferred_delimiter? — already correct.
    if used_open == pref_open {
        return;
    }

    // Body is everything between the delimiters.
    if src.len() < prefix_len + 2 {
        return;
    }
    let body = &src[prefix_len + 1..src.len() - 1];

    // contains_preferred_delimiter? — body contains pref_open or pref_close.
    if body.contains(pref_open) || body.contains(pref_close) {
        return;
    }

    // include_same_character_as_used_for_delimiter? — only for %w/%W/%i/%I.
    let is_word_or_sym_array = matches!(ty, "%w" | "%W" | "%i" | "%I");
    if is_word_or_sym_array && is_matchpair(used_open) {
        let elem_srcs = array_element_sources(node, cx);
        if elem_srcs
            .iter()
            .any(|s| s.contains(used_open) || s.contains(used_close))
        {
            return;
        }
    }

    let message = format!(
        "`{ty}`-literals should be delimited by `{pref_open}` and `{pref_close}`."
    );
    let node_range = cx.range(node);
    cx.emit_offense(node_range, &message, None);

    // Autocorrect: replace opening delimiter char and closing delimiter char.
    //
    // Guard: if the body contains a backslash-escaped version of the current
    // delimiter (e.g. `\]` in `%q[foo\]bar]`), changing the delimiter
    // would leave the backslash in the output and alter the literal's value.
    // Skip autocorrect and let the user fix manually.
    let esc_open = [b'\\', used_open as u8];
    let esc_close = [b'\\', used_close as u8];
    if body
        .as_bytes()
        .windows(2)
        .any(|w| w == esc_open || w == esc_close)
    {
        return;
    }

    let open_char_range = Range {
        start: node_range.start + prefix_len as u32,
        end: node_range.start + prefix_len as u32 + 1,
    };
    let close_char_range = Range {
        start: node_range.end - 1,
        end: node_range.end,
    };
    cx.emit_edit(open_char_range, &pref_open.to_string());
    cx.emit_edit(close_char_range, &pref_close.to_string());
}

#[cfg(test)]
mod tests {
    use super::{PercentLiteralDelimiters, PercentLiteralDelimitersOptions, default_delimiters};
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::{ConfigErrorKind, CopOptions};

    fn opts_with(entries: &[(&str, &str)]) -> PercentLiteralDelimitersOptions {
        let mut map = default_delimiters();
        for (k, v) in entries {
            map.insert(k.to_string(), v.to_string());
        }
        PercentLiteralDelimitersOptions {
            preferred_delimiters: map,
        }
    }

    // ── %w ──────────────────────────────────────────────────────────────────

    #[test]
    fn flags_percent_w_with_wrong_delimiter() {
        test::<PercentLiteralDelimiters>().expect_offense(indoc! {r#"
            x = %w(alpha beta)
                ^^^^^^^^^^^^^^ `%w`-literals should be delimited by `[` and `]`.
        "#});
    }

    #[test]
    fn no_offense_percent_w_with_correct_delimiter() {
        test::<PercentLiteralDelimiters>().expect_no_offenses("x = %w[alpha beta]\n");
    }

    #[test]
    fn autocorrects_percent_w_parens_to_brackets() {
        test::<PercentLiteralDelimiters>().expect_correction(
            indoc! {r#"
                x = %w(alpha beta)
                    ^^^^^^^^^^^^^^ `%w`-literals should be delimited by `[` and `]`.
            "#},
            "x = %w[alpha beta]\n",
        );
    }

    #[test]
    fn no_offense_percent_w_body_contains_preferred_close() {
        // body contains ']' — cannot safely change to [] delimiter.
        test::<PercentLiteralDelimiters>().expect_no_offenses("x = %w(alpha]0 beta)\n");
    }

    // ── %i ──────────────────────────────────────────────────────────────────

    #[test]
    fn flags_percent_i_with_wrong_delimiter() {
        test::<PercentLiteralDelimiters>().expect_offense(indoc! {r#"
            x = %i(foo bar)
                ^^^^^^^^^^^ `%i`-literals should be delimited by `[` and `]`.
        "#});
    }

    #[test]
    fn no_offense_percent_i_with_correct_delimiter() {
        test::<PercentLiteralDelimiters>().expect_no_offenses("x = %i[foo bar]\n");
    }

    #[test]
    fn autocorrects_percent_i_parens_to_brackets() {
        test::<PercentLiteralDelimiters>().expect_correction(
            indoc! {r#"
                x = %i(foo bar)
                    ^^^^^^^^^^^ `%i`-literals should be delimited by `[` and `]`.
            "#},
            "x = %i[foo bar]\n",
        );
    }

    // ── %r ──────────────────────────────────────────────────────────────────

    #[test]
    fn flags_percent_r_with_wrong_delimiter() {
        test::<PercentLiteralDelimiters>().expect_offense(indoc! {r#"
            x = %r(foo)
                ^^^^^^^ `%r`-literals should be delimited by `{` and `}`.
        "#});
    }

    #[test]
    fn no_offense_percent_r_with_correct_delimiter() {
        test::<PercentLiteralDelimiters>().expect_no_offenses("x = %r{foo}\n");
    }

    #[test]
    fn autocorrects_percent_r_parens_to_braces() {
        test::<PercentLiteralDelimiters>().expect_correction(
            indoc! {r#"
                x = %r(foo)
                    ^^^^^^^ `%r`-literals should be delimited by `{` and `}`.
            "#},
            "x = %r{foo}\n",
        );
    }

    // ── %q ──────────────────────────────────────────────────────────────────

    #[test]
    fn no_offense_percent_q_uses_default_delimiters() {
        // default: %q looks up "default" → () and literal uses ()
        test::<PercentLiteralDelimiters>().expect_no_offenses("x = %q(foo)\n");
    }

    #[test]
    fn flags_percent_q_with_wrong_delimiter_when_custom_config() {
        test::<PercentLiteralDelimiters>()
            .with_options(&opts_with(&[("%q", "[]")]))
            .expect_offense(indoc! {r#"
                x = %q(foo)
                    ^^^^^^^ `%q`-literals should be delimited by `[` and `]`.
            "#});
    }

    // ── %x ──────────────────────────────────────────────────────────────────

    #[test]
    fn flags_percent_x_with_wrong_delimiter_when_custom_config() {
        test::<PercentLiteralDelimiters>()
            .with_options(&opts_with(&[("%x", "[]")]))
            .expect_offense(indoc! {r#"
                x = %x(ls)
                    ^^^^^^ `%x`-literals should be delimited by `[` and `]`.
            "#});
    }

    // ── escaped delimiter guard ──────────────────────────────────────────────

    #[test]
    fn no_autocorrect_when_body_has_escaped_delimiter() {
        // %w[foo\]bar] — body contains \] which is an escape for the ] delimiter.
        // Changing to () would leave \] in the body changing the literal's value.
        // Offense is still emitted but without autocorrect.
        test::<PercentLiteralDelimiters>()
            .with_options(&opts_with(&[("%w", "()")]))
            .expect_no_corrections("x = %w[foo\\]bar]
");
    }

    // ── CopOptions round-trip ────────────────────────────────────────────────

    #[test]
    fn cop_options_round_trip() {
        let opts = PercentLiteralDelimitersOptions::default();
        let json = opts.to_config_json();
        let decoded =
            <PercentLiteralDelimitersOptions as CopOptions>::from_config_json(json.as_bytes())
                .expect("round-trip should succeed");
        assert_eq!(decoded.preferred_delimiters, opts.preferred_delimiters);
    }

    #[test]
    fn cop_options_type_mismatch_not_object() {
        let err =
            <PercentLiteralDelimitersOptions as CopOptions>::from_config_json(b"\"bad\"")
                .expect_err("non-object root is invalid");
        let ConfigErrorKind::NotAnObject = err.kind() else {
            panic!("expected NotAnObject, got {:?}", err.kind());
        };
    }

    #[test]
    fn cop_options_type_mismatch_preferred_delimiters_not_object() {
        let err = <PercentLiteralDelimitersOptions as CopOptions>::from_config_json(
            br#"{"PreferredDelimiters":"bad"}"#,
        )
        .expect_err("non-object PreferredDelimiters is invalid");
        let ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredDelimiters");
        assert_eq!(*expected, "object");
    }

    #[test]
    fn cop_options_type_mismatch_entry_not_string() {
        let err = <PercentLiteralDelimitersOptions as CopOptions>::from_config_json(
            br#"{"PreferredDelimiters":{"%w":42}}"#,
        )
        .expect_err("non-string entry value is invalid");
        let ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredDelimiters.%w");
        assert_eq!(*expected, "string");
    }
}

murphy_plugin_api::submit_cop!(PercentLiteralDelimiters);
