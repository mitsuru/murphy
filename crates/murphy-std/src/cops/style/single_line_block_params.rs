//! `Style/SingleLineBlockParams` — enforces named block parameters for
//! configured single-line methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SingleLineBlockParams
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks that single-line blocks passed to configured methods use the
//!   preferred parameter names specified via the `Methods` option. The
//!   default configuration matches `reduce` and `inject` with params
//!   `[acc, elem]`.
//!
//!   Covered:
//!     - Only single-line `Block` nodes are checked (`is_single_line`).
//!     - Receiver required -- bare `reduce { ... }` is not flagged.
//!     - All block arguments must be plain `Arg` nodes (no splat/destruct).
//!     - Argument prefix matching: `_a` satisfies expected `a`; bare `_` does not
//!       match named expected (strips to empty string).
//!     - Autocorrect: rewrites `|args|` and all matching body `Lvar`/`Lvasgn` nodes
//!       within the current scope (stops at nested blocks/defs).
//!     - Methods option: configurable map of method name -> preferred param names.
//!   Gaps:
//!     - `on_numblock` not covered (numbered params `_1`, `_2`).
//! ```

use std::collections::BTreeMap;

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SingleLineBlockParams;

const MSG: &str = "Name `%<method>s` block params `|%<params>s|`.";

/// Options for `Style/SingleLineBlockParams`.
/// The `Methods` config maps a method name to the list of preferred param names.
/// Default: reduce/inject -> [acc, elem].
#[derive(Clone, Debug)]
pub struct Options {
    /// Map of method_name -> preferred_param_names.
    pub methods: BTreeMap<String, Vec<String>>,
}

impl Default for Options {
    fn default() -> Self {
        let mut methods = BTreeMap::new();
        methods.insert("reduce".to_string(), vec!["acc".to_string(), "elem".to_string()]);
        methods.insert("inject".to_string(), vec!["acc".to_string(), "elem".to_string()]);
        Self { methods }
    }
}

impl CopOptions for Options {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        // Missing `Methods` -> defaults.
        let Some(methods_value) = obj.get("Methods") else {
            return Ok(Self::default());
        };

        // Methods must be an array of single-key objects.
        let arr = methods_value
            .as_array()
            .ok_or_else(|| ConfigError::type_mismatch("Methods", "array"))?;

        let mut methods = BTreeMap::new();
        for (i, item) in arr.iter().enumerate() {
            let map = item.as_object().ok_or_else(|| {
                ConfigError::type_mismatch(format!("Methods[{i}]"), "object")
            })?;
            for (method_name, params_value) in map {
                let params_arr = params_value.as_array().ok_or_else(|| {
                    ConfigError::type_mismatch(format!("Methods[{i}].{method_name}"), "array")
                })?;
                let mut param_names = Vec::new();
                for (j, pv) in params_arr.iter().enumerate() {
                    let s = pv.as_str().ok_or_else(|| {
                        ConfigError::type_mismatch(
                            format!("Methods[{i}].{method_name}[{j}]"),
                            "string",
                        )
                    })?;
                    param_names.push(s.to_string());
                }
                methods.insert(method_name.clone(), param_names);
            }
        }

        Ok(Self { methods })
    }

    fn to_config_json(&self) -> String {
        let arr: Vec<serde_json::Value> = self
            .methods
            .iter()
            .map(|(name, params)| {
                let param_vals: Vec<serde_json::Value> =
                    params.iter().map(|p| serde_json::Value::String(p.clone())).collect();
                let mut obj = serde_json::Map::new();
                obj.insert(name.clone(), serde_json::Value::Array(param_vals));
                serde_json::Value::Object(obj)
            })
            .collect();
        serde_json::json!({ "Methods": arr }).to_string()
    }
}

#[cop(
    name = "Style/SingleLineBlockParams",
    description = "Enforces the names of some block params.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl SingleLineBlockParams {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        check_block(node, cx, &opts);
    }
}

