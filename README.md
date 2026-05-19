# Murphy

Murphy is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby".
It is **not** a port of RuboCop and shares no code with it; the goal is to
eliminate RuboCop's slowness with a native Rust core.

## Status

**Phase 3 — user mruby cops (complete).** Working today:

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
- **User cops:** drop a `.rb` file into a `cops/` directory and Murphy runs
  it **in addition to** the native cops, merged into one deterministic JSON
  offense array. `cops/` is resolved relative to the invocation working
  directory (the project root, ADR 0004) — deliberately independent of the
  lint-target path. Each user cop:
  - runs **in-process** via an embedded mruby VM, reading the live shared
    prism AST through native primitives (**no serialization round-trip**);
  - is **per-cop isolated** — its own `mrb_state`;
  - is **deadline-guarded** — a runaway / `while true` cop is abandoned by a
    wall-clock watchdog and the run continues;
  - is **exception-isolated** — a cop that `raise`s degrades to exactly one
    `severity:"error"` offense for that cop×file; all other cops/files are
    unaffected and the run completes.
- Cop SDK (`Murphy::Cop` base): an `on_call_node(node)` visitor with
  `node.name` / `node.receiver_nil?` / `node.message_loc`,
  `add_offense(range, message:, severity:)`, and a `fix` block.
- A user cop whose derived name (`Murphy/<PascalCase(file-stem)>`) collides
  with a reserved engine name (e.g. `cops/no_receiver_puts.rb` →
  `Murphy/NoReceiverPuts`, `cops/syntax.rb` → `Murphy/Syntax`) is rejected
  with exit 2, so a user cop cannot silently shadow an engine cop.
- Syntax errors reported as `Murphy/Syntax` offenses.
- JSON array of offenses printed to stdout; multi-file aggregation.
- Exit codes 0/1/2/3. A malformed or unknown-key `murphy.toml` exits 2.

Not yet production-ready. Murphy is described as a "linter/formatter", but
**only the lint path exists today**. User mruby cops run, but their `fix`
block is **captured, not applied** — there is **no** `autocorrect` field in
the offense JSON and **no** autocorrect output yet (autocorrect *application*
and the `Offense.autocorrect` contract are Phase 4). There is also **no**
`murphy format` subcommand or formatter, **no** `[cops]` config or per-cop
enable / severity-override (`murphy.toml` is discovery-only:
`[files] include`/`exclude`; cops are loaded only from `cops/`), **no**
persistent cache (in-run memoization only), **no** LSP, and **no**
node-pattern DSL. `.gitignore` is intentionally **not** consulted.
Autocorrect application derives from Phase 4 onward; `[cops]` config and
`.rubocop.yml` migration are Phase 5; the rest are later phases too. See
[`docs/plans/2026-05-19-murphy-design.md`](docs/plans/2026-05-19-murphy-design.md)
for the full design,
[`docs/plans/2026-05-19-murphy-implementation-plan.md`](docs/plans/2026-05-19-murphy-implementation-plan.md)
for the roadmap,
[`docs/plans/2026-05-19-murphy-phase-2-plan.md`](docs/plans/2026-05-19-murphy-phase-2-plan.md)
for the Phase 2 detailed plan, and
[`docs/plans/2026-05-19-murphy-phase-3-plan.md`](docs/plans/2026-05-19-murphy-phase-3-plan.md)
for the Phase 3 detailed plan.

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

### User cops (`cops/`)

Drop a `.rb` cop into a `cops/` directory at the project root and Murphy runs
it **alongside** the native cops. The file derives its cop name as
`Murphy/<PascalCase(file-stem)>` (`no_puts.rb` → `Murphy/NoPuts`).

Given `app.rb`:

```ruby
puts "hello"
print "world"
```

and `cops/no_puts.rb`:

```ruby
class NoPutsCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc,
                message: "Do not use puts",
                severity: :warning) do |fix|
      fix.replace(node.message_loc, "logger.info")
    end
  end
end
```

The user cop's offense (`Murphy/NoPuts`) is merged with the native
`Murphy/NoReceiverPuts` into one deterministic array (here the native cop also
flags the receiver-less `print`):

```console
$ ./target/debug/murphy lint app.rb
[{"file":"app.rb","cop_name":"Murphy/NoPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Do not use puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":13,"end_offset":18},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

> **Scope note (Phase 3):** the `fix` block above is **captured but not
> applied** — there is no `autocorrect` field in the offense JSON and no
> autocorrect output. Autocorrect application is Phase 4; writing a `fix`
> today only makes the cop forward-compatible.

A broken cop is **isolated**, not fatal. Add `cops/bad.rb`:

```ruby
class BadCop < Murphy::Cop
  def on_call_node(node)
    raise "intentional cop bug"
  end
end
```

It degrades to exactly one `severity:"error"` offense for that cop×file and
**the run still completes** — every other cop's offenses are still present:

```console
$ ./target/debug/murphy lint app.rb
[{"file":"app.rb","cop_name":"Murphy/Bad","range":{"start_offset":0,"end_offset":0},"severity":"error","message":"cop `Murphy/Bad` raised an exception (isolated; design §6)"},{"file":"app.rb","cop_name":"Murphy/NoPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Do not use puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":13,"end_offset":18},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

A user cop whose derived name collides with a reserved engine name is
rejected as a setup error (exit 2) so it cannot silently shadow an engine
cop. Given `cops/no_receiver_puts.rb` (→ `Murphy/NoReceiverPuts`):

```console
$ ./target/debug/murphy lint app.rb
murphy: cop file ./cops/no_receiver_puts.rb derives the reserved engine cop name "Murphy/NoReceiverPuts"; rename the file (a user cop must not shadow an engine-owned name — its offenses would be silently deduped against the engine's)
$ echo $?
2
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
