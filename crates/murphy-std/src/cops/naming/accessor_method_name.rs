//! `Naming/AccessorMethodName` — flag `get_`/`set_` prefixes on accessor-style
//! method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/AccessorMethodName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_def`/`on_defs` (aliased) exactly. A definition is
//!   inspected only when its name is a "proper attribute name" — i.e. it does
//!   NOT end in `!`, `?`, or `=` (RuboCop's `proper_attribute_name?`). Two
//!   offense shapes:
//!     * reader (`bad_reader_name?`): name starts with `get_` AND the method
//!       takes no arguments (`!node.arguments?`).
//!     * writer (`bad_writer_name?`): name starts with `set_` AND the method
//!       takes exactly one argument whose type is `:arg` (RuboCop's
//!       `node.arguments.one?` + `arg_type?`). A lone `optarg`, `restarg`,
//!       `kwarg`, `kwoptarg`, `kwrestarg`, or `blockarg` does NOT qualify, and
//!       a trailing block arg pushes the count past one.
//!   Offense range is `node.loc.name` (the method-name token), so for
//!   `def self.get_thing` the caret lands on `get_thing`, after `self.`.
//!   Verified against rubocop 1.87.0: `get_value(attr)` (reader with args),
//!   `set_value` (writer with zero args), `set_foo(v = 1)` / `set_foo(*v)` /
//!   `set_foo(**o)` / `set_foo(&b)` / `set_foo(a, b)` (non-`:arg` or wrong
//!   arity), and `get_foo!` / `set_foo=(x)` (excluded suffixes) all produce no
//!   offense. Report-only (no autocorrect), matching RuboCop.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG_READER: &str = "Do not prefix reader method names with `get_`.";
const MSG_WRITER: &str = "Do not prefix writer method names with `set_`.";

#[derive(Default)]
pub struct AccessorMethodName;

#[cop(
    name = "Naming/AccessorMethodName",
    description = "Checks the naming of accessor methods for get_/set_.",
    default_severity = "warning",
    default_enabled = true
)]
impl AccessorMethodName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // `descendants` excludes the root node itself; chain it so a single
        // top-level `def` (whose root *is* the `Def` node) is also inspected.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            self.check_def(id, cx);
        }
    }
}

impl AccessorMethodName {
    fn check_def(&self, id: NodeId, cx: &Cx<'_>) {
        if !cx.is_any_def_type(id) {
            return;
        }
        // `method_name` returns the defined name for `Def`/`Defs` (gated above).
        let Some(name) = cx.method_name(id) else {
            return;
        };

        // RuboCop `proper_attribute_name?`: skip `!`, `?`, `=` suffixes.
        if name.ends_with('!') || name.ends_with('?') || name.ends_with('=') {
            return;
        }

        let message = if bad_reader_name(name, id, cx) {
            MSG_READER
        } else if bad_writer_name(name, id, cx) {
            MSG_WRITER
        } else {
            return;
        };

        cx.emit_offense(name_range(id, name, cx), message, None);
    }
}

/// Byte range of the method name within a `def`/`defs` definition, mirroring
/// RuboCop's `node.loc.name`.
///
/// Murphy leaves `loc.name` as `Range::ZERO` for `Def`/`Defs`, so the name is
/// located by its first occurrence in the node's source. The search starts past
/// any singleton receiver (`def self.x` / `def Foo.x`), so a receiver whose
/// source happens to contain the method name as a substring cannot mis-anchor
/// the caret. Beyond the receiver the name always precedes the argument list and
/// body and `def ` never contains a `get_`/`set_` prefix, so the first match is
/// the name. Falls back to the node start (a single-byte caret) if the name is
/// somehow not found.
fn name_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    // Begin the search after the singleton receiver, if any.
    let from = cx
        .def_receiver(id)
        .get()
        .map_or(0, |r| (cx.range(r).end - expr.start) as usize);
    match src[from..].find(name) {
        Some(off) => {
            let start = expr.start + (from + off) as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start,
        },
    }
}

/// RuboCop `bad_reader_name?`: `get_` prefix and no arguments.
fn bad_reader_name(name: &str, id: NodeId, cx: &Cx<'_>) -> bool {
    name.starts_with("get_") && def_arg_children(id, cx).is_empty()
}

