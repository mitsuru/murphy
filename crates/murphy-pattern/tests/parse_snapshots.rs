//! Parser snapshot tests (murphy-9cr.17, Task 9).
//!
//! Hand-written snapshots: each positive test pins the `{:#?}` debug render
//! of a `parse(src)` result against an expected literal, plus capture
//! metadata where captures are present. Error tests pin the `ParseError`
//! span and a message substring. The expected literals were blessed from a
//! real `cargo test` run — do not hand-edit them.

use murphy_pattern::{CaptureKind, parse};

// =====================================================================
// (A) Positive snapshot tests
// =====================================================================

#[test]
fn snapshot_wildcard() {
    let ast = parse("_").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Wildcard,
        span: PatSpan {
            start: 0,
            end: 1,
        },
    },
    captures: [],
}"#
    );
    assert_eq!(ast.n_captures(), 0);
}

#[test]
fn snapshot_nil_test() {
    let ast = parse("nil?").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: NilTest,
        span: PatSpan {
            start: 0,
            end: 4,
        },
    },
    captures: [],
}"#
    );
    assert_eq!(ast.n_captures(), 0);
}

#[test]
fn snapshot_sym_literal() {
    let ast = parse(":puts").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            Sym(
                "puts",
            ),
        ),
        span: PatSpan {
            start: 0,
            end: 5,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_node_with_nil_test_sym_and_seq_capture() {
    let ast = parse("(send nil? :puts $...)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: [
                Pat {
                    kind: NilTest,
                    span: PatSpan {
                        start: 6,
                        end: 10,
                    },
                },
                Pat {
                    kind: Lit(
                        Sym(
                            "puts",
                        ),
                    ),
                    span: PatSpan {
                        start: 11,
                        end: 16,
                    },
                },
                Pat {
                    kind: Capture {
                        slot: 0,
                        name: None,
                        body: Pat {
                            kind: Rest,
                            span: PatSpan {
                                start: 18,
                                end: 21,
                            },
                        },
                    },
                    span: PatSpan {
                        start: 17,
                        end: 21,
                    },
                },
            ],
        },
        span: PatSpan {
            start: 0,
            end: 22,
        },
    },
    captures: [
        Seq,
    ],
}"#
    );
    assert_eq!(ast.n_captures(), 1);
    assert_eq!(ast.capture_kinds(), &[CaptureKind::Seq]);
}

#[test]
fn snapshot_oneof_head_and_rest() {
    let ast = parse("({send csend} _ ...)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Node {
            head: OneOf(
                [
                    NodeKindTag(
                        17,
                    ),
                    NodeKindTag(
                        18,
                    ),
                ],
            ),
            children: [
                Pat {
                    kind: Wildcard,
                    span: PatSpan {
                        start: 14,
                        end: 15,
                    },
                },
                Pat {
                    kind: Rest,
                    span: PatSpan {
                        start: 16,
                        end: 19,
                    },
                },
            ],
        },
        span: PatSpan {
            start: 0,
            end: 20,
        },
    },
    captures: [],
}"#
    );
    assert_eq!(ast.n_captures(), 0);
}

