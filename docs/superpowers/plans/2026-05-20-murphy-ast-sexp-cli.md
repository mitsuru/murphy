# Murphy AST S-expression CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `murphy ast --format sexp` so Rubyists can inspect Prism-backed Ruby AST shape in Parser/RuboCop-style S-expressions.

**Architecture:** Implement AST-to-sexp conversion in `murphy-core` as a small focused module and expose one public `ast_to_sexp(&Ast)` function. Keep CLI changes thin: parse the `ast` subcommand, read file/stdin, call core conversion, and preserve stdout/stderr/exit-code conventions.

**Tech Stack:** Rust 2024, `ruby-prism` visitor/node APIs, existing `assert_cmd` integration tests, no new dependencies.

---

## File Structure

- Create: `crates/murphy-core/src/ast_sexp.rs` for S-expression rendering, Ruby-ish escaping, and AST node mapping tests.
- Modify: `crates/murphy-core/src/lib.rs` to expose `ast_to_sexp`.
- Modify: `crates/murphy-cli/src/main.rs` to add the `ast` subcommand and stdin/file reading.
- Create: `crates/murphy-cli/tests/ast_cli.rs` for CLI behavior tests.

## Task 1: Core S-expression Renderer for NilComparison Shapes

**Files:**
- Create: `crates/murphy-core/src/ast_sexp.rs`
- Modify: `crates/murphy-core/src/lib.rs`

- [ ] **Step 1: Write failing core tests**

Create `crates/murphy-core/src/ast_sexp.rs` with these tests and public function signature:

```rust
use crate::Ast;

pub fn ast_to_sexp(_ast: &Ast<'_>) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn sexp(source: &str) -> String {
        let ast = parse(source).expect("source parses");
        ast_to_sexp(&ast)
    }

    #[test]
    fn dumps_x_equal_nil() {
        assert_eq!(sexp("x == nil"), "s(:send, s(:lvar, :x), :==, s(:nil))");
    }

    #[test]
    fn dumps_nil_equal_x() {
        assert_eq!(sexp("nil == x"), "s(:send, s(:nil), :==, s(:lvar, :x))");
    }

    #[test]
    fn dumps_x_not_equal_nil() {
        assert_eq!(sexp("x != nil"), "s(:send, s(:lvar, :x), :!=, s(:nil))");
    }
}
```

Add to `crates/murphy-core/src/lib.rs`:

```rust
mod ast_sexp;
pub use ast_sexp::ast_to_sexp;
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p murphy-core ast_sexp::tests::`

Expected: the three tests fail because `ast_to_sexp` returns an empty string.

- [ ] **Step 3: Implement minimal CallNode / nil / lvar mapping**

Replace `crates/murphy-core/src/ast_sexp.rs` with:

```rust
use crate::Ast;
use ruby_prism::Node;

pub fn ast_to_sexp(ast: &Ast<'_>) -> String {
    render_node(ast.root())
}

fn render_node(node: Node<'_>) -> String {
    if let Some(program) = node.as_program_node() {
        return program
            .statements()
            .body()
            .iter()
            .next()
            .map(render_node)
            .unwrap_or_else(|| "s(:begin)".to_string());
    }
    if let Some(call) = node.as_call_node() {
        return render_call(&call);
    }
    if node.as_nil_node().is_some() {
        return "s(:nil)".to_string();
    }
    render_unknown(node)
}

fn render_call(call: &ruby_prism::CallNode<'_>) -> String {
    let name = bytes_to_string(call.name().as_slice());
    if call.receiver().is_none() && call.arguments().is_none() && is_identifier(&name) {
        return format!("s(:lvar, {})", render_symbol(&name));
    }

    let receiver = call
        .receiver()
        .map(render_node)
        .unwrap_or_else(|| "nil".to_string());
    let mut parts = vec!["s(:send".to_string(), receiver, render_symbol(&name)];
    if let Some(arguments) = call.arguments() {
        for arg in arguments.arguments().iter() {
            parts.push(render_node(arg));
        }
    }
    format!("{})", parts.join(", "))
}

fn render_unknown(node: Node<'_>) -> String {
    format!("s(:unknown, {:?})", format!("{node:?}"))
}

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn render_symbol(value: &str) -> String {
    if is_identifier(value) || matches!(value, "==" | "!=" | "<" | ">" | "<=" | ">=") {
        format!(":{value}")
    } else {
        format!(":{}", quote_string(value))
    }
}

fn quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn sexp(source: &str) -> String {
        let ast = parse(source).expect("source parses");
        ast_to_sexp(&ast)
    }

    #[test]
    fn dumps_x_equal_nil() {
        assert_eq!(sexp("x == nil"), "s(:send, s(:lvar, :x), :==, s(:nil))");
    }

    #[test]
    fn dumps_nil_equal_x() {
        assert_eq!(sexp("nil == x"), "s(:send, s(:nil), :==, s(:lvar, :x))");
    }

    #[test]
    fn dumps_x_not_equal_nil() {
        assert_eq!(sexp("x != nil"), "s(:send, s(:lvar, :x), :!=, s(:nil))");
    }
}
```

