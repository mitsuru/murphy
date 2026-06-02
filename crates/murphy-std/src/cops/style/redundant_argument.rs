//! `Style/RedundantArgument` — flags calls that pass the default argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantArgument
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches upstream `Enabled: pending`).
//!   Default Methods map mirrors RuboCop's default.yml:
//!     join: ''
//!     sum: 0
//!     exit: true
//!     exit!: false
//!     to_i: 10
//!     split: ' '
//!     chomp: "\n"
//!     chomp!: "\n"
//!   Argument matching compares decoded values directly (str literal value vs
//!   configured string, int literal vs configured int, true/false vs configured
//!   bool). Non-literal args (variables, expressions) are not matched.
//!   `exit`/`exit!` are the only methods that work without a receiver; all
//!   other configured methods require a receiver to fire.
//!   Both `send` and `csend` are handled (mirrors RuboCop's `alias on_csend`).
//! ```

use std::collections::BTreeMap;

use murphy_plugin_api::{
    ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantArgument;

const MSG: &str = "Argument %<arg>s is redundant because it is implied by default.";

/// The configured default value for a method.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultArg {
    Str(String),
    Int(i64),
    Bool(bool),
}

/// Options for `Style/RedundantArgument`.
#[derive(Clone, Debug)]
pub struct RedundantArgumentOptions {
    pub methods: BTreeMap<String, DefaultArg>,
}

impl Default for RedundantArgumentOptions {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        map.insert("join".to_string(), DefaultArg::Str(String::new()));
        map.insert("sum".to_string(), DefaultArg::Int(0));
        map.insert("exit".to_string(), DefaultArg::Bool(true));
        map.insert("exit!".to_string(), DefaultArg::Bool(false));
        map.insert("to_i".to_string(), DefaultArg::Int(10));
        map.insert("split".to_string(), DefaultArg::Str(" ".to_string()));
        map.insert("chomp".to_string(), DefaultArg::Str("\n".to_string()));
        map.insert("chomp!".to_string(), DefaultArg::Str("\n".to_string()));
        Self { methods: map }
    }
}

impl CopOptions for RedundantArgumentOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let Some(methods_val) = obj.get("Methods") else {
            return Ok(Self::default());
        };

        let methods_obj = methods_val
            .as_object()
            .ok_or_else(|| ConfigError::type_mismatch("Methods", "object"))?;

        let mut map = BTreeMap::new();
        for (key, val) in methods_obj {
            let default_arg = match val {
                serde_json::Value::String(s) => DefaultArg::Str(s.clone()),
                serde_json::Value::Number(n) => {
                    let i = n.as_i64().ok_or_else(|| {
                        ConfigError::type_mismatch(format!("Methods.{key}"), "integer")
                    })?;
                    DefaultArg::Int(i)
                }
                serde_json::Value::Bool(b) => DefaultArg::Bool(*b),
                _ => {
                    return Err(ConfigError::type_mismatch(
                        format!("Methods.{key}"),
                        "string, integer, or boolean",
                    ));
                }
            };
            map.insert(key.clone(), default_arg);
        }

        Ok(Self { methods: map })
    }

    fn to_config_json(&self) -> String {
        let methods_map: serde_json::Map<String, serde_json::Value> = self
            .methods
            .iter()
            .map(|(k, v)| {
                let json_val = match v {
                    DefaultArg::Str(s) => serde_json::Value::String(s.clone()),
                    DefaultArg::Int(i) => {
                        serde_json::Value::Number(serde_json::Number::from(*i))
                    }
                    DefaultArg::Bool(b) => serde_json::Value::Bool(*b),
                };
                (k.clone(), json_val)
            })
            .collect();
        let mut top = serde_json::Map::new();
        top.insert(
            "Methods".to_string(),
            serde_json::Value::Object(methods_map),
        );
        serde_json::to_string(&top).unwrap_or_default()
    }
}

/// Methods allowed to fire without a receiver.
const NO_RECEIVER_METHODS: &[&str] = &["exit", "exit!"];

#[cop(
    name = "Style/RedundantArgument",
    description = "Checks for a redundant argument passed to certain methods.",
    default_severity = "warning",
    default_enabled = false,
    options = RedundantArgumentOptions,
)]
impl RedundantArgument {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m,
        None => return,
    };

    if cx.call_receiver(node).get().is_none() && !NO_RECEIVER_METHODS.contains(&method) {
        return;
    }

    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    let opts = cx.options_or_default::<RedundantArgumentOptions>();
    let Some(expected) = opts.methods.get(method) else {
        return;
    };

    let arg = args[0];
    if !arg_matches(arg, expected, cx) {
        return;
    }

    let arg_src = cx.raw_source(cx.range(arg));
    let msg = MSG.replace("%<arg>s", arg_src);

    let removal = removal_range(node, arg, cx);
    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(removal, "");
}

