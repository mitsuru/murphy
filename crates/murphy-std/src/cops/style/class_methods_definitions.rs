//! `Style/ClassMethodsDefinitions` — enforces using `def self.method_name` or
//! `class << self` to define class methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassMethodsDefinitions
//! upstream_version_checked: 1.86.2
//! version_added: "0.89"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is at parity for both EnforcedStyle values. The `def_self` style
//!   fires on `class << self` blocks that contain only public instance defs;
//!   `self_class` fires on `def self.method_name` forms.
//!   Visibility detection matches RuboCop: only direct-child `def` nodes (receiver
//!   absent) of the sclass body count; inline `private def foo` wrapping excludes
//!   the def from the public set (it is a Send node, not a Def node); a preceding
//!   bare `private`/`protected` send marks all subsequent defs as non-public.
//!   Autocorrect (sclass -> `def self.x` unwrap) is deferred: the multi-line
//!   structural rewrite including comment extraction, de-indentation, and partial
//!   sclass preservation is left as a known v1 limitation. Offenses are reported
//!   correctly; users can apply the rewrite manually.
//! ```
//!
//! ## Matched shapes
//!
//! ### `def_self` style (default)
//!
//! A `class << self` (`sclass`) node where:
//! - The expression after `<<` is `self`
//! - The sclass body has at least one direct-child `def` node (receiver absent)
//! - All such `def` nodes are public (no preceding bare `private`/`protected`
//!   call in the same body)
//!
//! ### `self_class` style
//!
//! A `def self.method_name` node (`Def` with a `SelfExpr` receiver).
//!
//! ## Not matched
//!
//! - `class << not_self` (non-self singleton classes)
//! - `class << self` with no bare-def children (only `attr_accessor` etc.)
//! - `class << self` where any def is preceded by `private` or `protected`
//! - `def Foo.method` or `def object.method` (non-self receiver) in `self_class`
//!   mode -- only `def self.method` is flagged (matching RuboCop which fires only
//!   when `node.receiver.self_type?`)
//!
//! ## Autocorrect
//!
//! Autocorrect for the `def_self` direction (sclass -> `def self.x` unwrap) is
//! not implemented. The rewrite requires: extracting each def's source including
//! leading comments, de-indenting by the column difference, renaming `def foo` to
//! `def self.foo`, removing the def from the sclass body (possibly leaving an
//! empty sclass for non-def content), and inserting the new `def self.x` forms
//! after the sclass. This structural rewrite is deferred as a known v1 limitation.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ClassMethodsDefinitions;

const MSG_SCLASS: &str = "Do not define public methods within class << self.";

/// Enforced style for class method definitions.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Prefer `def self.method_name` form. Flag `class << self` with public defs.
    #[default]
    #[option(value = "def_self")]
    DefSelf,
    /// Prefer `class << self` form. Flag `def self.method_name`.
    #[option(value = "self_class")]
    SelfClass,
}

/// Cop options for [`ClassMethodsDefinitions`].
#[derive(CopOptions)]
pub struct ClassMethodsDefinitionsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "def_self",
        description = "Enforced style for class method definitions."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/ClassMethodsDefinitions",
    description = "Enforces using `def self.method_name` or `class << self` to define class methods.",
    default_severity = "warning",
    default_enabled = false,
    options = ClassMethodsDefinitionsOptions
)]
impl ClassMethodsDefinitions {
    /// `def_self` style: flag `class << self` blocks with all-public defs.
    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ClassMethodsDefinitionsOptions>();
        if opts.enforced_style != EnforcedStyle::DefSelf {
            return;
        }

        let NodeKind::Sclass { expr, body } = *cx.kind(node) else {
            return;
        };

        // Only flag `class << self` -- not `class << other_object`.
        if !matches!(cx.kind(expr), NodeKind::SelfExpr) {
            return;
        }

        // Check whether all direct-child `def` nodes (receiver absent) are public,
        // and that there is at least one such def.
        if !all_methods_public(body, cx) {
            return;
        }

