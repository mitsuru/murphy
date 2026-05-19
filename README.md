# Murphy

Murphy is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby".
It is **not** a port of RuboCop and shares no code with it; the goal is to
eliminate RuboCop's slowness with a native Rust core.

## Status

**Phase 2 — native engine scale-out (complete).** Working today:

- `murphy lint <file>...` parses Ruby via prism (single parse) — unchanged
  Phase-1 behavior.
- `murphy lint <dir>` and `murphy lint` (zero path args → discover from the
  current directory) walk for `.rb` files.
- Discovery is configured by an optional `murphy.toml` `[files]`
  `include`/`exclude` glob lists plus a `.murphyignore` file
  (gitignore-syntax). Ambient `.gitignore` is **deliberately not** honored —
  only `.murphyignore` and `exclude` prune files.
- Linting runs **file-level parallel across all cores** (rayon); output is
  deterministic regardless of thread or argument order.
- Within a single run, files with byte-identical content are parsed and
  linted once (**in-run memoization only — no persistent cache**); output is
  identical to the non-memoized result.
- One native cop: `Murphy/NoReceiverPuts`.
- Syntax errors reported as `Murphy/Syntax` offenses.
- JSON array of offenses printed to stdout; multi-file aggregation.
- Exit codes 0/1/2/3. A malformed or unknown-key `murphy.toml` exits 2.

Not yet production-ready. Murphy is described as a "linter/formatter", but
**only the lint path exists today** — there is **no** `murphy format`
subcommand or formatter, **no** mruby user-cop runtime, **no** autocorrect,
**no** per-cop config or severity (`murphy.toml` is discovery-only:
`[files] include`/`exclude`), **no** persistent cache (in-run memoization
only), and **no** LSP. `.gitignore` is intentionally **not** consulted.
Formatting/autocorrect derives from Phase 4 onward; per-cop config and
`.rubocop.yml` migration are Phase 5; the rest are later phases too. See
[`docs/plans/2026-05-19-murphy-design.md`](docs/plans/2026-05-19-murphy-design.md)
for the full design,
[`docs/plans/2026-05-19-murphy-implementation-plan.md`](docs/plans/2026-05-19-murphy-implementation-plan.md)
for the roadmap, and
[`docs/plans/2026-05-19-murphy-phase-2-plan.md`](docs/plans/2026-05-19-murphy-phase-2-plan.md)
for the Phase 2 detailed plan.

## Quickstart

Build the workspace:

```bash
cargo build              # debug binary at ./target/debug/murphy
cargo build --release    # optimized binary at ./target/release/murphy
```

Lint a single file. Given `a.rb` containing `puts "hi"`:

```console
$ ./target/debug/murphy lint a.rb
[{"file":"a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

A clean file prints an empty array and exits 0:

```console
$ ./target/debug/murphy lint clean.rb
[]
$ echo $?
0
```

Lint a **directory** — Murphy walks it recursively for `.rb` files (here a
directory containing `a.rb`, `clean.rb`, and `sub/deep.rb`):

```console
$ ./target/debug/murphy lint /tmp/mtest
[{"file":"/tmp/mtest/a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"/tmp/mtest/sub/deep.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

Run with **no path args** to discover from the current directory:

```console
$ cd /tmp/mtest && /path/to/murphy lint
[{"file":"./a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"./sub/deep.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

Output is deterministic regardless of argument or thread order; linting runs
in parallel across all cores.

Prune files with a `murphy.toml` `[files] exclude` glob list (the schema is
discovery-only — exactly `[files]` `include`/`exclude`; no per-cop or
severity keys exist yet):

```console
$ cat murphy.toml
[files]
exclude = ["sub/**"]
$ /path/to/murphy lint
[{"file":"./a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

Or with a `.murphyignore` file (gitignore-syntax). Note `.gitignore` itself
is **not** consulted:

```console
$ cat .murphyignore
sub/
$ /path/to/murphy lint
[{"file":"./a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

A malformed or unknown-key `murphy.toml` is a setup error (exit 2):

```console
$ cat murphy.toml
[files]
bogus = 1
$ /path/to/murphy lint
murphy: invalid murphy.toml: TOML parse error at line 2, column 1
  |
2 | bogus = 1
  | ^^^^^
unknown field `bogus`, expected `include` or `exclude`
$ echo $?
2
```

You can also run via cargo without the explicit binary path:

```bash
cargo run -p murphy-cli -- lint a.rb
```

## Exit codes

| Code | Meaning                                                   |
|------|-----------------------------------------------------------|
| 0    | No offenses                                               |
| 1    | Offenses found                                            |
| 2    | Config / cop / file-setup error (bad usage, missing file) |
| 3    | Internal failure                                          |

## Build & Test

```bash
cargo build                                  # debug build
cargo build --release                        # release build
cargo test --workspace                       # full test suite
cargo test -p murphy-core <name>             # single test by name
cargo test -p murphy-cli --test cli          # one integration test target
cargo fmt --check                            # formatting gate
cargo clippy --all-targets -- -D warnings    # lint gate
```

See [`CLAUDE.md`](CLAUDE.md) for the contributor command reference.