#[test]
fn snapshot_union() {
    let ast = parse("{int float}").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Union(
            [
                Pat {
                    kind: Kind(
                        NodeKindTag(
                            5,
                        ),
                    ),
                    span: PatSpan {
                        start: 1,
                        end: 4,
                    },
                },
                Pat {
                    kind: Kind(
                        NodeKindTag(
                            6,
                        ),
                    ),
                    span: PatSpan {
                        start: 5,
                        end: 10,
                    },
                },
            ],
        ),
        span: PatSpan {
            start: 0,
            end: 11,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_not() {
    let ast = parse("!(send _ :x)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Not(
            Pat {
                kind: Node {
                    head: Exact(
                        NodeKindTag(
                            17,
                        ),
                    ),
                    children: [
                        Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 7,
                                end: 8,
                            },
                        },
                        Pat {
                            kind: Lit(
                                Sym(
                                    "x",
                                ),
                            ),
                            span: PatSpan {
                                start: 9,
                                end: 11,
                            },
                        },
                    ],
                },
                span: PatSpan {
                    start: 1,
                    end: 12,
                },
            },
        ),
        span: PatSpan {
            start: 0,
            end: 12,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_parent() {
    let ast = parse("^(def _ _ _)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Parent(
            Pat {
                kind: Node {
                    head: Exact(
                        NodeKindTag(
                            32,
                        ),
                    ),
                    children: [
                        Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 6,
                                end: 7,
                            },
                        },
                        Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 8,
                                end: 9,
                            },
                        },
                        Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 10,
                                end: 11,
                            },
                        },
                    ],
                },
                span: PatSpan {
                    start: 1,
                    end: 12,
                },
            },
        ),
        span: PatSpan {
            start: 0,
            end: 12,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_descend() {
    let ast = parse("`(send nil? :raise)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Descend(
            Pat {
                kind: Node {
                    head: Exact(
                        NodeKindTag(
                            17,
                        ),
                    ),
                    children: [
                        Pat {
                            kind: NilTest,
                            span: PatSpan {
                                start: 7,
                                end: 11,
                            },
                        },
                        Pat {
                            kind: Lit(
                                Sym(
                                    "raise",
                                ),
                            ),
                            span: PatSpan {
                                start: 12,
                                end: 18,
                            },
                        },
                    ],
                },
                span: PatSpan {
                    start: 1,
                    end: 19,
                },
            },
        ),
        span: PatSpan {
            start: 0,
            end: 19,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_named_capture_and_predicate() {
    let ast = parse("(send $receiver #pred?)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: [
                Pat {
                    kind: Capture {
                        slot: 0,
                        name: Some(
                            "receiver",
                        ),
                        body: Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 6,
                                end: 7,
                            },
                        },
                    },
                    span: PatSpan {
                        start: 6,
                        end: 15,
                    },
                },
                Pat {
                    kind: Predicate(
                        "pred?",
                    ),
                    span: PatSpan {
                        start: 16,
                        end: 22,
                    },
                },
            ],
        },
        span: PatSpan {
            start: 0,
            end: 23,
        },
    },
    captures: [
        Node,
    ],
}"#
    );
    assert_eq!(ast.n_captures(), 1);
    assert_eq!(ast.capture_kinds(), &[CaptureKind::Node]);
}

#[test]
fn snapshot_bare_kind_name() {
    let ast = parse("send").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Kind(
            NodeKindTag(
                17,
            ),
        ),
        span: PatSpan {
            start: 0,
            end: 4,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_int_literal() {
    let ast = parse("42").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            Int(
                42,
            ),
        ),
        span: PatSpan {
            start: 0,
            end: 2,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_float_literal() {
    let ast = parse("1.5").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            Float(
                1.5,
            ),
        ),
        span: PatSpan {
            start: 0,
            end: 3,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_str_literal() {
    let ast = parse("\"hello\"").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            Str(
                "hello",
            ),
        ),
        span: PatSpan {
            start: 0,
            end: 7,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_true_false_nil_literals() {
    let t = parse("true").unwrap();
    assert_eq!(
        format!("{t:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            True,
        ),
        span: PatSpan {
            start: 0,
            end: 4,
        },
    },
    captures: [],
}"#
    );
    let f = parse("false").unwrap();
    assert_eq!(
        format!("{f:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            False,
        ),
        span: PatSpan {
            start: 0,
            end: 5,
        },
    },
    captures: [],
}"#
    );
    let n = parse("nil").unwrap();
    assert_eq!(
        format!("{n:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Lit(
            Nil,
        ),
        span: PatSpan {
            start: 0,
            end: 3,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_any_head() {
    let ast = parse("(_ _)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Node {
            head: Any,
            children: [
                Pat {
                    kind: Wildcard,
                    span: PatSpan {
                        start: 3,
                        end: 4,
                    },
                },
            ],
        },
        span: PatSpan {
            start: 0,
            end: 5,
        },
    },
    captures: [],
}"#
    );
}

#[test]
fn snapshot_named_and_anonymous_capture_mix() {
    // `$recv` is a named capture (slot 0); `$_` is an anonymous node
    // capture (slot 1) — both forms in a single pattern.
    let ast = parse("(send $recv $_)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: [
                Pat {
                    kind: Capture {
                        slot: 0,
                        name: Some(
                            "recv",
                        ),
                        body: Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 6,
                                end: 7,
                            },
                        },
                    },
                    span: PatSpan {
                        start: 6,
                        end: 11,
                    },
                },
                Pat {
                    kind: Capture {
                        slot: 1,
                        name: None,
                        body: Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 13,
                                end: 14,
                            },
                        },
                    },
                    span: PatSpan {
                        start: 12,
                        end: 14,
                    },
                },
            ],
        },
        span: PatSpan {
            start: 0,
            end: 15,
        },
    },
    captures: [
        Node,
        Node,
    ],
}"#
    );
    assert_eq!(ast.n_captures(), 2);
    assert_eq!(ast.capture_kinds(), &[CaptureKind::Node, CaptureKind::Node]);
}

