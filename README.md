# Murphy

Murphy is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby".
It is **not** a port of RuboCop and shares no code with it; the goal is to
eliminate RuboCop's slowness with a native Rust core.

## Status

**Phase 1 walking skeleton.** Working today:

- `murphy lint <file>...` parses Ruby via prism (single parse).
- One native cop: `Murphy/NoReceiverPuts`.
- Syntax errors reported as `Murphy/Syntax` offenses.
- JSON array of offenses printed to stdout; multi-file aggregation.
- Exit codes 0/1/2/3.

Not yet production-ready. Murphy is described as a "linter/formatter", but
**only the lint path exists today** — there is **no** `murphy format`
subcommand, mruby user-cop runtime, autocorrect, config format, or parallelism
yet. Formatting/autocorrect derives from Phase 4 onward; the rest are later
phases too. See
[`docs/plans/2026-05-19-murphy-design.md`](docs/plans/2026-05-19-murphy-design.md)
for the full design and
[`docs/plans/2026-05-19-murphy-implementation-plan.md`](docs/plans/2026-05-19-murphy-implementation-plan.md)
for the roadmap.

## Quickstart

Build the workspace:

```bash
cargo build              # debug binary at ./target/debug/murphy
cargo build --release    # optimized binary at ./target/release/murphy
```

Lint a file. Given `example.rb` containing `puts "hi"`:

```console
$ ./target/debug/murphy lint example.rb
[{"file":"example.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"}]
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

You can also run via cargo without the explicit binary path:

```bash
cargo run -p murphy-cli -- lint example.rb
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
