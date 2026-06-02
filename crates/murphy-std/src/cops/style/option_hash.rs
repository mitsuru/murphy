//! `Style/OptionHash` — flags optional hash parameters that should be keyword arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OptionHash
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects `def`/`defs` nodes whose last optional positional parameter is an
//!   empty hash (`options = {}`), with a name in `SuspiciousParamNames`.
//!   The method body is walked for bare `super` (`Zsuper`) — same as RuboCop's
//!   `node.parent.each_node(:zsuper).any?`, which crosses nested-def boundaries.
//!   `Allowlist` skips named methods. No autocorrect (matches upstream).
//!   Disabled by default (Enabled: false in RuboCop's default.yml).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! def fry(options = {})
//!   temperature = options.fetch(:temperature, 300)
//! end
//!
//! # good
//! def fry(temperature: 300)
//! end
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Prefer keyword arguments to options hashes.";

/// Cop options for `Style/OptionHash`.
#[derive(CopOptions)]
pub struct OptionHashOptions {
    #[option(
        name = "SuspiciousParamNames",
        default = ["options", "opts", "args"],
        description = "A list of parameter names that will be flagged by this cop."
    )]
    pub suspicious_param_names: Vec<String>,

    #[option(
        name = "Allowlist",
        default = [],
        description = "A list of method names that are allowed to use option hashes."
    )]
    pub allowlist: Vec<String>,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct OptionHash;

#[cop(
    name = "Style/OptionHash",
    description = "Don't use option hashes when you can use keyword arguments.",
    default_severity = "warning",
    default_enabled = false,
    options = OptionHashOptions,
)]
impl OptionHash {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(def_node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<OptionHashOptions>();

    // Get the method name for allowlist check.
    let Some(method_name) = cx.method_name(def_node) else {
        return;
    };
    if opts.allowlist.iter().any(|n| n.as_str() == method_name) {
        return;
    }

    // Get the args node.
    let args_node = match *cx.kind(def_node) {
        NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => args,
        _ => return,
    };

    // Get all children of the args node.
    let args_children = match *cx.kind(args_node) {
        NodeKind::Args(list) => cx.list(list).to_vec(),
        _ => return,
    };

    if args_children.is_empty() {
        return;
    }

    // Check if the last argument is an optarg with an empty hash default
    // and a suspicious name.
    let last_arg = *args_children.last().unwrap();
    let (name_sym, default_node) = match *cx.kind(last_arg) {
        NodeKind::Optarg { name, default } => (name, default),
        _ => return,
    };

    // Default must be an empty hash.
    let is_empty_hash = match *cx.kind(default_node) {
        NodeKind::Hash(list) => cx.list(list).is_empty(),
        _ => false,
    };
    if !is_empty_hash {
        return;
    }

    // Name must be in SuspiciousParamNames.
    let param_name = cx.symbol_str(name_sym);
    if !opts
        .suspicious_param_names
        .iter()
        .any(|n| n.as_str() == param_name)
    {
        return;
    }

    // Skip if the method body contains a bare `super` (Zsuper).
    // RuboCop's `node.parent.each_node(:zsuper)` — unconditional descendant walk.
    let body_opt = match *cx.kind(def_node) {
        NodeKind::Def { body, .. } | NodeKind::Defs { body, .. } => body,
        _ => return,
    };
    if let Some(body_node) = body_opt.get() {
        if subtree_contains_zsuper(cx, body_node) {
            return;
        }
    }

    // Offense on the optarg node.
    cx.emit_offense(cx.range(last_arg), MSG, None);
    // No autocorrect (upstream provides none).
}

/// Walk the subtree rooted at `node`, returning true if any descendant is a
/// `Zsuper` node. This intentionally crosses nested-def boundaries to match
/// RuboCop's `node.each_node(:zsuper).any?` behaviour.
fn subtree_contains_zsuper(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if matches!(*cx.kind(id), NodeKind::Zsuper) {
            return true;
        }
        stack.extend(cx.children(id));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{OptionHash, OptionHashOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offenses -----

    #[test]
    fn flags_options_hash_param() {
        test::<OptionHash>().expect_offense(indoc! {"
            def some_method(options = {})
                            ^^^^^^^^^^^^ Prefer keyword arguments to options hashes.
              puts some_arg
            end
        "});
    }

    #[test]
    fn flags_opts_param() {
        test::<OptionHash>().expect_offense(indoc! {"
            def some_method(opts = {})
                            ^^^^^^^^^ Prefer keyword arguments to options hashes.
            end
        "});
    }

    #[test]
    fn flags_args_param() {
        test::<OptionHash>().expect_offense(indoc! {"
            def some_method(args = {})
                            ^^^^^^^^^ Prefer keyword arguments to options hashes.
            end
        "});
    }

    // ----- Configured suspicious names -----

    #[test]
    fn flags_configured_suspicious_name() {
        test::<OptionHash>()
            .with_options(&OptionHashOptions {
                suspicious_param_names: vec!["options".to_string(), "config".to_string()],
                allowlist: vec![],
            })
            .expect_offense(indoc! {"
                def steep(flavor, duration, config={})
                                            ^^^^^^^^^ Prefer keyword arguments to options hashes.
                end
            "});
    }

    // ----- No offense -----

    #[test]
    fn accepts_non_suspicious_name() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def steep(flavor, duration, config={})
              nil
            end
        "});
    }

    #[test]
    fn accepts_no_arguments() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def meditate
              puts true
            end
        "});
    }

    #[test]
    fn accepts_nonempty_hash_default() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def cook(instructions, ingredients = { hot: [], cold: [] })
              nil
            end
        "});
    }

    #[test]
    fn accepts_super_in_body() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def allowed(foo, options = {})
              super
            end
        "});
    }

    #[test]
    fn accepts_super_with_code_before() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def allowed(foo, options = {})
              bar

              super
            end
        "});
    }

    #[test]
    fn accepts_super_in_nested_block() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def allowed(foo, options = {})
              5.times do
                super
              end
            end
        "});
    }

    #[test]
    fn accepts_allowlisted_method() {
        test::<OptionHash>()
            .with_options(&OptionHashOptions {
                suspicious_param_names: vec![
                    "options".to_string(),
                    "opts".to_string(),
                    "args".to_string(),
                ],
                allowlist: vec!["to_json".to_string()],
            })
            .expect_no_offenses(indoc! {"
                def to_json(options = {})
                end
            "});
    }

    #[test]
    fn accepts_keyword_arguments() {
        test::<OptionHash>().expect_no_offenses(indoc! {"
            def fry(temperature: 300)
              nil
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(OptionHash);
