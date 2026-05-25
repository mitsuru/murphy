use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, node_pattern};

node_pattern!(
    is_deprecated_env_method,
    "(send (const nil? :ENV) {:clone :dup :freeze})"
);
node_pattern!(
    is_deprecated_exists_method,
    "(send (const nil? {:File :Dir :FileTest}) :exists? _)"
);
node_pattern!(
    is_deprecated_socket_method,
    "(send (const nil? :Socket) {:gethostbyaddr :gethostbyname} ...)"
);
node_pattern!(
    is_deprecated_attr_method,
    "(send nil? :attr _ {true false})"
);
node_pattern!(is_deprecated_iterator_method, "(send nil? :iterator?)");

#[derive(Default)]
pub struct DeprecatedClassMethods;

#[cop(
    name = "Lint/DeprecatedClassMethods",
    description = "Flag deprecated class method calls with safe replacements.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DeprecatedClassMethods {
    #[on_node(
        kind = "send",
        methods = [
            "attr",
            "clone",
            "dup",
            "exists?",
            "freeze",
            "gethostbyaddr",
            "gethostbyname",
            "iterator?"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_deprecated_class_method(node, cx) {
            return;
        };

        let Some(offense) = offense(node, cx) else {
            return;
        };
        cx.emit_offense(offense.range, &offense.message, None);
        if let Some(replacement) = offense.replacement {
            cx.emit_edit(replacement.range, &replacement.text);
        }
    }
}

fn is_deprecated_class_method(node: NodeId, cx: &Cx<'_>) -> bool {
    is_deprecated_env_method(node, cx)
        || is_deprecated_exists_method(node, cx)
        || is_deprecated_socket_method(node, cx)
        || is_deprecated_attr_method(node, cx)
        || is_deprecated_iterator_method(node, cx)
}

struct DeprecatedOffense {
    range: Range,
    message: String,
    replacement: Option<Replacement>,
}

struct Replacement {
    range: Range,
    text: String,
}

fn offense(node: NodeId, cx: &Cx<'_>) -> Option<DeprecatedOffense> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    let method = cx.symbol_str(method);

    if method == "attr" {
        let boolean_arg = cx.list(args).get(1).copied()?;
        let preferred = match *cx.kind(boolean_arg) {
            NodeKind::True_ => "attr_accessor",
            NodeKind::False_ => "attr_reader",
            _ => return None,
        };
        let first_arg = cx.list(args).first().copied()?;
        let replacement = format!("{preferred} {}", cx.raw_source(cx.range(first_arg)));
        return Some(DeprecatedOffense {
            range: cx.range(node),
            message: format!(
                "`{}` is deprecated in favor of `{replacement}`.",
                cx.raw_source(cx.range(node))
            ),
            replacement: Some(Replacement {
                range: cx.range(node),
                text: replacement,
            }),
        });
    }

    if method == "iterator?" {
        let range = cx.range(node);
        return Some(DeprecatedOffense {
            range,
            message: "`iterator?` is deprecated in favor of `block_given?`.".to_string(),
            replacement: Some(Replacement {
                range,
                text: "block_given?".to_string(),
            }),
        });
    }

    let receiver = receiver.get()?;
    let const_name = top_level_const_name(cx, receiver)?;
    match (const_name, method) {
        ("File" | "Dir" | "FileTest", "exists?") => {
            // RuboCop-style range: `receiver.selector` only, excluding
            // arguments — `File.exists?(path)` flags `File.exists?`.
            let range = receiver_selector_range(cx, node)?;
            let selector = cx.node(node).loc.name;
            Some(DeprecatedOffense {
                range,
                message: "Use `exist?` instead of deprecated `exists?`".to_string(),
                replacement: Some(Replacement {
                    range: selector,
                    text: "exist?".to_string(),
                }),
            })
        }
        ("ENV", "clone" | "dup") => {
            let range = receiver_selector_range(cx, node)?;
            let preferred = "ENV.to_h";
            Some(DeprecatedOffense {
                range,
                message: format!(
                    "`{}` is deprecated in favor of `{preferred}`.",
                    cx.raw_source(range)
                ),
                replacement: Some(Replacement {
                    range: cx.range(node),
                    text: preferred.to_string(),
                }),
            })
        }
        ("ENV", "freeze") => {
            let range = receiver_selector_range(cx, node)?;
            Some(DeprecatedOffense {
                range,
                message: format!(
                    "`{}` is deprecated in favor of `ENV`.",
                    cx.raw_source(range)
                ),
                replacement: Some(Replacement {
                    range: cx.range(node),
                    text: "ENV".to_string(),
                }),
            })
        }
        ("Socket", "gethostbyaddr") => socket_offense(node, cx, "Addrinfo#getnameinfo"),
        ("Socket", "gethostbyname") => socket_offense(node, cx, "Addrinfo.getaddrinfo"),
        _ => None,
    }
}

