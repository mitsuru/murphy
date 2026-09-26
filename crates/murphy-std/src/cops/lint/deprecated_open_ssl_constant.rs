//! `Lint/DeprecatedOpenSSLConstant` — flag algorithm constants for
//! `OpenSSL::Cipher` / `OpenSSL::Digest` (deprecated since OpenSSL 2.2.0) and
//! autocorrect to the string-argument form.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DeprecatedOpenSSLConstant
//! upstream_version_checked: 1.86.2
//! version_added: "0.84"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full port: `OpenSSL::Cipher::AES.new(128, :GCM)` → `OpenSSL::Cipher.new('aes-128-gcm')`,
//!   `OpenSSL::Digest::SHA256.new` → `OpenSSL::Digest.new('SHA256')`,
//!   `OpenSSL::Digest::SHA256.digest('foo')` → `OpenSSL::Digest.digest('SHA256', 'foo')`.
//!   The argument-safety guard is a positive Int/Sym/Str allow-list rather than
//!   RuboCop's negative variable/call/const skip — a deliberate, slightly
//!   stricter choice that additionally skips Dstr/array/hash args (which RuboCop
//!   would attempt to rewrite into malformed code). Also mirrors the
//!   `digest_const?` receiver guard, `OpenSSL::Cipher::Cipher` passthrough, the
//!   NO_ARG_ALGORITHM (BF/DES/IDEA/RC4) special case, and the 3-char cipher-name
//!   chunking + CBC default-mode reconstruction.
//! ```
//!
//! ## Matched shapes
//!
//! `send` nodes with selector `new`/`digest` whose receiver is
//! `OpenSSL::Cipher::<X>` or `OpenSSL::Digest::<X>`.
//!
//! ## Autocorrect
//!
//! Whole-node replacement: rebuilds `<parent_const>.<selector>(<args>)`. This is
//! an AST-shuffle (the algorithm name moves from a constant into a string
//! argument), so whole-node interpolation is the right tool over surgical edits.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Lint/DeprecatedOpenSSLConstant` `algorithm_const` is
// `(send $(const (const (const {nil? cbase} :OpenSSL) {:Cipher :Digest}) _) ...)`
// (any method, any args; `RESTRICT_ON_SEND = %i[new digest]` restricts dispatch).
// Murphy's `check_send` already restricts to `new`/`digest` via
// `#[on_node(kind = "send", methods = ["new", "digest"])]`, so the verbatim
// port uses `_ ...` (any method, any args) to match the same node set as
// RuboCop's method-omitted form (per `Style/DateTime` precedent).
// In Murphy `::OpenSSL` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (pinned by `flags_cbase_prefixed`). Namespaced `Foo::OpenSSL` still
// does not flag (pinned by `boundary_ignores_namespaced_openssl`). `send`
// covers `Send` only, matching `#[on_node(kind = "send")]` (csend still does
// not flag, pinned by `boundary_ignores_csend`). The algorithm-name `_`
// matches any const name (`AES`, `SHA256`, etc). The capture stays hand-rolled
// below (parent extraction for replacement); this is predicate-only.
def_node_matcher!(
    is_algorithm_const_send,
    "(send (const (const (const nil? :OpenSSL) {:Cipher :Digest}) _) _ ...)"
);

// RuboCop parity: `Lint/DeprecatedOpenSSLConstant` `digest_const?` is
// `(const _ :Digest)` (any scope). In Murphy `_` covers bare + `::` +
// namespaced (all skip, pinned by `boundary_skips_cbase_digest_new` +
// `boundary_skips_namespaced_digest_new`). The cop dispatches on `send`
// only, so csend never reaches this check (pinned by
// `boundary_ignores_csend`). Predicate-only, no captures, byte-identical
// suppression.
def_node_matcher!(is_digest_const, "(const _ :Digest)");

#[derive(Default)]
pub struct DeprecatedOpenSSLConstant;

const NO_ARG_ALGORITHM: &[&str] = &["BF", "DES", "IDEA", "RC4"];