fn check_block(node: NodeId, cx: &Cx<'_>, opts: &Options) {
    // Only single-line blocks.
    if !cx.is_single_line(node) {
        return;
    }

    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };

    // Receiver required.
    if cx.call_receiver(call).get().is_none() {
        return;
    }

    // Method must be in configured methods.
    let Some(method_name) = cx.method_name(call) else {
        return;
    };
    let Some(preferred_params) = opts.methods.get(method_name) else {
        return;
    };

    // All block args must be plain `Arg` nodes.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let arg_nodes = cx.list(args_list);
    if arg_nodes.is_empty() {
        return;
    }
    // All must be plain Arg.
    let actual_names: Vec<&str> = arg_nodes
        .iter()
        .filter_map(|&n| {
            if let NodeKind::Arg(sym) = *cx.kind(n) {
                Some(cx.symbol_str(sym))
            } else {
                None
            }
        })
        .collect();
    if actual_names.len() != arg_nodes.len() {
        // Some args are not plain Arg -- skip.
        return;
    }

    // Skip if param count does not match configured count exactly, to avoid
    // dropping extra params or building incomplete preferred names.
    if actual_names.len() != preferred_params.len() {
        return;
    }

    // Check if args already match.
    if args_match(&actual_names, preferred_params) {
        return;
    }

    // Build the preferred names, preserving underscore prefix from originals.
    let preferred_with_underscores: Vec<String> = actual_names
        .iter()
        .zip(preferred_params.iter())
        .map(|(actual, preferred)| {
            if actual.starts_with('_') {
                format!("_{preferred}")
            } else {
                preferred.clone()
            }
        })
        .collect();
    let joined = preferred_with_underscores.join(", ");
    let msg = MSG
        .replace("%<method>s", method_name)
        .replace("%<params>s", &joined);

    // Offense range: the args node (including pipes).
    let args_range = cx.range(args);
    // Extend range to include surrounding `|` pipes.
    let full_args_range = args_range_with_pipes(node, args_range, cx);

    cx.emit_offense(full_args_range, &msg, None);

    // Autocorrect: replace args declaration.
    cx.emit_edit(full_args_range, &format!("|{joined}|"));

    // Rename matching body lvars within the current scope (not crossing into
    // nested blocks or defs, which would introduce shadowing).
    if let Some(body_id) = body.get() {
        // Build old->new map, only for names that actually change.
        let rename_map: Vec<(String, String)> = actual_names
            .iter()
            .zip(preferred_with_underscores.iter())
            .filter(|(old, new)| **old != new.as_str())
            .map(|(old, new)| (old.to_string(), new.clone()))
            .collect();

        if !rename_map.is_empty() {
            rename_in_scope(body_id, &rename_map, cx);
        }
    }
}

