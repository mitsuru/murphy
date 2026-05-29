# Murphy

Murphy is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby".
It is **not** a port of RuboCop and shares no code with it; the goal is to
eliminate RuboCop's slowness with a native Rust core.

## Status

**Phase 6 — standard cop + perf gates (complete).** Working today:

- `murphy lint <file>...` parses Ruby via prism (single parse) — unchanged
  Phase-1 behavior.
- `murphy lint <dir>` and `murphy lint` (zero path args → discover from the
  current directory) walk for `.rb` files.
- Discovery is configured by an optional `.murphy.yml` `AllCops:
  Include`/`Exclude` glob lists plus a `.murphyignore` file
  (gitignore-syntax). Ambient `.gitignore` is **deliberately not** honored —
  only `.murphyignore` and `Exclude` prune files.
- Linting runs **file-level parallel across all cores** (rayon); output is
  deterministic regardless of thread or argument order.
- Within a single run, files with byte-identical content are parsed and
  linted once (**in-run memoization only — no persistent cache**); output is
  identical to the non-memoized result.
- Standard built-ins from ADR 0018 are enabled by default across `Murphy`, `Lint`,
  `Style`, and limited `Layout` namespaces.
- `.murphy.yml` also supports `AllCops.CopsPath` (user-cop path), per-cop
  `Enabled: false`, and per-cop `Severity: warning | error` override.
  Directory discovery excludes the configured cops path so cop implementation
  files are not linted as ordinary source unless named explicitly.
  The `.murphy.yml` format is intentionally RuboCop-compatible: any
  `.rubocop.yml` can serve as a `.murphy.yml` with only minor additions.
- `murphy migrate <.rubocop.yml>` normalizes a `.rubocop.yml` to `.murphy.yml`
  (adds `AllCops.CopsPath: cops`, emits plugin rename hints); the output is
  valid `.murphy.yml` that Murphy reads directly.
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
- **Native plugin packs:** configure `plugins:` in `.murphy.yml` to load
  `.so`/`.dylib` libraries as cop providers. Per-cop config is passed as JSON to
  native callbacks. Native file-level cops can narrow files with
  `Include` / `Exclude` globs (for example: `Include = ["app/**/*.rb"]`), while
  parent-directory traversal in file strings (for example `../` or `a/../b.rb`) is
  treated as out-of-scope for those matches.
- Cop SDK (`Murphy::Cop` base): an `on_call_node(node)` visitor with
  `node.name` / `node.receiver_nil?` / `node.message_loc`,
  `add_offense(range, message:, severity:)`, and a `fix` block.
- **Autocorrect:** a cop's `fix` block is now applied to source via
  `murphy lint --fix` (or `-a`). Edits are applied in descending-offset order,
  overlapping edits are conflict-logged and skipped, and Murphy re-parses and
  re-lints until a fixpoint or max-iter cutoff (`--debug` prints per-file
  iteration count, status, and conflict count). `--` separates file args from
  flags. The `Offense.autocorrect` JSON field appears when a fix is available
  and is absent (not `null`) when it is not — existing tooling that reads only
  the five frozen fields (`file`, `cop_name`, `range`, `severity`, `message`)
  is unaffected (ADR 0013).
- A user cop whose derived name (`Murphy/<PascalCase(file-stem)>`) collides
  with a reserved engine name (e.g. `cops/no_receiver_puts.rb` →
  `Murphy/NoReceiverPuts`, `cops/syntax.rb` → `Murphy/Syntax`) is rejected
  with exit 2, so a user cop cannot silently shadow an engine cop.
- Syntax errors reported as `Murphy/Syntax` offenses.
- `# murphy:disable`, `# murphy:enable`, `# murphy:todo` inline directives are
  supported in comments:
  - `# murphy:disable Cop/Name` disables a cop from that line onward
  - `# murphy:disable` disables all cops from that line onward
  - `# murphy:enable Cop/Name` re-enables one cop from that line onward
  - `# murphy:enable` re-enables all cops
  - `# murphy:todo Cop/Name` suppresses only offenses on that line
- `# murphy:todo` suppresses all cop offenses only on that line
- Syntax offenses are never suppressed by inline directives.
- JSON array of offenses printed to stdout; multi-file aggregation.
- `murphy lint --profile` emits JSON profiling data, with optional
  `--profile-format speedscope` output.
- Exit codes 0/1/2/3. A malformed `.murphy.yml` exits 2.

Not yet production-ready. Murphy is described as a "linter/formatter", but
**only the lint path exists today**. Autocorrect (`murphy lint --fix`/`-a`)
applies fix blocks to source with conflict-safe descending-offset apply, a
reparse-rerun fixpoint loop, and idempotency guarantees (ADR 0013). There is
**no** `murphy format` subcommand or formatter, **no** persistent cache (in-run
memoization only), **no** LSP, and **no** node-pattern DSL. `.gitignore` is
intentionally **not** consulted. Full RuboCop parity, formatter `murphy format`,
and sandboxing remain later. Phase 6 adds local quality/perf scripts:
`scripts/perf/phase6_hyperfine.sh` and `scripts/diff/phase6_rubocop_diff.sh`.
See
[`docs/plans/2026-05-19-murphy-design.md`](docs/plans/2026-05-19-murphy-design.md)
for the full design,
[`docs/plans/2026-05-19-murphy-implementation-plan.md`](docs/plans/2026-05-19-murphy-implementation-plan.md)
for the roadmap,
[`docs/plans/2026-05-19-murphy-phase-2-plan.md`](docs/plans/2026-05-19-murphy-phase-2-plan.md)
for the Phase 2 detailed plan,
[`docs/plans/2026-05-19-murphy-phase-3-plan.md`](docs/plans/2026-05-19-murphy-phase-3-plan.md)
for the Phase 3 detailed plan, and
[`docs/decisions/0014-phase-4-gate-review.md`](docs/decisions/0014-phase-4-gate-review.md)
for the Phase 4 gate ADR (autocorrect),
[`docs/decisions/0015-phase-5-config-schema.md`](docs/decisions/0015-phase-5-config-schema.md)
for the Phase 5 config schema,
[`docs/decisions/0016-rubocop-yml-migration-mapping.md`](docs/decisions/0016-rubocop-yml-migration-mapping.md)
for migration mapping, and
[`docs/decisions/0017-phase-5-gate-review.md`](docs/decisions/0017-phase-5-gate-review.md)
for the Phase 5 gate ADR.

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

