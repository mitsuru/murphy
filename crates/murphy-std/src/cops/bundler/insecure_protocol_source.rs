//! `Bundler/InsecureProtocolSource` — flag deprecated insecure `source`
//! arguments in a Gemfile (`source :gemcutter`, `source :rubygems`,
//! `source :rubyforge`, and — when `AllowHttpProtocol: false` —
//! `source 'http://rubygems.org'`), suggesting `'https://rubygems.org'`. The
//! cop runs only on Gemfile/gems.rb files; the host applies the per-cop
//! `Include` from `config/default.yml`, so this cop never inspects the filename
//! itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/InsecureProtocolSource
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `def_node_matcher :insecure_protocol_source?,
//!   '(send nil? :source ${(sym :gemcutter) (sym :rubygems) (sym :rubyforge)
//!   (:str "http://rubygems.org")})'`. We match a bare `source` send
//!   (`nil?` receiver, send-only) with EXACTLY one argument that is either a
//!   `Sym` whose value is `gemcutter`/`rubygems`/`rubyforge` or a `Str` whose
//!   value is `http://rubygems.org`. The single-argument restriction mirrors the
//!   pattern's lack of `...` — `source :rubygems, require: false` (2 args) does
//!   NOT match, verified against standalone rubocop 1.87.0. The first argument is
//!   read directly without unwrapping parentheses, matching the strict `sym`/`str`
//!   pattern (same deliberate strictness as `Bundler/DuplicatedGem`).
//!
//!   The HTTP-string case is gated by `AllowHttpProtocol` (default `true`): when
//!   true (the default), `source 'http://rubygems.org'` is NOT flagged
//!   (RuboCop's `return if allow_http_protocol? && use_http_protocol`); when
//!   false it is flagged with `MSG_HTTP_PROTOCOL`. The symbol cases ignore
//!   `AllowHttpProtocol` and always fire.
//!
//!   Offense range is the argument node (RuboCop's `add_offense(source_node)`),
//!   not the whole `source` call — caret bounds cover just `:rubygems` /
//!   `'http://rubygems.org'`. Autocorrect replaces the argument range with the
//!   literal `'https://rubygems.org'` (single quotes), matching RuboCop's
//!   `corrector.replace(source_node, "'https://rubygems.org'")`; this is a
//!   type-changing whole-node replacement (sym/str → str), so the surgical
//!   two-edit pattern does not apply. Messages, offense column, the 2-arg
//!   exemption, the receiver exemption, and both autocorrect outputs were
//!   verified case-by-case against standalone rubocop 1.87.0.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG_HTTP_PROTOCOL: &str = "Use `https://rubygems.org` instead of `http://rubygems.org`.";

const REPLACEMENT: &str = "'https://rubygems.org'";

#[derive(Default)]
pub struct InsecureProtocolSource;

#[derive(CopOptions)]
pub struct InsecureProtocolSourceOptions {
    #[option(
        name = "AllowHttpProtocol",
        default = true,
        description = "When true, do not flag `source 'http://rubygems.org'`."
    )]
    pub allow_http_protocol: bool,
}

#[cop(
    name = "Bundler/InsecureProtocolSource",
    description = "The source `:gemcutter`, `:rubygems`, and `:rubyforge` are deprecated because HTTP requests are insecure.",
    default_severity = "warning",
    default_enabled = true,
    options = InsecureProtocolSourceOptions,
)]
impl InsecureProtocolSource {
    #[on_node(kind = "send", methods = ["source"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop's pattern is `(send nil? :source ...)`: bare call, no receiver.
        if cx.call_receiver(node).get().is_some() {
            return;
        }

        // The pattern captures a single argument (no `...`), so a multi-arg
        // call like `source :rubygems, require: false` does not match.
        let args = cx.call_arguments(node);
        let [arg] = args else {
            return;
        };
        let arg = *arg;

        let opts = cx.options_or_default::<InsecureProtocolSourceOptions>();

