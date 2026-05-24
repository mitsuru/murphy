# Sorted Tokens Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose a RuboCop-style source-ordered token stream on Murphy's arena AST and native cop context.

**Architecture:** `murphy-prism::parse_with_tokens()` remains the Prism-facing source of truth. `murphy-translate` copies Prism tokens into owned `murphy-ast::SourceToken` values, and `murphy-plugin-api::Cx` exposes the same slice to cops through append-only `CxRaw` fields.

**Tech Stack:** Rust workspace, `murphy-prism`, `murphy-translate`, `murphy-ast`, `murphy-plugin-api`, `murphy-core`, Cargo tests.

---

## File Structure

- Modify `crates/murphy-ast/src/node.rs`: add `SourceToken` and `SourceTokenKind`.
- Modify `crates/murphy-ast/src/builder.rs`: store and append source tokens.
- Modify `crates/murphy-ast/src/ast.rs`: store tokens, expose `sorted_tokens()`, include tokens in `AstRawParts`, and update raw-parts tests.
- Modify `crates/murphy-ast/src/lib.rs`: re-export token types.
- Modify `crates/murphy-ast/src/serialize.rs`: bump `FORMAT_VERSION`, serialize/deserialize tokens, and test round-trip plus bad token kind.
- Modify `crates/murphy-translate/src/translate.rs`: call `parse_with_tokens()`, map Prism token types, and test token order/ranges.
- Modify `crates/murphy-plugin-api/src/abi.rs`: append token pointer/length fields to `CxRaw` and update layout tests.
- Modify `crates/murphy-plugin-api/src/cx.rs`: expose `Cx::sorted_tokens()` and update test builders/tests.
- Modify `crates/murphy-plugin-api/src/internal.rs` and `crates/murphy-plugin-api/src/test_support.rs`: populate new raw fields in test helpers.
- Modify `crates/murphy-core/src/dispatch.rs`: populate new raw fields in host dispatch.
- Run focused crate tests first, then workspace tests.

### Task 1: Add SourceToken Storage to murphy-ast

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`
- Modify: `crates/murphy-ast/src/builder.rs`
- Modify: `crates/murphy-ast/src/ast.rs`
- Modify: `crates/murphy-ast/src/lib.rs`

- [ ] **Step 1: Write failing AST storage test**

Add a test to `crates/murphy-ast/src/ast.rs`:

```rust
#[test]
fn sorted_tokens_and_raw_parts_borrow_the_arena_storage() {
    use crate::builder::AstBuilder;
    use crate::node::{SourceToken, SourceTokenKind};

    let mut b = AstBuilder::new("foo(1)", "t.rb");
    let one = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
    b.add_source_token(SourceToken {
        kind: SourceTokenKind::LeftParen,
        range: Range { start: 3, end: 4 },
    });
    b.add_source_token(SourceToken {
        kind: SourceTokenKind::RightParen,
        range: Range { start: 5, end: 6 },
    });
    let ast = b.finish(one);

    assert_eq!(
        ast.sorted_tokens(),
        &[
            SourceToken {
                kind: SourceTokenKind::LeftParen,
                range: Range { start: 3, end: 4 },
            },
            SourceToken {
                kind: SourceTokenKind::RightParen,
                range: Range { start: 5, end: 6 },
            },
        ]
    );
    assert_eq!(ast.raw_parts().sorted_tokens, ast.sorted_tokens());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p murphy-ast sorted_tokens_and_raw_parts_borrow_the_arena_storage
```

Expected: FAIL because `SourceToken`, `SourceTokenKind`, `add_source_token`,
`sorted_tokens`, and `AstRawParts::sorted_tokens` do not exist.

- [ ] **Step 3: Implement minimal AST token storage**

Add to `crates/murphy-ast/src/node.rs` after `Range`:

```rust
/// A compact source token for RuboCop-style layout cops.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceToken {
    pub range: Range,
    pub kind: SourceTokenKind,
}

/// Murphy's compact classification of Prism source tokens.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceTokenKind {
    LeftParen,
    RightParen,
    Comment,
    Newline,
    IgnoredNewline,
    HeredocStart,
    HeredocEnd,
    Other,
}
```

Update `crates/murphy-ast/src/builder.rs`:

```rust
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    SourceToken, StringId, Symbol,
};