Generate a performance summary or a Speedscope trace while linting:

```console
./target/debug/murphy lint --profile clean.rb > profile.json
./target/debug/murphy lint --profile --profile-format speedscope clean.rb > profile-trace.json
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

Prune files and configure cops with `.murphy.yml` (RuboCop-compatible format):

```console
$ cat .murphy.yml
AllCops:
  Exclude:
    - "sub/**"
  CopsPath: cops

Murphy/NoReceiverPuts:
  Severity: error
$ /path/to/murphy lint
[{"file":"./a.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"error","message":"Use a logger instead of puts"}]
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
flags the receiver-less `print`). Because `NoPuts` has a `fix` block, its
offense includes the `autocorrect` field (ADR 0013); offenses without a fix
omit it entirely:

```console
$ ./target/debug/murphy lint app.rb
[{"file":"app.rb","cop_name":"Murphy/NoPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Do not use puts","autocorrect":{"edits":[{"range":{"start_offset":0,"end_offset":4},"replacement":"logger.info"}]}},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":13,"end_offset":18},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

Apply the fix with `--fix` (or `-a`). Murphy re-parses and re-lints until a
fixpoint; after fixing, the `puts` is replaced with `logger.info` and `app.rb`
becomes `logger.info "hello"\nprint "world"\n`. The `print` offense from
`Murphy/NoReceiverPuts` has no fix and remains (exit 1):

```console
$ ./target/debug/murphy lint --fix app.rb
murphy: fixed 1 of 1 files
[{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":20,"end_offset":25},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

Add `--debug` to see per-file iteration count, convergence status, and conflict
count:

```console
$ ./target/debug/murphy lint --fix --debug app.rb
murphy: fixed 1 of 1 files
murphy: debug: app.rb iterations=1 status=Converged conflicts=0 written=true
[{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":20,"end_offset":25},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

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
[{"file":"app.rb","cop_name":"Murphy/Bad","range":{"start_offset":0,"end_offset":0},"severity":"error","message":"cop `Murphy/Bad` raised an exception (isolated; design §6)"},{"file":"app.rb","cop_name":"Murphy/NoPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Do not use puts","autocorrect":{"edits":[{"range":{"start_offset":0,"end_offset":4},"replacement":"logger.info"}]}},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":0,"end_offset":4},"severity":"warning","message":"Use a logger instead of puts"},{"file":"app.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":13,"end_offset":18},"severity":"warning","message":"Use a logger instead of puts"}]
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

A malformed `.murphy.yml` is a setup error (exit 2):

```console
$ cat .murphy.yml
AllCops: [unclosed
$ /path/to/murphy lint
murphy: invalid .murphy.yml: did not find expected node content at ...
$ echo $?
2
```

Normalize a `.rubocop.yml` to `.murphy.yml` (adds `CopsPath`, plugin rename hint):

```console
$ ./target/debug/murphy migrate .rubocop.yml > .murphy.yml
$ echo $?
0
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
cargo test -p murphy-cli --test migrate      # migration integration tests
cargo fmt --check                            # formatting gate
cargo clippy --all-targets -- -D warnings    # lint gate
```

Autocorrect commands:

```bash
murphy lint --fix <file>...         # apply fix blocks, write files back
murphy lint -a <file>...            # alias for --fix
murphy lint --fix --debug <file>... # also print per-file iteration/status/conflict line
murphy lint --fix -- <file>...      # -- separates files from flags
```

Profiling commands:

`murphy lint` can emit machine-readable profiling data instead of offense output:

```bash
murphy lint --profile <file>...
murphy lint --profile --profile-format speedscope <file>...
```

`--profile` enables profiling. The default output is `summary` format JSON.
`--profile-format` is optional and independent from `--format`.

- Summary output (`summary`): per-cop and per-file wall time totals (microseconds),
  p95 microseconds, invocation counts, and a hot-file list.
- Speedscope output (`speedscope`): a Speedscope-compatible `traceEvents` array.

Example summary payload keys:

```json
{
  "cop_wall_micros": { "Murphy/NoReceiverPuts": 1420 },
  "cop_file_micros": { "Murphy/NoReceiverPuts": { "./app.rb": 1420 } },
  "p95_micros": 1420,
  "hot_files": [{ "file": "./app.rb", "wall_micros": 1420 }],
  "invocation_count": { "Murphy/NoReceiverPuts": 1 }
}
```

You can redirect profile output to a file and load it in your browser/trace tool:

```bash
murphy lint --profile --profile-format speedscope app.rb > /tmp/profile.json
```

`--profile-format` must be used with `--profile`:

```bash
murphy lint --profile-format speedscope app.rb  # exits 2
```

See [`CLAUDE.md`](CLAUDE.md) for the contributor command reference.