        // Offense range: just the "class << self" header (start of sclass to end of expr).
        let offense_range = Range {
            start: cx.range(node).start,
            end: cx.range(expr).end,
        };
        cx.emit_offense(offense_range, MSG_SCLASS, None);
        // Autocorrect is deferred (see module-level notes).
    }

    /// `self_class` style: flag `def self.method_name` forms.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ClassMethodsDefinitionsOptions>();
        if opts.enforced_style != EnforcedStyle::SelfClass {
            return;
        }

        let NodeKind::Def { receiver, .. } = *cx.kind(node) else {
            return;
        };

        // Only flag when the receiver is `self`.
        let Some(recv_id) = receiver.get() else {
            return;
        };
        if !matches!(cx.kind(recv_id), NodeKind::SelfExpr) {
            return;
        }

        // Offense range: from the `def` keyword to the end of the method name
        // (e.g. `def self.one`). Because `Def` nodes use `push` (not
        // `push_named`), `cx.loc(node).name` is Range::ZERO. Find the name
        // end via token search: look for the first `Other` token after the
        // receiver's end that matches the method name symbol text.
        let name_end = def_method_name_end(node, recv_id, cx);
        let offense_range = Range {
            start: cx.range(node).start,
            end: name_end,
        };
        cx.emit_offense(
            offense_range,
            "Use `class << self` to define a class method.",
            None,
        );
    }
}

// ---------------------------------------------------------------------------
// Visibility helpers
// ---------------------------------------------------------------------------

/// Returns `true` iff the sclass body contains at least one direct-child
/// `def` node (receiver absent) **and** all such nodes are public.
///
/// Mirrors RuboCop `all_methods_public?` + `def_nodes`:
/// - Only direct children that are `Def { receiver: None }` count.
/// - Inline `private def foo` => the child is a `Send` node (not a `Def`),
///   so it is not counted at all.
/// - A preceding bare `private`/`protected`/`public` send changes the running
///   visibility for all subsequent `Def` children.
fn all_methods_public(body: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(body_id) = body.get() else {
        return false; // no body -> no defs -> false
    };

    let elements: &[NodeId] = match *cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(list),
        _ => {
            // Single-element body (no Begin wrapper): the body itself is the
            // sole statement.
            return is_public_bare_def(body_id, cx);
        }
    };

    let mut found_def = false;
    let mut visibility_public = true; // default visibility is public

    for &el in elements {
        match *cx.kind(el) {
            // Bare `private`/`protected`/`public` call with no args and no
            // receiver changes the running visibility for subsequent defs.
            NodeKind::Send {
                receiver,
                method,
                args,
            } if receiver.get().is_none() && cx.list(args).is_empty() => {
                let name = cx.symbol_str(method);
                match name {
                    "private" | "protected" => visibility_public = false,
                    "public" => visibility_public = true,
                    _ => {}
                }
            }
            // Direct `def` child with no receiver -- instance method inside
            // the singleton class body.
            NodeKind::Def { receiver, .. } if receiver.get().is_none() => {
                found_def = true;
                if !visibility_public {
                    return false; // at least one def is not public
                }
            }
            // Anything else: attr_accessor, `def self.x`, inline
            // `private def foo` (which is a Send node), etc. -- ignored for
            // visibility purposes, consistent with RuboCop `def_nodes` only
            // collecting `:def`-type children.
            _ => {}
        }
    }

    found_def
}

/// Helper for the single-statement-body case: the body itself is a single node.
/// Returns true iff the single node is a bare `def` (no receiver).
fn is_public_bare_def(node_id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node_id),
        NodeKind::Def { receiver, .. } if receiver.get().is_none()
    )
}

/// Find the end offset of the method name token in a `def self.method_name`
/// node. This is needed because `NodeKind::Def` nodes are created with
/// `builder.push` (not `push_named`), so `cx.loc(node).name` is `Range::ZERO`.
///
/// Searches for the first `SourceTokenKind::Other` token after the receiver
/// range end that matches the method name symbol text.
fn def_method_name_end(node: NodeId, recv_id: NodeId, cx: &Cx<'_>) -> u32 {
    let NodeKind::Def { name, .. } = *cx.kind(node) else {
        return cx.range(node).end;
    };
    let name_str = cx.symbol_str(name);
    let name_bytes = name_str.as_bytes();
    let recv_end = cx.range(recv_id).end;
    let node_end = cx.range(node).end;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find the method name token starting from after the receiver.
    let idx = toks.partition_point(|t| t.range.start < recv_end);
    if let Some(tok) = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_bytes
        })
    {
        tok.range.end
    } else {
        // Fallback: use the receiver end (should not happen).
        recv_end
    }
}