/// Rename local variable references (`Lvar`) and assignments (`Lvasgn`) within
/// `node`, stopping only when a nested block/def redeclares a rename target as
/// its own parameter (shadow). Free variable references in nested blocks ARE
/// renamed.
fn rename_in_scope(node: NodeId, rename_map: &[(String, String)], cx: &Cx<'_>) {
    match cx.kind(node) {
        NodeKind::Lvar(sym) => {
            let name = cx.symbol_str(*sym);
            if let Some((_, new_name)) = rename_map.iter().find(|(old, _)| old == name) {
                cx.emit_edit(cx.range(node), new_name);
            }
        }
        NodeKind::Lvasgn { name, value } => {
            let name_str = cx.symbol_str(*name);
            if let Some((_, new_name)) = rename_map.iter().find(|(old, _)| old == name_str) {
                // Rename the assignment target. Use loc.name which is the name sub-range
                // of the Lvasgn node (excludes the ` =` part).
                let name_range = cx.node(node).loc.name;
                if name_range != murphy_plugin_api::Range::ZERO {
                    cx.emit_edit(name_range, new_name);
                }
            }
            // Recurse into the value (right-hand side of assignment).
            if let Some(val_id) = value.get() {
                rename_in_scope(val_id, rename_map, cx);
            }
        }
        // Nested blocks: recurse with a filtered rename_map that excludes any
        // name shadowed by the nested block's own parameters.
        NodeKind::Block { args: inner_args, body: inner_body, .. } => {
            // Build filtered rename_map: remove entries whose names are declared
            // as params in this nested block.
            let inner_shadowed = collect_block_param_names(*inner_args, cx);
            let filtered: Vec<(String, String)> = rename_map
                .iter()
                .filter(|(old, _)| !inner_shadowed.iter().any(|&s| s == old.as_str()))
                .cloned()
                .collect();
            if !filtered.is_empty() {
                if let Some(body_id) = inner_body.get() {
                    rename_in_scope(body_id, &filtered, cx);
                }
            }
        }
        // Numblock: numbered params (_1, _2) can't shadow named outer params.
        NodeKind::Numblock { body: inner_body, .. } => {
            if let Some(body_id) = inner_body.get() {
                rename_in_scope(body_id, rename_map, cx);
            }
        }
        // Hard scope boundaries: method/class definitions reset all locals.
        NodeKind::Def { .. } | NodeKind::Defs { .. } => {
            // Stop recursion entirely.
        }
        // For all other nodes, recurse into children.
        _ => {
            for child in cx.children(node) {
                rename_in_scope(child, rename_map, cx);
            }
        }
    }
}

/// Collect plain `Arg` parameter names declared in a block's `Args` node.
fn collect_block_param_names<'a>(args_id: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return vec![];
    };
    cx.list(list)
        .iter()
        .filter_map(|&n| {
            if let NodeKind::Arg(sym) = *cx.kind(n) {
                Some(cx.symbol_str(sym))
            } else {
                None
            }
        })
        .collect()
}

/// Returns `true` if actual params already match the preferred names
/// (ignoring leading underscores).
///
/// Strips leading underscores from both actual and expected before comparing.
/// For example: `_acc` matches `acc`, and `acc` matches `acc`.
/// A bare `_` strips to `""`, which only matches if the preferred is also `""`.
fn args_match(actual: &[&str], preferred: &[String]) -> bool {
    let actual_stripped: Vec<&str> =
        actual.iter().map(|a| a.trim_start_matches('_')).collect();
    let expected = preferred.iter().take(actual.len());
    actual_stripped.iter().zip(expected).all(|(a, e)| *a == e.trim_start_matches('_'))
}