- [ ] **Step 4: Run core renderer tests**

Run: `cargo test -p murphy-core ast_sexp::tests::`

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-core/src/ast_sexp.rs crates/murphy-core/src/lib.rs
git commit -m "feat: add AST sexp renderer"
```

## Task 2: Literal and Call Argument Coverage

**Files:**
- Modify: `crates/murphy-core/src/ast_sexp.rs`

- [ ] **Step 1: Add failing literal/call tests**

Append these tests inside `mod tests` in `crates/murphy-core/src/ast_sexp.rs`:

```rust
#[test]
fn dumps_receiver_and_argument_call() {
    assert_eq!(sexp("obj.foo(1)"), "s(:send, s(:lvar, :obj), :foo, s(:int, 1))");
}

#[test]
fn dumps_receiverless_method_call() {
    assert_eq!(sexp("foo(1)"), "s(:send, nil, :foo, s(:int, 1))");
}

#[test]
fn dumps_basic_literals() {
    assert_eq!(sexp("true"), "s(:true)");
    assert_eq!(sexp("false"), "s(:false)");
    assert_eq!(sexp("1"), "s(:int, 1)");
    assert_eq!(sexp("'x'"), "s(:str, \"x\")");
    assert_eq!(sexp(":x"), "s(:sym, :x)");
}

#[test]
fn escapes_string_literals() {
    assert_eq!(sexp("'a\\nb'"), "s(:str, \"a\\\\nb\")");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p murphy-core ast_sexp::tests::`

Expected: new tests fail because integer/string/symbol/boolean rendering is missing.

- [ ] **Step 3: Implement literal rendering**

Extend `render_node` after the nil branch with these checks:

```rust
if node.as_true_node().is_some() {
    return "s(:true)".to_string();
}
if node.as_false_node().is_some() {
    return "s(:false)".to_string();
}
if let Some(integer) = node.as_integer_node() {
    return format!("s(:int, {})", bytes_to_string(integer.location().as_slice()));
}
if let Some(string) = node.as_string_node() {
    return format!("s(:str, {})", quote_string(&bytes_to_string(string.unescaped())));
}
if let Some(symbol) = node.as_symbol_node() {
    return format!("s(:sym, {})", render_symbol(&bytes_to_string(symbol.unescaped())));
}
```

The resulting `render_node` order should be: program, call, nil, true, false, integer, string, symbol, unknown.

Use `Location::as_slice()` for integers so the output preserves the source spelling for simple integer literals. Use `StringNode::unescaped()` and `SymbolNode::unescaped()` for simple string and symbol values.

- [ ] **Step 4: Run tests**

Run: `cargo test -p murphy-core ast_sexp::tests::`

Expected: all AST sexp tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-core/src/ast_sexp.rs
git commit -m "feat: render basic AST sexp literals"
```

## Task 3: CLI `ast --format sexp`

**Files:**
- Modify: `crates/murphy-cli/src/main.rs`
- Create: `crates/murphy-cli/tests/ast_cli.rs`

- [ ] **Step 1: Write failing CLI stdin test**

Create `crates/murphy-cli/tests/ast_cli.rs`:

```rust
use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn ast_sexp_reads_stdin() {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    let assert = cmd
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("x == nil")
        .assert()
        .code(0);

    assert_eq!(
        assert.get_output().stdout,
        b"s(:send, s(:lvar, :x), :==, s(:nil))\n"
    );
    assert!(assert.get_output().stderr.is_empty());
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test -p murphy-cli --test ast_cli ast_sexp_reads_stdin`

Expected: failure with unknown subcommand `ast`.

- [ ] **Step 3: Implement `ast` subcommand in CLI**

In `crates/murphy-cli/src/main.rs`, add `ast_to_sexp` to the `use murphy_core::{...}` list.

Add this helper near `read_source`:

```rust
fn read_ast_source(path: &str) -> Result<String, AppError> {
    if path == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)
            .map_err(|e| AppError::setup(format!("cannot read stdin: {e}")))?;
        return Ok(source);
    }
    std::fs::read_to_string(Path::new(path))
        .map_err(|e| AppError::setup(format!("cannot read {path:?}: {e}")))
}
```

In `run`, before `migrate`/`lsp`/`lint` handling or directly after `lsp`, add:

```rust
if subcommand == "ast" {
    let path = match post_subcommand {
        [flag, format, path] if flag == "--format" && format == "sexp" => path,
        _ => return Err(AppError::setup("usage: murphy ast --format sexp <path|->")),
    };
    let source = read_ast_source(path)?;
    let ast = match parse(&source) {
        Ok(ast) => ast,
        Err(err) => {
            return Err(AppError {
                code: EXIT_OFFENSES,
                message: err.to_string(),
            });
        }
    };
    let sexp = ast_to_sexp(&ast);
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{sexp}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(EXIT_OK);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    return Ok(EXIT_OK);
}
```

Update usage strings in missing/unknown subcommand messages to include `murphy ast --format sexp <path|->`.

- [ ] **Step 4: Run stdin CLI test**

Run: `cargo test -p murphy-cli --test ast_cli ast_sexp_reads_stdin`

Expected: test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-cli/src/main.rs crates/murphy-cli/tests/ast_cli.rs
git commit -m "feat: add ast sexp CLI"
```

## Task 4: CLI File Input and Error Contracts

**Files:**
- Modify: `crates/murphy-cli/tests/ast_cli.rs`

- [ ] **Step 1: Add failing CLI contract tests**

Append these tests to `crates/murphy-cli/tests/ast_cli.rs`:

```rust
#[test]
fn ast_sexp_reads_file() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("sample.rb");
    std::fs::write(&path, "nil == x").expect("write sample");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&path)
        .assert()
        .code(0);

    assert_eq!(
        assert.get_output().stdout,
        b"s(:send, s(:nil), :==, s(:lvar, :x))\n"
    );
    assert!(assert.get_output().stderr.is_empty());
}