#[cop(
    name = "Lint/DeprecatedOpenSSLConstant",
    description = "Flag deprecated OpenSSL algorithm constants.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DeprecatedOpenSSLConstant {
    #[on_node(kind = "send", methods = ["new", "digest"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let args = cx.call_arguments(node);
        // RuboCop skips when an argument is a variable, call, or const because
        // the rewrite cannot be proven correct. The OpenSSL algorithm
        // constructors only accept simple static literals (size ints, mode
        // symbols, name strings), so we use a stricter positive allow-list:
        // anything other than `Int`/`Sym`/`Str` (e.g. `Dstr`, arrays, hashes)
        // is also dynamic for our purposes and must not be autocorrected.
        if args.iter().any(|&arg| {
            !matches!(
                cx.kind(arg),
                NodeKind::Int(..) | NodeKind::Sym(..) | NodeKind::Str(..)
            )
        }) {
            return;
        }

        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        // `(const _ :Digest)` — any scope (bare + `::` + namespaced skip).
        if is_digest_const(receiver, cx) {
            return;
        }

        // `algorithm_const`: receiver must be `OpenSSL::{Cipher|Digest}::<X>`.
        // `(send (const (const (const nil? :OpenSSL) {:Cipher :Digest}) _) _ ...)`
        // (`OpenSSL` / `::OpenSSL`, top-level only). `send` covers `Send` only.
        if !is_algorithm_const_send(node, cx) {
            return;
        }
        let Some(parent) = algorithm_const_parent(receiver, cx) else {
            return;
        };

        let parent_source = cx.raw_source(cx.range(parent));
        let method = cx.raw_source(cx.loc(node).name);
        let Some(replacement_args) = replacement_args(node, receiver, parent, cx) else {
            return;
        };

        let original = cx.raw_source(cx.range(node));
        let message = format!(
            "Use `{parent_source}.{method}({replacement_args})` instead of `{original}`."
        );
        cx.emit_offense(cx.range(node), &message, None);

        let replacement = format!("{parent_source}.{method}({replacement_args})");
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// If `receiver` is `OpenSSL::{Cipher|Digest}::<X>`, return the parent const
/// node (`OpenSSL::Cipher` / `OpenSSL::Digest`).
fn algorithm_const_parent(receiver: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    // `(send (const (const (const nil? :OpenSSL) {:Cipher :Digest}) _) _ ...)`
    // already verified the `OpenSSL::{Cipher,Digest}::<X>` chain via
    // `is_algorithm_const_send`; just extract the parent for replacement.
    // receiver = (const <parent> :X)
    let NodeKind::Const { scope, .. } = *cx.kind(receiver) else {
        return None;
    };
    scope.get()
}

/// `openssl_class` — the parent const's source (`OpenSSL::Cipher` /
/// `OpenSSL::Digest`).
fn openssl_class<'a>(parent: NodeId, cx: &Cx<'a>) -> &'a str {
    cx.raw_source(cx.range(parent))
}

/// `algorithm_name` — the short const name, hyphenated into 3-char chunks for
/// cipher algorithms not in NO_ARG_ALGORITHM.
fn algorithm_name(receiver: NodeId, parent: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Const { name: sym, .. } = *cx.kind(receiver) else {
        return String::new();
    };
    let name = cx.symbol_str(sym);
    if openssl_class(parent, cx) == "OpenSSL::Cipher" && !NO_ARG_ALGORITHM.contains(&name) {
        scan_three(name).join("-")
    } else {
        name.to_string()
    }
}

/// `name.scan(/.{3}/)` — non-overlapping 3-char groups, trailing remainder
/// dropped (matches Ruby's `String#scan`).
fn scan_three(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    chars
        .chunks(3)
        .filter(|c| c.len() == 3)
        .map(|c| c.iter().collect())
        .collect()
}

/// `sanitize_arguments` — RuboCop's `arg.str_type? ? arg.value : arg.source`
/// then `.tr(":'", '')` then `split('-')`, flattened. `string_str` / `symbol_str`
/// give already-unquoted values for the common `Str` / `Sym` cases; the
/// fallback strips stray `:` / `'` from raw source to match `tr`.
fn sanitize_arguments(node: NodeId, cx: &Cx<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for &arg in cx.call_arguments(node) {
        let argument: String = match *cx.kind(arg) {
            NodeKind::Str(id) => cx.string_str(id).to_string(),
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_string(),
            _ => cx.raw_source(cx.range(arg)).replace([':', '\''], ""),
        };
        for part in argument.split('-') {
            out.push(part.to_string());
        }
    }
    out
}