        let message = match *cx.kind(arg) {
            NodeKind::Sym(sym) => {
                let value = cx.symbol_str(sym);
                if !matches!(value, "gemcutter" | "rubygems" | "rubyforge") {
                    return;
                }
                format!(
                    "The source `:{value}` is deprecated because HTTP requests are insecure. \
                     Please change your source to 'https://rubygems.org' if possible, \
                     or 'http://rubygems.org' if not."
                )
            }
            NodeKind::Str(id) => {
                if cx.string_str(id) != "http://rubygems.org" {
                    return;
                }
                // `AllowHttpProtocol` (default true) suppresses only the
                // HTTP-string case, never the deprecated symbols.
                if opts.allow_http_protocol {
                    return;
                }
                MSG_HTTP_PROTOCOL.to_string()
            }
            _ => return,
        };

        cx.emit_offense(cx.range(arg), &message, None);
        cx.emit_edit(cx.range(arg), REPLACEMENT);
    }
}

murphy_plugin_api::submit_cop!(InsecureProtocolSource);

#[cfg(test)]
mod tests {
    use super::{InsecureProtocolSource, InsecureProtocolSourceOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_rubygems_symbol() {
        test::<InsecureProtocolSource>().expect_offense(indoc! {r#"
            source :rubygems
                   ^^^^^^^^^ The source `:rubygems` is deprecated because HTTP requests are insecure. Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not.
        "#});
    }

    #[test]
    fn flags_rubyforge_symbol() {
        test::<InsecureProtocolSource>().expect_offense(indoc! {r#"
            source :rubyforge
                   ^^^^^^^^^^ The source `:rubyforge` is deprecated because HTTP requests are insecure. Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not.
        "#});
    }

    #[test]
    fn flags_gemcutter_symbol() {
        test::<InsecureProtocolSource>().expect_offense(indoc! {r#"
            source :gemcutter
                   ^^^^^^^^^^ The source `:gemcutter` is deprecated because HTTP requests are insecure. Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not.
        "#});
    }

    #[test]
    fn corrects_symbol_to_https_url() {
        test::<InsecureProtocolSource>().expect_correction(
            indoc! {r#"
                source :rubygems
                       ^^^^^^^^^ The source `:rubygems` is deprecated because HTTP requests are insecure. Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not.
            "#},
            indoc! {r#"
                source 'https://rubygems.org'
            "#},
        );
    }

    #[test]
    fn allows_http_string_by_default() {
        // Default AllowHttpProtocol: true → http string is not flagged.
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            source 'http://rubygems.org'
        "#});
    }

    #[test]
    fn flags_http_string_when_allow_http_protocol_false() {
        let opts = InsecureProtocolSourceOptions {
            allow_http_protocol: false,
        };
        test::<InsecureProtocolSource>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                source 'http://rubygems.org'
                       ^^^^^^^^^^^^^^^^^^^^^ Use `https://rubygems.org` instead of `http://rubygems.org`.
            "#});
    }

    #[test]
    fn corrects_http_string_when_allow_http_protocol_false() {
        let opts = InsecureProtocolSourceOptions {
            allow_http_protocol: false,
        };
        test::<InsecureProtocolSource>().with_options(&opts).expect_correction(
            indoc! {r#"
                source 'http://rubygems.org'
                       ^^^^^^^^^^^^^^^^^^^^^ Use `https://rubygems.org` instead of `http://rubygems.org`.
            "#},
            indoc! {r#"
                source 'https://rubygems.org'
            "#},
        );
    }

    #[test]
    fn ignores_https_string() {
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            source 'https://rubygems.org'
        "#});
    }

    #[test]
    fn ignores_other_symbol() {
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            source :other
        "#});
    }

    #[test]
    fn ignores_source_with_receiver() {
        // `(send nil? :source ...)` requires a nil receiver.
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            foo.source :rubygems
        "#});
    }

    #[test]
    fn ignores_multi_argument_source() {
        // The pattern captures exactly one argument (no `...`), so a
        // multi-arg call does not match — verified against rubocop 1.87.0.
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            source :rubygems, require: false
        "#});
    }

    #[test]
    fn ignores_non_source_method() {
        test::<InsecureProtocolSource>().expect_no_offenses(indoc! {r#"
            gem :rubygems
        "#});
    }
}