#[test]
fn ast_unknown_format_exits_2_with_empty_stdout() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("json")
        .arg("-")
        .write_stdin("x == nil")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn ast_missing_file_exits_2_with_empty_stdout() {
    let dir = tempdir().expect("create tempdir");
    let missing = dir.path().join("missing.rb");
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&missing)
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn ast_parse_error_exits_1_with_empty_stdout() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("def (\n")
        .assert()
        .code(1);

    assert!(assert.get_output().stdout.is_empty());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p murphy-cli --test ast_cli`

Expected: all ast CLI tests pass. If file input or parse error fails, fix only the CLI `ast` branch.

- [ ] **Step 3: Run regression tests for existing subcommands**

Run: `cargo test -p murphy-cli --test cli bad_usage_exits_2 lint_clean_file_exits_0_with_empty_json_array`

Expected: both tests pass, proving existing usage/error path and lint path still work.

- [ ] **Step 4: Commit**

```bash
git add crates/murphy-cli/tests/ast_cli.rs crates/murphy-cli/src/main.rs
git commit -m "test: cover ast sexp CLI contracts"
```

## Task 5: Final Quality Gates and Beads Close

**Files:**
- Beads state only, plus any final fixes discovered by tests.

- [ ] **Step 1: Run full test suite**

Run: `cargo test`

Expected: all workspace tests pass.

- [ ] **Step 2: Close issue**

Run:

```bash
bd close murphy-j1p --reason "Implemented murphy ast --format sexp for Rubyist-facing AST fixtures"
```

Expected: issue closes successfully.

- [ ] **Step 3: Commit beads export if changed**

Run: `git status --short`

If `.beads/issues.jsonl` changed, commit it:

```bash
git add .beads/issues.jsonl
git commit -m "chore: close ast sexp CLI task"
```

If no tracked file changed, do not create an empty commit.

- [ ] **Step 4: Push session work**

```bash
git pull --rebase
bd dolt push
git push
git status
```

Expected: branch is up to date with `origin/main` and working tree is clean.

## Self-Review

- Spec coverage: stdin/file input, required `--format sexp`, stdout/stderr split, setup errors, parse error, and initial NilComparison/literal node shapes are covered.
- Scope control: no JSON output, Rust fixture output, ranges, NodePattern evaluation, or cop execution is included.
- Type consistency: public core function is `ast_to_sexp(&Ast<'_>) -> String`; CLI calls the existing `parse(&source)` and then `ast_to_sexp(&ast)`.