fn replacement_args(
    node: NodeId,
    receiver: NodeId,
    parent: NodeId,
    cx: &Cx<'_>,
) -> Option<String> {
    // `OpenSSL::Cipher::Cipher.new('aes-128-cbc')` passthrough.
    if cx.raw_source(cx.range(receiver)) == "OpenSSL::Cipher::Cipher" {
        let first = cx.call_arguments(node).first().copied()?;
        return Some(cx.raw_source(cx.range(first)).to_string());
    }

    let algorithm_name = algorithm_name(receiver, parent, cx);
    if openssl_class(parent, cx) == "OpenSSL::Cipher" {
        Some(build_cipher_arguments(
            node,
            &algorithm_name,
            cx.call_arguments(node).is_empty(),
            cx,
        ))
    } else {
        let mut parts = vec![format!("'{algorithm_name}'")];
        for &arg in cx.call_arguments(node) {
            parts.push(cx.raw_source(cx.range(arg)).to_string());
        }
        Some(parts.join(", "))
    }
}

fn build_cipher_arguments(
    node: NodeId,
    algorithm_name: &str,
    no_arguments: bool,
    cx: &Cx<'_>,
) -> String {
    let algorithm_parts: Vec<String> =
        algorithm_name.to_lowercase().split('-').map(str::to_string).collect();
    let size_and_mode: Vec<String> =
        sanitize_arguments(node, cx).iter().map(|s| s.to_lowercase()).collect();

    let first_upper = algorithm_parts
        .first()
        .map(|s| s.to_uppercase())
        .unwrap_or_default();
    if NO_ARG_ALGORITHM.contains(&first_upper.as_str()) && no_arguments {
        format!("'{}'", algorithm_parts.first().cloned().unwrap_or_default())
    } else {
        let mut combined = algorithm_parts;
        combined.extend(size_and_mode.iter().cloned());
        if size_and_mode.is_empty() {
            combined.push("cbc".to_string());
        }
        let joined = combined.into_iter().take(3).collect::<Vec<_>>().join("-");
        format!("'{joined}'")
    }
}

murphy_plugin_api::submit_cop!(DeprecatedOpenSSLConstant);