pub struct AstBuilder {
    nodes: Vec<AstNode>,
    node_lists: Vec<NodeId>,
    interner: InternBuilder,
    comments: Vec<Comment>,
    source_tokens: Vec<SourceToken>,
    source: SourceBuffer,
}

pub fn add_source_token(&mut self, token: SourceToken) {
    self.source_tokens.push(token);
}
```

Include `source_tokens` in `AstBuilder::new()` and `AstBuilder::finish()`.

Update `crates/murphy-ast/src/ast.rs`:

```rust
use crate::node::{
    AstNode, Comment, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer, SourceToken,
};

pub struct Ast {
    pub(crate) nodes: Vec<AstNode>,
    pub(crate) node_lists: Vec<NodeId>,
    pub(crate) interner: Interner,
    pub(crate) comments: Vec<Comment>,
    pub(crate) source_tokens: Vec<SourceToken>,
    pub(crate) source: SourceBuffer,
    pub(crate) root: NodeId,
}

pub fn sorted_tokens(&self) -> &[SourceToken] {
    &self.source_tokens
}
```

Add `sorted_tokens` to `AstRawParts` and `raw_parts()`.

Update `crates/murphy-ast/src/lib.rs` re-exports:

```rust
pub use node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    SourceToken, SourceTokenKind, StringId, Symbol,
};
```

- [ ] **Step 4: Run focused AST test**

Run:

```bash
cargo test -p murphy-ast sorted_tokens_and_raw_parts_borrow_the_arena_storage
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-ast/src/node.rs crates/murphy-ast/src/builder.rs crates/murphy-ast/src/ast.rs crates/murphy-ast/src/lib.rs
git commit -m "feat(ast): store sorted source tokens"
```

### Task 2: Serialize Source Tokens

**Files:**
- Modify: `crates/murphy-ast/src/serialize.rs`

- [ ] **Step 1: Write failing serialization tests**

Add tests in `crates/murphy-ast/src/serialize.rs`:

```rust
#[test]
fn round_trip_source_tokens() {
    let mut b = crate::AstBuilder::new("foo(1)\n", "t.rb");
    let root = b.push(crate::NodeKind::Int(1), r(4, 5));
    b.add_source_token(crate::SourceToken {
        kind: crate::SourceTokenKind::LeftParen,
        range: r(3, 4),
    });
    b.add_source_token(crate::SourceToken {
        kind: crate::SourceTokenKind::Newline,
        range: r(6, 7),
    });
    let ast = b.finish(root);

    let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
    assert_eq!(restored.sorted_tokens(), ast.sorted_tokens());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p murphy-ast round_trip_source_tokens
```

Expected: FAIL until serialization includes `source_tokens`.

- [ ] **Step 3: Implement token serialization**

In `serialize.rs`, import token types:

```rust
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    SourceToken, SourceTokenKind, StringId, Symbol,
};
```

Bump:

```rust
pub const FORMAT_VERSION: u32 = 2;
```

Add helpers:

```rust
fn write_source_token(t: SourceToken, out: &mut Vec<u8>) {
    write_range(t.range, out);
    put_u8(out, t.kind as u8);
}

