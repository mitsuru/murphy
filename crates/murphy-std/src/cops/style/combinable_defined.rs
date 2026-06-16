//! `Style/CombinableDefined` — combine nested `defined?` calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CombinableDefined
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags multiple `defined?` calls joined by `&&` that could be combined.
//!   Autocorrect is a v1 gap; only offense reporting is implemented.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct CombinableDefined;

#[cop(
    name = "Style/CombinableDefined",
    description = "Combine nested `defined?` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CombinableDefined {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.parent(node).get().is_some_and(|parent| matches!(cx.kind(parent), NodeKind::And { .. })) {
            return;
        }
        let mut defined_nodes = Vec::new();
        let mut work = vec![node];
        while let Some(current) = work.pop() {
            match cx.kind(current) {
                NodeKind::And { lhs, rhs } => {
                    work.push(*lhs);
                    work.push(*rhs);
                }
                NodeKind::Defined(_) => {
                    defined_nodes.push(current);
                }
                _ => {}
            }
        }
        if defined_nodes.len() < 2 {
            return;
        }
        let sources: Vec<_> = defined_nodes.iter().filter_map(|&dn| {
            let NodeKind::Defined(val_id) = *cx.kind(dn) else {
                return None;
            };
            match cx.kind(val_id) {
                NodeKind::Send { .. } | NodeKind::Const { .. } => {
                    Some((dn, cx.raw_source(cx.range(val_id))))
                }
                _ => None,
            }
        }).collect();
        let has_nested = sources.iter().any(|&(dn, val_src)| {
            sources.iter().any(|&(other, other_src)| {
                other != dn
                    && other_src.starts_with(val_src)
                    && other_src != val_src
                    && other_src.as_bytes().get(val_src.len()).is_none_or(|&b| {
                        // Must be a nesting delimiter (. or ::), not a longer identifier.
                        b == b'.' || b == b':'
                    })
            })
        });
        if has_nested {
            cx.emit_offense(cx.range(node), "Combine nested `defined?` calls.", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CombinableDefined;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_combinable_nested_constants() {
        test::<CombinableDefined>().expect_offense(indoc! {"
            defined?(Foo) && defined?(Foo::Bar) && defined?(Foo::Bar::Baz)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
        "});
    }

    #[test]
    fn flags_combinable_nested_methods() {
        test::<CombinableDefined>().expect_offense(indoc! {"
            defined?(foo) && defined?(foo.bar) && defined?(foo.bar.baz)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine nested `defined?` calls.
        "});
    }

    #[test]
    fn accepts_single_defined() {
        test::<CombinableDefined>().expect_no_offenses("defined?(Foo)\n");
    }

    #[test]
    fn accepts_unrelated_and() {
        test::<CombinableDefined>().expect_no_offenses("a && b\n");
    }

    #[test]
    fn accepts_different_base_names_constants() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(Foo) && defined?(FooBar)\n",
        );
    }

    #[test]
    fn accepts_different_base_names_methods() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(foo) && defined?(foo_bar)\n",
        );
    }

    #[test]
    fn accepts_unrelated_defineds() {
        test::<CombinableDefined>().expect_no_offenses(
            "defined?(A) && defined?(B) && defined?(C)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(CombinableDefined);