#[cfg(test)]
mod tests {
    use super::DeprecatedOpenSSLConstant;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_cipher_aes() {
        test::<DeprecatedOpenSSLConstant>().expect_offense(indoc! {r#"
            OpenSSL::Cipher::AES.new(128, :GCM)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Cipher.new('aes-128-gcm')` instead of `OpenSSL::Cipher::AES.new(128, :GCM)`.
        "#});
    }

    #[test]
    fn flags_digest_new() {
        test::<DeprecatedOpenSSLConstant>().expect_offense(indoc! {r#"
            OpenSSL::Digest::SHA256.new
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Digest.new('SHA256')` instead of `OpenSSL::Digest::SHA256.new`.
        "#});
    }

    #[test]
    fn flags_digest_digest() {
        test::<DeprecatedOpenSSLConstant>().expect_offense(indoc! {r#"
            OpenSSL::Digest::SHA256.digest('foo')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Digest.digest('SHA256', 'foo')` instead of `OpenSSL::Digest::SHA256.digest('foo')`.
        "#});
    }

    #[test]
    fn does_not_flag_already_correct() {
        test::<DeprecatedOpenSSLConstant>().expect_no_offenses(indoc! {r#"
            OpenSSL::Cipher.new('aes-128-gcm')
            OpenSSL::Digest.new('SHA256')
            OpenSSL::Digest.digest('SHA256', 'foo')
        "#});
    }

    #[test]
    fn does_not_flag_dynamic_arguments() {
        test::<DeprecatedOpenSSLConstant>().expect_no_offenses(indoc! {r#"
            OpenSSL::Cipher::AES.new(128, mode)
            OpenSSL::Cipher::AES.new(foo.bar)
            OpenSSL::Cipher::AES.new(SOME_CONST)
        "#});
    }

    #[test]
    fn does_not_flag_complex_literal_arguments() {
        // Interpolated strings, arrays, and hashes are not the simple
        // Int/Sym/Str literals the rewrite can handle — skip them.
        test::<DeprecatedOpenSSLConstant>().expect_no_offenses(indoc! {r##"
            OpenSSL::Cipher::AES.new("#{size}-gcm")
            OpenSSL::Cipher::AES.new([128])
        "##});
    }

    #[test]
    fn corrects_cipher_aes() {
        test::<DeprecatedOpenSSLConstant>().expect_correction(
            indoc! {r#"
                OpenSSL::Cipher::AES.new(128, :GCM)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Cipher.new('aes-128-gcm')` instead of `OpenSSL::Cipher::AES.new(128, :GCM)`.
            "#},
            "OpenSSL::Cipher.new('aes-128-gcm')\n",
        );
    }

    #[test]
    fn corrects_cipher_aes_no_mode_defaults_cbc() {
        test::<DeprecatedOpenSSLConstant>().expect_correction(
            indoc! {r#"
                OpenSSL::Cipher::AES128.new
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Cipher.new('aes-128-cbc')` instead of `OpenSSL::Cipher::AES128.new`.
            "#},
            "OpenSSL::Cipher.new('aes-128-cbc')\n",
        );
    }

    #[test]
    fn corrects_digest_new() {
        test::<DeprecatedOpenSSLConstant>().expect_correction(
            indoc! {r#"
                OpenSSL::Digest::SHA256.new
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Digest.new('SHA256')` instead of `OpenSSL::Digest::SHA256.new`.
            "#},
            "OpenSSL::Digest.new('SHA256')\n",
        );
    }

    #[test]
    fn corrects_digest_digest() {
        test::<DeprecatedOpenSSLConstant>().expect_correction(
            indoc! {r#"
                OpenSSL::Digest::SHA256.digest('foo')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Digest.digest('SHA256', 'foo')` instead of `OpenSSL::Digest::SHA256.digest('foo')`.
            "#},
            "OpenSSL::Digest.digest('SHA256', 'foo')\n",
        );
    }

    #[test]
    fn flags_cbase_prefixed() {
        // `::OpenSSL::Cipher::AES` is still detected (cbase grandparent scope).
        // RuboCop's `openssl_class` is `children.first.source`, which here is
        // `::OpenSSL::Cipher` (leading `::` included) and therefore does NOT
        // string-equal `OpenSSL::Cipher` — so RuboCop falls through to the
        // generic `'<name>', args...` form rather than the cipher reconstruction.
        // Murphy mirrors that exact (arguably surprising) upstream behavior.
        test::<DeprecatedOpenSSLConstant>().expect_offense(indoc! {r#"
            ::OpenSSL::Cipher::AES.new(128, :GCM)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `::OpenSSL::Cipher.new('AES', 128, :GCM)` instead of `::OpenSSL::Cipher::AES.new(128, :GCM)`.
        "#});
    }

    #[test]
    fn no_arg_algorithm_des() {
        test::<DeprecatedOpenSSLConstant>().expect_correction(
            indoc! {r#"
                OpenSSL::Cipher::DES.new
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `OpenSSL::Cipher.new('des')` instead of `OpenSSL::Cipher::DES.new`.
            "#},
            "OpenSSL::Cipher.new('des')\n",
        );
    }

    // --- Boundary characterization (murphy-ft88.13): pin the exact node set
    // the hand-rolled `algorithm_const_parent` (nested `OpenSSL::{Cipher,Digest}`
    // chain, top-level only) matches, so the verbatim
    // `(send (const (const (const nil? :OpenSSL) {:Cipher :Digest}) _) _ ...)`
    // refactor can be proven equivalent. `::OpenSSL` collapses:
    // `nil?` covers bare + `::` (both flag, cbase pinned by
    // `flags_cbase_prefixed`). Namespaced `Foo::OpenSSL` is not top-level,
    // so it does not flag. `send` covers `Send` only, matching
    // `#[on_node(kind = "send")]` (csend still does not flag).

    #[test]
    fn boundary_ignores_namespaced_openssl() {
        test::<DeprecatedOpenSSLConstant>().expect_no_offenses("Foo::OpenSSL::Cipher::AES.new(128, :GCM)\n");
    }

    #[test]
    fn boundary_ignores_csend() {
        test::<DeprecatedOpenSSLConstant>().expect_no_offenses("OpenSSL::Cipher::AES&.new(128, :GCM)\n");
    }

    // --- Boundary characterization (murphy-ft88.28): pin the exact node set
    // the hand-rolled `is_named_const(Digest)` (any scope, no scope check)
    // matches, so the verbatim `(const _ :Digest)` refactor can be proven
    // equivalent. `_` covers any scope: bare + `::` + namespaced all skip
    // (no offense, already-correct form). The cop dispatches on `send` only,
    // so csend never reaches the Digest check (already pinned by
    // `boundary_ignores_csend`).

    #[test]
    fn boundary_skips_cbase_digest_new() {
        test::<DeprecatedOpenSSLConstant>()
            .expect_no_offenses("::Digest.new('SHA256')\n");
    }

    #[test]
    fn boundary_skips_namespaced_digest_new() {
        test::<DeprecatedOpenSSLConstant>()
            .expect_no_offenses("Foo::Digest.new('SHA256')\n");
    }

    #[test]
    fn boundary_skips_cbase_openssl_digest_new() {
        test::<DeprecatedOpenSSLConstant>()
            .expect_no_offenses("::OpenSSL::Digest.new('SHA256')\n");
    }
}