fn read_source_token(cur: &mut &[u8]) -> Result<SourceToken, SerError> {
    let range = read_range(cur)?;
    let kind = match get_u8(cur)? {
        0 => SourceTokenKind::LeftParen,
        1 => SourceTokenKind::RightParen,
        2 => SourceTokenKind::Comment,
        3 => SourceTokenKind::Newline,
        4 => SourceTokenKind::IgnoredNewline,
        5 => SourceTokenKind::HeredocStart,
        6 => SourceTokenKind::HeredocEnd,
        7 => SourceTokenKind::Other,
        _ => return Err(SerError::BadDiscriminant),
    };
    Ok(SourceToken { range, kind })
}
```

Write a token section after comments and before source text. Read it in the
same position and set `Ast { source_tokens, ... }`.

- [ ] **Step 4: Run focused serialization tests**

Run:

```bash
cargo test -p murphy-ast round_trip_source_tokens
cargo test -p murphy-ast serialize::tests::from_bytes_rejects_format_version_mismatch
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-ast/src/serialize.rs
git commit -m "feat(ast): serialize sorted source tokens"
```

### Task 3: Translate Prism Tokens into Arena Tokens

**Files:**
- Modify: `crates/murphy-translate/src/translate.rs`

- [ ] **Step 1: Write failing translate tests**

Add tests near existing comment tests:

```rust
#[test]
fn translates_sorted_tokens_for_layout_punctuation_and_comments() {
    let ast = translate("foo(1) # c\nbar(\n  2\n)\n", "t.rb");
    let tokens: Vec<_> = ast
        .sorted_tokens()
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                murphy_ast::SourceTokenKind::LeftParen
                    | murphy_ast::SourceTokenKind::RightParen
                    | murphy_ast::SourceTokenKind::Comment
                    | murphy_ast::SourceTokenKind::Newline
            )
        })
        .map(|t| (t.kind, ast.raw_source(t.range).to_string()))
        .collect();

    assert!(tokens.contains(&(murphy_ast::SourceTokenKind::LeftParen, "(".to_string())));
    assert!(tokens.contains(&(murphy_ast::SourceTokenKind::RightParen, ")".to_string())));
    assert!(tokens.contains(&(murphy_ast::SourceTokenKind::Comment, "# c".to_string())));
    assert!(tokens.contains(&(murphy_ast::SourceTokenKind::Newline, "\n".to_string())));
    assert!(ast.sorted_tokens().windows(2).all(|pair| pair[0].range.start <= pair[1].range.start));
}

#[test]
fn translates_ignored_newline_and_heredoc_tokens() {
    let ast = translate("foo( <<~HEREDOC )\nbody\nHEREDOC\n", "t.rb");
    let kinds: Vec<_> = ast.sorted_tokens().iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&murphy_ast::SourceTokenKind::IgnoredNewline));
    assert!(kinds.contains(&murphy_ast::SourceTokenKind::HeredocStart));
    assert!(kinds.contains(&murphy_ast::SourceTokenKind::HeredocEnd));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p murphy-translate translates_sorted_tokens
```

Expected: FAIL because `translate()` has not populated tokens.

- [ ] **Step 3: Implement token mapping**

Update imports:

```rust
use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range, SourceToken, SourceTokenKind};
```

Update `translate()`:

```rust
let result = prism::parse_with_tokens(source.as_bytes());
let mut t = Translator {
    builder: AstBuilder::new(source, path),
};
let root = t.translate_program(&result.parse().node());
for token in result.tokens() {
    t.builder.add_source_token(SourceToken {
        kind: Translator::source_token_kind(token.type_()),
        range: Range {
            start: token.start_offset() as u32,
            end: token.end_offset() as u32,
        },
    });
}
for c in result.parse().comments() {
    let loc = c.location();
    let range = Translator::range(&loc);
    let kind = match c.type_() {
        prism::CommentType::InlineComment => murphy_ast::CommentKind::Inline,
        _ => murphy_ast::CommentKind::Block,
    };
    t.builder.add_comment(range, kind);
}
```

Add helper:

```rust
fn source_token_kind(kind: prism::pm_token_type_t) -> SourceTokenKind {
    match kind {
        prism::PM_TOKEN_PARENTHESIS_LEFT | prism::PM_TOKEN_PARENTHESIS_LEFT_PARENTHESES => {
            SourceTokenKind::LeftParen
        }
        prism::PM_TOKEN_PARENTHESIS_RIGHT => SourceTokenKind::RightParen,
        prism::PM_TOKEN_COMMENT => SourceTokenKind::Comment,
        prism::PM_TOKEN_NEWLINE => SourceTokenKind::Newline,
        prism::PM_TOKEN_IGNORED_NEWLINE => SourceTokenKind::IgnoredNewline,
        prism::PM_TOKEN_HEREDOC_START => SourceTokenKind::HeredocStart,
        prism::PM_TOKEN_HEREDOC_END => SourceTokenKind::HeredocEnd,
        _ => SourceTokenKind::Other,
    }
}
```

- [ ] **Step 4: Run focused translate tests**

Run:

```bash
cargo test -p murphy-translate translates_sorted_tokens
cargo test -p murphy-translate translates_ignored_newline_and_heredoc_tokens
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-translate/src/translate.rs
git commit -m "feat(translate): thread Prism tokens into arena"
```

### Task 4: Expose Tokens Through Plugin Cx

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs`
- Modify: `crates/murphy-plugin-api/src/cx.rs`
- Modify: `crates/murphy-plugin-api/src/internal.rs`
- Modify: `crates/murphy-plugin-api/src/test_support.rs`
- Modify: `crates/murphy-core/src/dispatch.rs`