#[cfg(test)]
mod tests {
    use super::{ClassMethodsDefinitions, ClassMethodsDefinitionsOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // -----------------------------------------------------------------------
    // def_self style (default) -- offense cases
    // -----------------------------------------------------------------------

    #[test]
    fn def_self_flags_sclass_with_public_method_and_attr_reader() {
        // Fires: class << self contains attr_reader (non-def) + public def.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_offense(indoc! {"
                class A
                  class << self
                  ^^^^^^^^^^^^^ Do not define public methods within class << self.
                    attr_reader :two

                    def three
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_flags_sclass_with_only_public_methods() {
        // Fires: class << self contains only public defs.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_offense(indoc! {"
                class A
                  class << self
                  ^^^^^^^^^^^^^ Do not define public methods within class << self.
                    def one
                      :one
                    end

                    def two
                      :two
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_flags_sclass_with_def_self_and_bare_def() {
        // Fires: sclass has `def self.one` (has receiver -> not counted) + public `def two`.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_offense(indoc! {"
                class A
                  class << self
                  ^^^^^^^^^^^^^ Do not define public methods within class << self.
                    def self.one
                    end

                    def two
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_flags_single_def_in_sclass() {
        // Fires: single def in sclass body (no Begin wrapper).
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_offense(indoc! {"
                class Foo
                  class << self
                  ^^^^^^^^^^^^^ Do not define public methods within class << self.
                    def do_something
                      # TODO
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_flags_with_public_reset() {
        // `public` keyword resets visibility back to public after `private`.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_offense(indoc! {"
                class A
                  class << self
                  ^^^^^^^^^^^^^ Do not define public methods within class << self.
                    private

                    public

                    def one
                    end
                  end
                end
            "});
    }

    // -----------------------------------------------------------------------
    // def_self style -- no-offense cases
    // -----------------------------------------------------------------------

    #[test]
    fn def_self_accepts_sclass_with_private_method() {
        // No offense: sclass has `private` before a def -> that def is not public.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_no_offenses(indoc! {"
                class A
                  class << self
                    def one
                    end

                    private

                    def two
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_accepts_sclass_without_methods() {
        // No offense: sclass has no bare `def` children (only attr_reader).
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_no_offenses(indoc! {"
                class A
                  class << self
                    attr_reader :one
                  end
                end
            "});
    }

    #[test]
    fn def_self_accepts_def_self_form() {
        // No offense: `def self.method` is the preferred def_self style.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_no_offenses(indoc! {"
                class A
                  def self.one
                  end
                end
            "});
    }

    #[test]
    fn def_self_accepts_sclass_on_non_self_receiver() {
        // No offense: `class << not_self` -- expr is not SelfExpr.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_no_offenses(indoc! {"
                class A
                  class << not_self
                    def one
                    end
                  end
                end
            "});
    }

    #[test]
    fn def_self_accepts_sclass_with_only_self_methods() {
        // No offense: sclass body has only `def self.x` (receiver present) ->
        // no receiver-absent def nodes -> def_nodes is empty.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::DefSelf,
            })
            .expect_no_offenses(indoc! {"
                class A
                  class << self
                    def self.one
                    end
                  end
                end
            "});
    }

    // -----------------------------------------------------------------------
    // self_class style -- offense cases
    // -----------------------------------------------------------------------

    #[test]
    fn self_class_flags_def_self_method() {
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::SelfClass,
            })
            .expect_offense(indoc! {"
                class A
                  def self.one
                  ^^^^^^^^^^^^ Use `class << self` to define a class method.
                  end
                end
            "});
    }

    // -----------------------------------------------------------------------
    // self_class style -- no-offense cases
    // -----------------------------------------------------------------------

    #[test]
    fn self_class_accepts_sclass_form() {
        // No offense: `class << self` is the preferred self_class style.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::SelfClass,
            })
            .expect_no_offenses(indoc! {"
                class A
                  class << self
                    def one
                    end
                  end
                end
            "});
    }

    #[test]
    fn self_class_accepts_def_on_non_self_receiver() {
        // No offense: `def object.method` -- receiver is not SelfExpr.
        test::<ClassMethodsDefinitions>()
            .with_options(&ClassMethodsDefinitionsOptions {
                enforced_style: EnforcedStyle::SelfClass,
            })
            .expect_no_offenses(indoc! {"
                object = Object.new
                def object.method
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(ClassMethodsDefinitions);
