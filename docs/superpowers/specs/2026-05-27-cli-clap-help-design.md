# CLI clap help design

## Goal

Improve Murphy's CLI help and option parsing by replacing the current ad hoc
top-level argument parser with `clap`, while preserving the existing command
behavior and exit-code contracts.

The immediate user-visible win is that `murphy --help`, `murphy lint --help`,
and nested help such as `murphy cops list --help` become first-class help
surfaces instead of setup errors. The longer-term reason is that RuboCop-shaped
options will grow over time, and clap gives those options a typed, documented,
and testable home.

## Scope

This change covers the CLI parsing and help surface only.

- Add `clap` to `murphy-cli`.
- Model the existing commands as typed clap commands:
  - `lint`
  - `migrate`
  - `ast`
  - `cops list`
  - `lsp`
- Preserve current behavior for existing flags and arguments:
  - `lint --fix` / `-a`
  - `lint --debug`
  - `lint --no-cache`
  - `lint --format human|json|progress`
  - path discovery and explicit paths, including `--` before a path beginning
    with `-`
  - `migrate <.rubocop.yml>`
  - `ast --format sexp <path|->`
  - `cops list --format=table|json`
  - `lsp`
- Add focused integration tests for help output.

This change does not add new RuboCop compatibility flags beyond the behavior
Murphy already supports. It only creates a better parser structure for adding
them later.

## Architecture

Keep `crates/murphy-cli` as a binary crate. Define a small clap command model in
`main.rs` near the current entry point:

- `Cli` owns the top-level parser.
- `Command` is a clap subcommand enum.
- Small `Args` structs represent each command's flags and positional
  arguments.

The existing lint, migration, AST, cops, and LSP execution code remains the
source of behavior. The parser should convert typed clap values into those
existing execution paths rather than rewriting the lint pipeline.

For `cops list`, either expose a typed entry point in `cops.rs` or keep a small
adapter that passes the clap-parsed format into the existing renderer. The
preferred direction is a typed entry point so nested help and parsing are owned
by clap, not a second parser.

## Error Handling

Help requests should exit 0 and write clap's help to stdout.

Invalid usage should continue to exit with setup error code 2. Since clap's
default invalid-usage exit code is also 2, the main requirement is preserving
Murphy's internal panic handling and the existing runtime errors. Exact wording
of parser diagnostics may change to clap's style, and tests should avoid
pinning fragile prose.

Runtime behavior keeps the existing Murphy contracts:

- lint clean: 0
- lint offenses: 1
- setup/config/file errors: 2
- caught internal panic: 3
- broken stdout pipe: 0 where already handled

## Help Content

Use concise command descriptions and examples through clap metadata. The first
iteration should make help discoverable and readable, not exhaustive.

The top-level help should show all subcommands. `lint --help` should show the
supported output formats, autocorrect flags, cache flag, debug flag, and path
arguments. `cops list --help` should show the table/json format choices.

## Testing

Add integration tests in `crates/murphy-cli/tests/cli.rs` and
`crates/murphy-cli/tests/cops_subcommand.rs`:

- `murphy --help` exits 0 and lists the primary subcommands.
- `murphy lint --help` exits 0 and mentions key lint flags.
- `murphy cops list --help` exits 0 and mentions `--format`.

Run the focused CLI tests after implementation:

- `cargo test -p murphy-cli --test cli`
- `cargo test -p murphy-cli --test cops_subcommand`

If parser changes touch shared behavior unexpectedly, run the full
`murphy-cli` test target set.

## Acceptance

The work is accepted when:

- The existing supported command behavior remains intact.
- Help requests at the top level, `lint`, and `cops list` exit 0.
- Relevant CLI integration tests pass.
- The implementation avoids changing `MURPHY_PLUGIN_ABI_VERSION`.
