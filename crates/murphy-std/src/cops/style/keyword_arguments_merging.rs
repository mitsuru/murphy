//! `Style/KeywordArgumentsMerging` — flags `**opts.merge(...)` in method calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/KeywordArgumentsMerging
//! upstream_version_checked: 1.68.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Detection: kwsplat whose child is a `.merge(...)` call, where the
//!       kwsplat is the first element of a hash argument to an outer call.
//!     - Autocorrect: replaces `**opts.merge(foo: true)` with `**opts, foo: true`
//!       and `**opts.merge(other_opts)` with `**opts, **other_opts`.
//!     - Hash args with braces (`merge({foo: true})`) have the surrounding braces
//!       stripped; hash args without braces pass through as-is.
//!     - The outer method call may be either `Send` or `Csend` (safe navigation).
//!   Offense range is the merge call node (excludes the `**` prefix).
//!   Autocorrect edit range is the kwsplat node (includes the `**` prefix).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! some_method(**opts.merge(foo: true))
//! some_method(**opts.merge(other_opts))
//!
//! # good
//! some_method(**opts, foo: true)
//! some_method(**opts, **other_opts)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Provide additional arguments directly rather than using `merge`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct KeywordArgumentsMerging;

#[cop(
    name = "Style/KeywordArgumentsMerging",
    description = "When passing an existing hash as keyword arguments, provide additional \
                   arguments directly rather than using `merge`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl KeywordArgumentsMerging {
    #[on_node(kind = "kwsplat")]
    fn check_kwsplat(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(kwsplat: NodeId, cx: &Cx<'_>) {
    // The kwsplat's child must be a `.merge(...)` Send with at least one arg.
    let NodeKind::Kwsplat(child_opt) = *cx.kind(kwsplat) else {
        return;
    };
    let Some(merge_call) = child_opt.get() else {
        return;
    };
    // Must be a plain Send (not safe-nav) named :merge.
    let NodeKind::Send { receiver: recv_opt, .. } = *cx.kind(merge_call) else {
        return;
    };
    let Some(receiver) = recv_opt.get() else {
        return;
    };
    if cx.method_name(merge_call) != Some("merge") {
        return;
    }
    let merge_args = cx.call_arguments(merge_call);
    if merge_args.is_empty() {
        return;
    }

    // The kwsplat must be the first child of its parent hash, and that hash
    // must itself be a direct argument of an outer Send or Csend.
    let Some(hash_id) = cx.parent(kwsplat).get() else {
        return;
    };
    let NodeKind::Hash(hash_list) = *cx.kind(hash_id) else {
        return;
    };
    let hash_children = cx.list(hash_list);
    // The kwsplat must be the first element of the hash.
    if hash_children.first().copied() != Some(kwsplat) {
        return;
    }

    // The hash must be a direct argument of an outer Send or Csend.
    let Some(outer_call) = cx.parent(hash_id).get() else {
        return;
    };
    if !matches!(cx.kind(outer_call), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }

    // Emit offense at the merge call node (excluding `**`).
    cx.emit_offense(cx.range(merge_call), MSG, None);

    // Autocorrect: replace the entire kwsplat with `**receiver, <expanded args>`.
    let recv_src = cx.raw_source(cx.range(receiver));
    let other_parts: Vec<String> = merge_args
        .iter()
        .map(|&arg| {
            let src = cx.raw_source(cx.range(arg));
            if matches!(cx.kind(arg), NodeKind::Hash(_)) {
                // Hash arg: strip surrounding braces if present, using safe string methods.
                if let Some(inner) = src.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    inner.trim().to_owned()
                } else {
                    src.to_owned()
                }
            } else {
                format!("**{src}")
            }
        })
        .collect();

    let replacement = format!("**{}, {}", recv_src, other_parts.join(", "));
    cx.emit_edit(cx.range(kwsplat), &replacement);
}

#[cfg(test)]
mod tests {
    use super::KeywordArgumentsMerging;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_plain_kwsplat() {
        test::<KeywordArgumentsMerging>().expect_no_offenses("some_method(**opts)\n");
    }

    #[test]
    fn no_offense_merge_no_args() {
        // merge with no arguments — not flagged.
        test::<KeywordArgumentsMerging>().expect_no_offenses("some_method(**opts.merge)\n");
    }

    #[test]
    fn no_offense_kwsplat_not_first_in_hash() {
        // `a: 1` is before the kwsplat — kwsplat is not first child, not flagged.
        test::<KeywordArgumentsMerging>()
            .expect_no_offenses("some_method(a: 1, **opts.merge(foo: true))\n");
    }

    #[test]
    fn no_offense_already_direct() {
        test::<KeywordArgumentsMerging>().expect_no_offenses("some_method(**opts, foo: true)\n");
    }

    // --- Offense cases ---

    #[test]
    fn flags_merge_with_hash_arg() {
        test::<KeywordArgumentsMerging>().expect_offense(indoc! {"
            some_method(**opts.merge(foo: true))
                          ^^^^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
        "});
    }

    #[test]
    fn flags_merge_with_non_hash_arg() {
        test::<KeywordArgumentsMerging>().expect_offense(indoc! {"
            some_method(**opts.merge(other_opts))
                          ^^^^^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
        "});
    }

    // --- Autocorrect cases ---

    #[test]
    fn corrects_merge_with_hash_arg() {
        test::<KeywordArgumentsMerging>().expect_correction(
            indoc! {"
                some_method(**opts.merge(foo: true))
                              ^^^^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
            "},
            "some_method(**opts, foo: true)\n",
        );
    }

    #[test]
    fn corrects_merge_with_non_hash_arg() {
        test::<KeywordArgumentsMerging>().expect_correction(
            indoc! {"
                some_method(**opts.merge(other_opts))
                              ^^^^^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
            "},
            "some_method(**opts, **other_opts)\n",
        );
    }

    #[test]
    fn corrects_merge_with_braced_hash() {
        test::<KeywordArgumentsMerging>().expect_correction(
            indoc! {r#"
                some_method(**opts.merge({foo: true}))
                              ^^^^^^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
            "#},
            "some_method(**opts, foo: true)\n",
        );
    }

    #[test]
    fn corrects_idempotent() {
        test::<KeywordArgumentsMerging>().expect_no_offenses("some_method(**opts, foo: true)\n");
    }

    #[test]
    fn flags_safe_navigation_outer_call() {
        // The outer call can be a Csend (safe navigation).
        test::<KeywordArgumentsMerging>().expect_offense(indoc! {"
            obj&.foo(**opts.merge(bar: 1))
                       ^^^^^^^^^^^^^^^^^^ Provide additional arguments directly rather than using `merge`.
        "});
    }
}
murphy_plugin_api::submit_cop!(KeywordArgumentsMerging);
