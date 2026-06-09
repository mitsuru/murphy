//! `Style/HashLookupMethod` — enforces `Hash#[]` or `Hash#fetch` consistency.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashLookupMethod
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: brackets (default) and fetch are supported.
//!   AllowedReceivers is not yet wired (config option deferred).
//!   csend (safe-navigation) variant is not handled in correction.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const BRACKET_MSG: &str = "Use `Hash#[]` instead of `Hash#fetch`.";
const FETCH_MSG: &str = "Use `Hash#fetch` instead of `Hash#[]`.";

#[derive(Default)]
pub struct HashLookupMethod;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "brackets")]
    Brackets,
    #[option(value = "fetch")]
    Fetch,
}

#[derive(CopOptions)]
pub struct HashLookupMethodOptions {
    #[option(
        default = "brackets",
        description = "Enforced style for hash lookup."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/HashLookupMethod",
    description = "Enforce consistent hash lookup method.",
    default_severity = "warning",
    default_enabled = false,
    options = HashLookupMethodOptions
)]
impl HashLookupMethod {
    #[on_node(kind = "send", methods = ["[]", "fetch"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let Some(_recv_id) = receiver.get() else {
            return;
        };
        let opts = cx.options_or_default::<HashLookupMethodOptions>();
        let method_name = cx.symbol_str(method);

        match opts.enforced_style {
            EnforcedStyle::Brackets => {
                if method_name == "fetch" {
                    let arg_list = cx.list(args);
                    if arg_list.len() == 1 {
                        cx.emit_offense(cx.range(node), BRACKET_MSG, None);
                    }
                }
            }
            EnforcedStyle::Fetch => {
                if method_name == "[]" {
                    let arg_list = cx.list(args);
                    if arg_list.len() == 1 {
                        cx.emit_offense(cx.range(node), FETCH_MSG, None);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HashLookupMethod, HashLookupMethodOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn brackets_style_flags_fetch() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_offense(indoc! {"
                hash.fetch(key)
                ^^^^^^^^^^^^^^^ Use `Hash#[]` instead of `Hash#fetch`.
            "});
    }

    #[test]
    fn brackets_style_accepts_brackets() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash[key]\n");
    }

    #[test]
    fn fetch_style_flags_brackets() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_offense(indoc! {"
                hash[key]
                ^^^^^^^^^ Use `Hash#fetch` instead of `Hash#[]`.
            "});
    }

    #[test]
    fn fetch_style_accepts_fetch() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Fetch })
            .expect_no_offenses("hash.fetch(key)\n");
    }

    #[test]
    fn fetch_with_default_is_ignored() {
        test::<HashLookupMethod>()
            .with_options(&HashLookupMethodOptions { enforced_style: EnforcedStyle::Brackets })
            .expect_no_offenses("hash.fetch(key, default)\n");
    }

    #[test]
    fn default_style_is_brackets() {
        let opts = HashLookupMethodOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Brackets);
    }
}
murphy_plugin_api::submit_cop!(HashLookupMethod);