/// RuboCop `bad_writer_name?`: `set_` prefix and exactly one positional
/// argument of type `:arg`.
fn bad_writer_name(name: &str, id: NodeId, cx: &Cx<'_>) -> bool {
    name.starts_with("set_")
        && matches!(def_arg_children(id, cx), [arg] if matches!(*cx.kind(*arg), NodeKind::Arg(_)))
}

/// The children of a def's `Args` node (`arg`/`optarg`/`restarg`/`blockarg`/…),
/// or an empty slice for an argument-less def.
fn def_arg_children<'a>(id: NodeId, cx: &Cx<'a>) -> &'a [NodeId] {
    let Some(args) = cx.def_arguments(id).get() else {
        return &[];
    };
    match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::AccessorMethodName;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses (ground-truth carets from rubocop 1.87.0) ---

    #[test]
    fn flags_get_reader() {
        // rubocop: line 1, col 5 (`get_foo`).
        test::<AccessorMethodName>().expect_offense(indoc! {r#"
            def get_foo
                ^^^^^^^ Do not prefix reader method names with `get_`.
            end
        "#});
    }

    #[test]
    fn flags_set_writer() {
        // rubocop: line 1, col 5 (`set_foo`).
        test::<AccessorMethodName>().expect_offense(indoc! {r#"
            def set_foo(value)
                ^^^^^^^ Do not prefix writer method names with `set_`.
            end
        "#});
    }

    #[test]
    fn flags_singleton_get_reader() {
        // `def self.get_thing`: name `get_thing` at col 10 (after `self.`).
        test::<AccessorMethodName>().expect_offense(indoc! {r#"
            def self.get_thing
                     ^^^^^^^^^ Do not prefix reader method names with `get_`.
            end
        "#});
    }

    #[test]
    fn flags_reader_with_empty_parens() {
        // `def get_foo()`: still no arguments → offense at col 5.
        test::<AccessorMethodName>().expect_offense(indoc! {r#"
            def get_foo()
                ^^^^^^^ Do not prefix reader method names with `get_`.
            end
        "#});
    }

    #[test]
    fn flags_singleton_set_writer_on_const_receiver() {
        // `def Config.set_value(v)`: name `set_value` at col 12 (after `Config.`).
        // Pins the receiver-anchored name search.
        test::<AccessorMethodName>().expect_offense(indoc! {r#"
            def Config.set_value(v)
                       ^^^^^^^^^ Do not prefix writer method names with `set_`.
            end
        "#});
    }

    // --- non-offenses (verified against rubocop: NOT flagged) ---

    #[test]
    fn ignores_reader_with_arguments() {
        // `get_value(attr)`: a reader must take no args.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def get_value(attr)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_zero_arguments() {
        // `set_value`: a writer must take exactly one arg.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_value
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_optional_argument() {
        // `set_foo(value = 1)`: optarg is not `:arg`.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(value = 1)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_splat_argument() {
        // `set_foo(*v)`: restarg is not `:arg`.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(*v)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_double_splat_argument() {
        // `set_foo(**o)`: kwrestarg is not `:arg`.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(**o)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_block_argument_only() {
        // `set_foo(&b)`: blockarg is not `:arg`.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(&b)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_two_arguments() {
        // `set_foo(a, b)`: arity must be exactly one.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(a, b)
            end
        "#});
    }

    #[test]
    fn ignores_writer_with_trailing_block_argument() {
        // `set_foo(value, &b)`: blockarg pushes arity past one.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo(value, &b)
            end
        "#});
    }

    #[test]
    fn ignores_reader_with_block_argument() {
        // `get_foo(&b)`: a reader must take no args; a block arg counts.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def get_foo(&b)
            end
        "#});
    }

    #[test]
    fn ignores_bang_suffix() {
        // `get_foo!` / `set_foo!(x)`: `!` suffix is excluded.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def get_foo!
            end
        "#});
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo!(x)
            end
        "#});
    }

    #[test]
    fn ignores_predicate_suffix() {
        // `get_foo?`: `?` suffix is excluded.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def get_foo?
            end
        "#});
    }

    #[test]
    fn ignores_setter_suffix() {
        // `set_foo=(x)`: `=` suffix is excluded.
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def set_foo=(x)
            end
        "#});
    }

    #[test]
    fn ignores_unrelated_method_names() {
        test::<AccessorMethodName>().expect_no_offenses(indoc! {r#"
            def foo
            end

            def bar(value)
            end

            def getter
            end

            def settings(value)
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(AccessorMethodName);