/// Find the range from the opening `|` to the closing `|` around the args.
/// Falls back to `args_range` if pipes can't be found.
fn args_range_with_pipes(node: NodeId, args_range: Range, cx: &Cx<'_>) -> Range {
    let block_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    let lo = toks.partition_point(|t| t.range.start < block_range.start);
    let mut pipes = toks[lo..]
        .iter()
        .take_while(|t| t.range.start < block_range.end)
        .filter(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"|"
        })
        .copied();

    let Some(open_pipe) = pipes.next() else {
        return args_range;
    };
    let Some(close_pipe) = pipes.next() else {
        return args_range;
    };

    Range {
        start: open_pipe.range.start,
        end: close_pipe.range.end,
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, SingleLineBlockParams};
    use murphy_plugin_api::test_support::{indoc, test};

    fn default_opts() -> Options {
        Options::default()
    }

    // -------------------------------------------------------------------------
    // Offense cases
    // -------------------------------------------------------------------------

    #[test]
    fn flags_reduce_with_wrong_params() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_offense(indoc! {"
                foo.reduce { |c, d| c + d }
                             ^^^^^^ Name `reduce` block params `|acc, elem|`.
            "});
    }

    #[test]
    fn flags_inject_with_wrong_params() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_offense(indoc! {"
                foo.inject { |x, y| x + y }
                             ^^^^^^ Name `inject` block params `|acc, elem|`.
            "});
    }

    #[test]
    fn corrects_reduce_params_and_body_lvars() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    foo.reduce { |c, d| c + d }
                                 ^^^^^^ Name `reduce` block params `|acc, elem|`.
                "},
                "foo.reduce { |acc, elem| acc + elem }\n",
            );
    }

    #[test]
    fn corrects_inject_params() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    foo.inject { |x, y| x + y }
                                 ^^^^^^ Name `inject` block params `|acc, elem|`.
                "},
                "foo.inject { |acc, elem| acc + elem }\n",
            );
    }

    #[test]
    fn preserves_underscore_prefix_in_correction() {
        // `|_, _d|` -> `|_acc, _elem|`
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    foo.reduce { |_, _d| 1 }
                                 ^^^^^^^ Name `reduce` block params `|_acc, _elem|`.
                "},
                "foo.reduce { |_acc, _elem| 1 }\n",
            );
    }

    // -------------------------------------------------------------------------
    // No-offense cases
    // -------------------------------------------------------------------------

    #[test]
    fn no_offense_already_correct_params() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.reduce { |acc, elem| acc + elem }\n");
    }

    #[test]
    fn no_offense_underscore_correct() {
        // `|acc, _elem|` is OK -- underscore on second param matches `elem`.
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.reduce { |acc, _elem| acc }\n");
    }

    #[test]
    fn no_offense_multi_line_block() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses(indoc! {"
                foo.reduce do |c, d|
                  c + d
                end
            "});
    }

    #[test]
    fn no_offense_no_receiver() {
        // Bare `reduce { ... }` without receiver is not flagged.
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("reduce { |c, d| c + d }\n");
    }

    #[test]
    fn no_offense_method_not_in_config() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.map { |c, d| c + d }\n");
    }

    #[test]
    fn no_offense_empty_block() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.reduce { true }\n");
    }

    #[test]
    fn no_offense_splat_arg() {
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.reduce { |*args| args }\n");
    }


    #[test]
    fn corrects_nested_block_does_not_rename_shadow() {
        // Nested block with same param name should NOT have its lvar renamed.
        // Only the outer block's references should be renamed.
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    foo.reduce { |c, d| xs.map { |c| c } }
                                 ^^^^^^ Name `reduce` block params `|acc, elem|`.
                "},
                "foo.reduce { |acc, elem| xs.map { |c| c } }
",
            );
    }


    #[test]
    fn no_offense_more_params_than_configured() {
        // More params than configured -- skip to avoid dropping extra params.
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_no_offenses("foo.reduce { |x, y, z| x + y + z }
");
    }

    #[test]
    fn corrects_free_var_in_nested_block_is_renamed() {
        // Free variable reference to outer block param inside nested block IS renamed.
        test::<SingleLineBlockParams>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {r#"
                    foo.reduce { |c, d| xs.each { use(c) }; c + d }
                                 ^^^^^^ Name `reduce` block params `|acc, elem|`.
                "#},
                "foo.reduce { |acc, elem| xs.each { use(acc) }; acc + elem }\n",
            );
    }

    // -------------------------------------------------------------------------
    // Options parsing tests
    // -------------------------------------------------------------------------

    #[test]
    fn options_parse_error_not_an_object() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(b"[]")
            .expect_err("array root should be invalid");
        assert_eq!(err.kind(), &ConfigErrorKind::NotAnObject);
    }

    #[test]
    fn options_parse_error_methods_not_array() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(br#"{"Methods": "wrong"}"#)
            .expect_err("Methods string should be invalid");
        assert!(
            matches!(err.kind(), ConfigErrorKind::TypeMismatch { field, .. } if field == "Methods")
        );
    }

    #[test]
    fn options_parse_error_method_entry_not_object() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(br#"{"Methods": ["bad"]}"#)
            .expect_err("string element should be invalid");
        assert!(
            matches!(err.kind(), ConfigErrorKind::TypeMismatch { field, .. } if field == "Methods[0]")
        );
    }
}

murphy_plugin_api::submit_cop!(SingleLineBlockParams);