fn top_level_const_name<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return None;
    };
    if scope.is_none() {
        Some(cx.symbol_str(name))
    } else {
        None
    }
}

fn socket_offense(node: NodeId, cx: &Cx<'_>, preferred: &str) -> Option<DeprecatedOffense> {
    let range = receiver_selector_range(cx, node)?;
    Some(DeprecatedOffense {
        range,
        message: format!(
            "`{}` is deprecated in favor of `{preferred}`.",
            cx.raw_source(range)
        ),
        replacement: None,
    })
}

fn receiver_selector_range(cx: &Cx<'_>, node: NodeId) -> Option<Range> {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return None;
    };
    let receiver = receiver.get()?;
    Some(Range {
        start: cx.range(receiver).start,
        end: cx.node(node).loc.name.end,
    })
}

#[cfg(test)]
mod tests {
    use super::DeprecatedClassMethods;
    use murphy_plugin_api::{
        Range,
        test_support::{indoc, run_cop_with_edits, test},
    };

    #[test]
    fn flags_file_exists_and_filetest_exists() {
        // RuboCop-style range: receiver.selector only, no args.
        test::<DeprecatedClassMethods>().expect_offense(indoc! {r#"
            File.exists?(path)
            ^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
            Dir.exists?(path)
            ^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
            FileTest.exists?(path)
            ^^^^^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
        "#});
    }

    #[test]
    fn flags_rubocop_deprecated_class_method_shapes() {
        test::<DeprecatedClassMethods>().expect_offense(indoc! {r#"
            ENV.freeze
            ^^^^^^^^^^ `ENV.freeze` is deprecated in favor of `ENV`.
            Socket.gethostbyname(host)
            ^^^^^^^^^^^^^^^^^^^^ `Socket.gethostbyname` is deprecated in favor of `Addrinfo.getaddrinfo`.
            iterator?
            ^^^^^^^^^ `iterator?` is deprecated in favor of `block_given?`.
            attr :name, true
            ^^^^^^^^^^^^^^^^ `attr :name, true` is deprecated in favor of `attr_accessor :name`.
        "#});
    }

    // murphy-h03f: cbase (::X) modifier + receiver.selector range.

    #[test]
    fn flags_cbase_env_freeze() {
        // `::ENV` and bare `ENV` collapse to the same `Const{scope:None}`
        // in Murphy's AST, so the existing pattern already accepts both.
        // This test pins that contract so a future AST that splits the
        // shapes can't silently regress us.
        test::<DeprecatedClassMethods>().expect_offense(indoc! {r#"
                ::ENV.freeze
                ^^^^^^^^^^^^ `::ENV.freeze` is deprecated in favor of `ENV`.
            "#});
    }

    #[test]
    fn flags_cbase_file_exists_with_receiver_selector_range() {
        // Range covers `::File.exists?`, not the whole call with args.
        test::<DeprecatedClassMethods>().expect_offense(indoc! {r#"
                ::File.exists?(path)
                ^^^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
            "#});
    }

    #[test]
    fn flags_file_exists_uses_receiver_selector_range() {
        test::<DeprecatedClassMethods>().expect_offense(indoc! {r#"
                File.exists?(path)
                ^^^^^^^^^^^^ Use `exist?` instead of deprecated `exists?`
            "#});
    }

    #[test]
    fn autocorrects_selector_only_and_reaches_fixpoint() {
        let run = run_cop_with_edits::<DeprecatedClassMethods>("File.exists?(path)\n");
        assert_eq!(run.edits[0].range, Range { start: 5, end: 12 });
        assert_eq!(run.edits[0].replacement, "exist?");
        test::<DeprecatedClassMethods>()
            .expect_no_offenses("File.exist?(path)\n名前 = File.exist?(path)\n");
    }
}