- [ ] **Step 1: Write failing Cx test**

Add to `crates/murphy-plugin-api/src/cx.rs` tests:

```rust
#[test]
fn sorted_tokens_match_the_underlying_ast() {
    let mut b = AstBuilder::new("foo(1)", "t.rb".to_string());
    let root = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
    b.add_source_token(murphy_ast::SourceToken {
        kind: murphy_ast::SourceTokenKind::LeftParen,
        range: Range { start: 3, end: 4 },
    });
    let ast = b.finish(root);
    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_eq!(cx.sorted_tokens(), ast.sorted_tokens());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p murphy-plugin-api sorted_tokens_match_the_underlying_ast
```

Expected: FAIL until `CxRaw` and `Cx` expose sorted tokens.

- [ ] **Step 3: Append CxRaw fields and accessor**

Update `crates/murphy-plugin-api/src/abi.rs` import:

```rust
use murphy_ast::{AstNode, Comment, NodeId, NodeKindTag, Range, SourceToken};
```

Append to `CxRaw`:

```rust
/// Source tokens in source order.
pub sorted_tokens: *const SourceToken,
pub sorted_tokens_len: usize,
```

Update the layout test so the appended fields come after `sink`:

```rust
assert_eq!(offset_of!(CxRaw, sorted_tokens), 136);
assert_eq!(offset_of!(CxRaw, sorted_tokens_len), 144);
assert_eq!(size_of::<CxRaw>(), 152);
```

Update `crates/murphy-plugin-api/src/cx.rs` import and accessor:

```rust
use murphy_ast::{
    AstNode, Comment, NodeId, NodeKind, OptNodeId, Range, SourceToken, collect_children,
};

pub fn sorted_tokens(&self) -> &'a [SourceToken] {
    unsafe { slice(self.raw.sorted_tokens, self.raw.sorted_tokens_len) }
}
```

Add `sorted_tokens` fields to every `CxRaw` builder in `cx.rs`, `internal.rs`,
`test_support.rs`, and `murphy-core/src/dispatch.rs`:

```rust
sorted_tokens: p.sorted_tokens.as_ptr(),
sorted_tokens_len: p.sorted_tokens.len(),
```

- [ ] **Step 4: Run focused plugin/core tests**

Run:

```bash
cargo test -p murphy-plugin-api sorted_tokens_match_the_underlying_ast
cargo test -p murphy-plugin-api cx_raw_layout_is_frozen
cargo test -p murphy-core dispatch
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-plugin-api/src/abi.rs crates/murphy-plugin-api/src/cx.rs crates/murphy-plugin-api/src/internal.rs crates/murphy-plugin-api/src/test_support.rs crates/murphy-core/src/dispatch.rs
git commit -m "feat(plugin-api): expose sorted tokens through Cx"
```

### Task 5: Workspace Verification and Issue Close

**Files:**
- Verify: all files touched by Tasks 1-4

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --all
```

Expected: exits 0.

- [ ] **Step 2: Run focused tests**

Run:

```bash
cargo test -p murphy-ast
cargo test -p murphy-translate
cargo test -p murphy-plugin-api
cargo test -p murphy-core
```

Expected: all pass.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 4: Confirm ABI version unchanged**

Run:

```bash
rg -n "MURPHY_PLUGIN_ABI_VERSION" crates/murphy-plugin-api/src/abi.rs
```

Expected: `pub const MURPHY_PLUGIN_ABI_VERSION: u32 = 1;`.

- [ ] **Step 5: Close bead and push**

Run:

```bash
bd close murphy-ji60 --reason="Implemented sorted_tokens on arena AST and native Cx"
bd dolt push
git status
git push
git status
```

Expected: branch pushed and working tree clean.