#[test]
fn snapshot_nested_capture_slot_order() {
    // `$(send $inner _)` — outer anonymous capture is slot 0, the inner
    // named capture `$inner` is slot 1: source order (outer-before-inner),
    // not post-order. Pinned both in the slot fields of the snapshot and
    // explicitly in the slot assertions below.
    let ast = parse("$(send $inner _)").unwrap();
    assert_eq!(
        format!("{ast:#?}"),
        r#"PatternAst {
    root: Pat {
        kind: Capture {
            slot: 0,
            name: None,
            body: Pat {
                kind: Node {
                    head: Exact(
                        NodeKindTag(
                            17,
                        ),
                    ),
                    children: [
                        Pat {
                            kind: Capture {
                                slot: 1,
                                name: Some(
                                    "inner",
                                ),
                                body: Pat {
                                    kind: Wildcard,
                                    span: PatSpan {
                                        start: 7,
                                        end: 8,
                                    },
                                },
                            },
                            span: PatSpan {
                                start: 7,
                                end: 13,
                            },
                        },
                        Pat {
                            kind: Wildcard,
                            span: PatSpan {
                                start: 14,
                                end: 15,
                            },
                        },
                    ],
                },
                span: PatSpan {
                    start: 1,
                    end: 16,
                },
            },
        },
        span: PatSpan {
            start: 0,
            end: 16,
        },
    },
    captures: [
        Node,
        Node,
    ],
}"#
    );
    assert_eq!(ast.n_captures(), 2);
    assert_eq!(ast.capture_kinds(), &[CaptureKind::Node, CaptureKind::Node]);
}

// =====================================================================
// (B) Error tests — span and message
// =====================================================================

#[test]
fn error_unknown_node_type() {
    // `sned` is not a node type; the span covers the whole identifier.
    let err = parse("sned").unwrap_err();
    assert_eq!((err.span.start, err.span.end), (0, 4));
    assert!(
        err.message.contains("unknown node type") && err.message.contains("sned"),
        "message was: {}",
        err.message
    );
}

#[test]
fn error_empty_union() {
    // `{}` — an empty union; the span covers `{` through `}`.
    let err = parse("{}").unwrap_err();
    assert_eq!((err.span.start, err.span.end), (0, 2));
    assert!(
        err.message.to_lowercase().contains("union"),
        "message was: {}",
        err.message
    );
}

#[test]
fn error_duplicate_rest_in_child_list() {
    // Two `...` in one node child list; the span points at the second `...`.
    let err = parse("(array ... ...)").unwrap_err();
    assert_eq!((err.span.start, err.span.end), (11, 14));
    assert!(err.message.contains("..."), "message was: {}", err.message);
}

#[test]
fn error_duplicate_capture_name() {
    // `(send $x $x)` — the duplicate `$x`; the span points at the second
    // `x` identifier (bytes 10..11).
    let err = parse("(send $x $x)").unwrap_err();
    assert_eq!((err.span.start, err.span.end), (10, 11));
    assert!(
        err.message.contains("duplicate capture name") && err.message.contains('x'),
        "message was: {}",
        err.message
    );
}

#[test]
fn error_unclosed_paren() {
    // `(send` — runs out of input before the closing `)`; the span points
    // at the opening `(` (bytes 0..1).
    let err = parse("(send").unwrap_err();
    assert_eq!((err.span.start, err.span.end), (0, 1));
    assert!(
        err.message.contains("unclosed `(`"),
        "message was: {}",
        err.message
    );
}
