//! Lowering snapshot tests (murphy-9cr.17, Task 12).
//!
//! Each positive test pins the `{:#?}` debug render of a
//! `compile(src)` result (= `lower(parse(src))`) against an expected
//! literal, plus capture metadata where captures are present. The
//! expected literals were blessed from a real `cargo test` run — do not
//! hand-edit them. They cover the same v1 grammar inputs as the Task 9
//! parser snapshots.

use murphy_pattern::CaptureKind;

// =====================================================================
// (A) `compile` API tests
// =====================================================================

#[test]
fn compile_runs_parse_then_lower() {
    let ir = murphy_pattern::compile("(send nil? :puts $...)").expect("ok");
    assert_eq!(ir.captures.len(), 1);
}

#[test]
fn compile_propagates_parse_error() {
    assert!(murphy_pattern::compile("(sned _)").is_err());
}

// =====================================================================
// (B) Lowering snapshot tests
// =====================================================================

#[test]
fn snapshot_wildcard() {
    let ir = murphy_pattern::compile("_").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Wildcard,
    ],
    children: [],
    tags: [],
    str_pool: "",
    captures: [],
    root: IrNodeId(
        0,
    ),
}"#
    );
}

#[test]
fn snapshot_sym_literal() {
    let ir = murphy_pattern::compile(":puts").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        LitSym(
            StrRef {
                start: 0,
                len: 4,
            },
        ),
    ],
    children: [],
    tags: [],
    str_pool: "puts",
    captures: [],
    root: IrNodeId(
        0,
    ),
}"#
    );
}

#[test]
fn snapshot_node_with_nil_test_sym_and_seq_capture() {
    let ir = murphy_pattern::compile("(send nil? :puts $...)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        NilTest,
        LitSym(
            StrRef {
                start: 0,
                len: 4,
            },
        ),
        Rest,
        Capture {
            slot: 0,
            body: IrNodeId(
                2,
            ),
        },
        Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: IrSlice {
                start: 0,
                len: 3,
            },
        },
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
        IrNodeId(
            3,
        ),
    ],
    tags: [],
    str_pool: "puts",
    captures: [
        CaptureMeta {
            kind: Seq,
            name: None,
        },
    ],
    root: IrNodeId(
        4,
    ),
}"#
    );
    assert_eq!(ir.captures.len(), 1);
    assert_eq!(ir.captures[0].kind, CaptureKind::Seq);
    assert!(ir.captures[0].name.is_none());
}

#[test]
fn snapshot_oneof_head_and_rest() {
    let ir = murphy_pattern::compile("({send csend} _ ...)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Wildcard,
        Rest,
        Node {
            head: OneOf(
                IrSlice {
                    start: 0,
                    len: 2,
                },
            ),
            children: IrSlice {
                start: 0,
                len: 2,
            },
        },
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
    ],
    tags: [
        NodeKindTag(
            17,
        ),
        NodeKindTag(
            18,
        ),
    ],
    str_pool: "",
    captures: [],
    root: IrNodeId(
        2,
    ),
}"#
    );
    assert!(ir.captures.is_empty());
}

#[test]
fn snapshot_union() {
    let ir = murphy_pattern::compile("{int float}").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Kind(
            NodeKindTag(
                5,
            ),
        ),
        Kind(
            NodeKindTag(
                6,
            ),
        ),
        Union(
            IrSlice {
                start: 0,
                len: 2,
            },
        ),
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
    ],
    tags: [],
    str_pool: "",
    captures: [],
    root: IrNodeId(
        2,
    ),
}"#
    );
}

#[test]
fn snapshot_not() {
    let ir = murphy_pattern::compile("!(send _ :x)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Wildcard,
        LitSym(
            StrRef {
                start: 0,
                len: 1,
            },
        ),
        Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: IrSlice {
                start: 0,
                len: 2,
            },
        },
        Not(
            IrNodeId(
                2,
            ),
        ),
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
    ],
    tags: [],
    str_pool: "x",
    captures: [],
    root: IrNodeId(
        3,
    ),
}"#
    );
}

#[test]
fn snapshot_parent() {
    let ir = murphy_pattern::compile("^(def _ _ _)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Wildcard,
        Wildcard,
        Wildcard,
        Node {
            head: Exact(
                NodeKindTag(
                    32,
                ),
            ),
            children: IrSlice {
                start: 0,
                len: 3,
            },
        },
        Parent(
            IrNodeId(
                3,
            ),
        ),
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
        IrNodeId(
            2,
        ),
    ],
    tags: [],
    str_pool: "",
    captures: [],
    root: IrNodeId(
        4,
    ),
}"#
    );
}

#[test]
fn snapshot_descend() {
    let ir = murphy_pattern::compile("`(send nil? :raise)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        NilTest,
        LitSym(
            StrRef {
                start: 0,
                len: 5,
            },
        ),
        Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: IrSlice {
                start: 0,
                len: 2,
            },
        },
        Descend(
            IrNodeId(
                2,
            ),
        ),
    ],
    children: [
        IrNodeId(
            0,
        ),
        IrNodeId(
            1,
        ),
    ],
    tags: [],
    str_pool: "raise",
    captures: [],
    root: IrNodeId(
        3,
    ),
}"#
    );
}

#[test]
fn snapshot_named_capture_and_predicate() {
    let ir = murphy_pattern::compile("(send $receiver #pred?)").unwrap();
    assert_eq!(
        format!("{ir:#?}"),
        r#"PatternIr {
    nodes: [
        Wildcard,
        Capture {
            slot: 0,
            body: IrNodeId(
                0,
            ),
        },
        Predicate(
            StrRef {
                start: 8,
                len: 5,
            },
        ),
        Node {
            head: Exact(
                NodeKindTag(
                    17,
                ),
            ),
            children: IrSlice {
                start: 0,
                len: 2,
            },
        },
    ],
    children: [
        IrNodeId(
            1,
        ),
        IrNodeId(
            2,
        ),
    ],
    tags: [],
    str_pool: "receiverpred?",
    captures: [
        CaptureMeta {
            kind: Node,
            name: Some(
                StrRef {
                    start: 0,
                    len: 8,
                },
            ),
        },
    ],
    root: IrNodeId(
        3,
    ),
}"#
    );
    assert_eq!(ir.captures.len(), 1);
    assert_eq!(ir.captures[0].kind, CaptureKind::Node);
    let name = ir.captures[0].name.expect("named");
    assert_eq!(
        &ir.str_pool[name.start as usize..(name.start + name.len) as usize],
        "receiver"
    );
}