fn arg_matches(arg: NodeId, expected: &DefaultArg, cx: &Cx<'_>) -> bool {
    match (cx.kind(arg), expected) {
        (NodeKind::Str(sym), DefaultArg::Str(s)) => cx.string_str(*sym) == s.as_str(),
        (NodeKind::Int(n), DefaultArg::Int(i)) => n == i,
        (NodeKind::True_, DefaultArg::Bool(true)) => true,
        (NodeKind::False_, DefaultArg::Bool(false)) => true,
        _ => false,
    }
}

/// Compute the byte range to delete (autocorrect).
///
/// - Parenthesized: `array.join('')` → delete `('')`
/// - Unparenthesized: `exit true` → delete ` true`
fn removal_range(node: NodeId, arg: NodeId, cx: &Cx<'_>) -> Range {
    if cx.is_parenthesized(node) {
        let selector = cx.selector(node);
        let node_end = cx.range(node).end;
        let toks = cx.sorted_tokens();
        let idx = toks.partition_point(|t| t.range.start < selector.end);
        if let Some(lparen) = toks[idx..]
            .iter()
            .take_while(|t| t.range.start < node_end)
            .find(|t| t.kind == SourceTokenKind::LeftParen)
        {
            return Range {
                start: lparen.range.start,
                end: node_end,
            };
        }
        Range {
            start: cx.range(arg).start,
            end: node_end,
        }
    } else {
        let selector = cx.selector(node);
        Range {
            start: selector.end,
            end: cx.range(node).end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DefaultArg, RedundantArgument, RedundantArgumentOptions};
    use murphy_plugin_api::{CopOptions, test_support::{indoc, test}};

    #[test]
    fn flags_join_empty_double_quote() {
        test::<RedundantArgument>().expect_offense(indoc! {r#"
            array.join("")
            ^^^^^^^^^^^^^^ Argument "" is redundant because it is implied by default.
        "#});
    }

    #[test]
    fn corrects_join_empty_double_quote() {
        test::<RedundantArgument>().expect_correction(
            indoc! {r#"
                array.join("")
                ^^^^^^^^^^^^^^ Argument "" is redundant because it is implied by default.
            "#},
            "array.join\n",
        );
    }

    #[test]
    fn flags_join_empty_single_quote() {
        test::<RedundantArgument>().expect_offense(indoc! {r#"
            array.join('')
            ^^^^^^^^^^^^^^ Argument '' is redundant because it is implied by default.
        "#});
    }

    #[test]
    fn corrects_join_empty_single_quote() {
        test::<RedundantArgument>().expect_correction(
            indoc! {r#"
                array.join('')
                ^^^^^^^^^^^^^^ Argument '' is redundant because it is implied by default.
            "#},
            "array.join\n",
        );
    }

    #[test]
    fn flags_sum_zero() {
        test::<RedundantArgument>().expect_offense(indoc! {"
            array.sum(0)
            ^^^^^^^^^^^^ Argument 0 is redundant because it is implied by default.
        "});
    }

    #[test]
    fn corrects_sum_zero() {
        test::<RedundantArgument>().expect_correction(
            indoc! {"
                array.sum(0)
                ^^^^^^^^^^^^ Argument 0 is redundant because it is implied by default.
            "},
            "array.sum\n",
        );
    }

    #[test]
    fn flags_exit_true() {
        test::<RedundantArgument>().expect_offense(indoc! {"
            exit(true)
            ^^^^^^^^^^ Argument true is redundant because it is implied by default.
        "});
    }

    #[test]
    fn corrects_exit_true() {
        test::<RedundantArgument>().expect_correction(
            indoc! {"
                exit(true)
                ^^^^^^^^^^ Argument true is redundant because it is implied by default.
            "},
            "exit\n",
        );
    }

    #[test]
    fn flags_exit_bang_false() {
        test::<RedundantArgument>().expect_offense(indoc! {"
            exit!(false)
            ^^^^^^^^^^^^ Argument false is redundant because it is implied by default.
        "});
    }

    #[test]
    fn corrects_exit_bang_false() {
        test::<RedundantArgument>().expect_correction(
            indoc! {"
                exit!(false)
                ^^^^^^^^^^^^ Argument false is redundant because it is implied by default.
            "},
            "exit!\n",
        );
    }

    #[test]
    fn flags_to_i_ten() {
        test::<RedundantArgument>().expect_offense(indoc! {"
            string.to_i(10)
            ^^^^^^^^^^^^^^^ Argument 10 is redundant because it is implied by default.
        "});
    }

    #[test]
    fn corrects_to_i_ten() {
        test::<RedundantArgument>().expect_correction(
            indoc! {"
                string.to_i(10)
                ^^^^^^^^^^^^^^^ Argument 10 is redundant because it is implied by default.
            "},
            "string.to_i\n",
        );
    }

    #[test]
    fn flags_split_space() {
        test::<RedundantArgument>().expect_offense(indoc! {r#"
            string.split(" ")
            ^^^^^^^^^^^^^^^^^ Argument " " is redundant because it is implied by default.
        "#});
    }

    #[test]
    fn corrects_split_space() {
        test::<RedundantArgument>().expect_correction(
            indoc! {r#"
                string.split(" ")
                ^^^^^^^^^^^^^^^^^ Argument " " is redundant because it is implied by default.
            "#},
            "string.split\n",
        );
    }

    #[test]
    fn flags_chomp_newline() {
        test::<RedundantArgument>().expect_offense(indoc! {r#"
            string.chomp("\n")
            ^^^^^^^^^^^^^^^^^^ Argument "\n" is redundant because it is implied by default.
        "#});
    }

    #[test]
    fn corrects_chomp_newline() {
        test::<RedundantArgument>().expect_correction(
            indoc! {r#"
                string.chomp("\n")
                ^^^^^^^^^^^^^^^^^^ Argument "\n" is redundant because it is implied by default.
            "#},
            "string.chomp\n",
        );
    }

    #[test]
    fn flags_chomp_bang_newline() {
        test::<RedundantArgument>().expect_offense(indoc! {r#"
            string.chomp!("\n")
            ^^^^^^^^^^^^^^^^^^^ Argument "\n" is redundant because it is implied by default.
        "#});
    }

    #[test]
    fn flags_exit_true_unparenthesized() {
        test::<RedundantArgument>().expect_offense(indoc! {"
            exit true
            ^^^^^^^^^ Argument true is redundant because it is implied by default.
        "});
    }

    #[test]
    fn corrects_exit_true_unparenthesized() {
        test::<RedundantArgument>().expect_correction(
            indoc! {"
                exit true
                ^^^^^^^^^ Argument true is redundant because it is implied by default.
            "},
            "exit\n",
        );
    }

    #[test]
    fn accepts_join_noarg() {
        test::<RedundantArgument>().expect_no_offenses("array.join\n");
    }

    #[test]
    fn accepts_join_nondefault() {
        test::<RedundantArgument>().expect_no_offenses("array.join(\",\")\n");
    }

    #[test]
    fn accepts_sum_nonzero() {
        test::<RedundantArgument>().expect_no_offenses("array.sum(1)\n");
    }

    #[test]
    fn accepts_to_i_non_decimal() {
        test::<RedundantArgument>().expect_no_offenses("string.to_i(16)\n");
    }

    #[test]
    fn accepts_multiple_args() {
        test::<RedundantArgument>().expect_no_offenses("foo.join('', '')\n");
    }

    #[test]
    fn accepts_no_receiver_for_non_exit() {
        test::<RedundantArgument>().expect_no_offenses("split(\" \")\n");
    }

    #[test]
    fn from_config_json_missing_methods_uses_default() {
        let opts = RedundantArgumentOptions::from_config_json(b"{}")
            .expect("missing field returns default");
        assert_eq!(opts.methods.get("join"), Some(&DefaultArg::Str(String::new())));
    }

    #[test]
    fn from_config_json_parses_methods() {
        let json = r#"{"Methods": {"foo": 2, "bar": "hello", "baz": true}}"#;
        let opts = RedundantArgumentOptions::from_config_json(json.as_bytes())
            .expect("valid config");
        assert_eq!(opts.methods.get("foo"), Some(&DefaultArg::Int(2)));
        assert_eq!(
            opts.methods.get("bar"),
            Some(&DefaultArg::Str("hello".to_string()))
        );
        assert_eq!(opts.methods.get("baz"), Some(&DefaultArg::Bool(true)));
    }

    #[test]
    fn from_config_json_methods_wrong_type() {
        use murphy_plugin_api::ConfigErrorKind;
        let err = RedundantArgumentOptions::from_config_json(b"{\"Methods\": 42}")
            .expect_err("non-object Methods is invalid");
        let ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "Methods");
        assert_eq!(*expected, "object");
    }

    #[test]
    fn from_config_json_methods_value_wrong_type() {
        use murphy_plugin_api::ConfigErrorKind;
        let json = r#"{"Methods": {"foo": [1, 2]}}"#;
        let err = RedundantArgumentOptions::from_config_json(json.as_bytes())
            .expect_err("array value is invalid");
        let ConfigErrorKind::TypeMismatch { field, .. } = err.kind() else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "Methods.foo");
    }

    #[test]
    fn roundtrip_config() {
        let opts = RedundantArgumentOptions::default();
        let json = opts.to_config_json();
        let opts2 = RedundantArgumentOptions::from_config_json(json.as_bytes())
            .expect("roundtrip");
        assert_eq!(opts.methods, opts2.methods);
    }
}
murphy_plugin_api::submit_cop!(RedundantArgument);
