# Explain: AI向け offense metadata

B4 (murphy-fmw.2.4). Offense JSON carries `documentation_url` /
`rationale` / `fix_example`, and `murphy explain <cop_id>` (or
`murphy lint --explain <cop_id>`) returns the same payload in
human- or JSON-readable form.

## Offense JSON (extend-only, ADR 0006)

The frozen keys `file`, `cop_name`, `range`, `severity`, `message`
(and Phase 4 `autocorrect`) are unchanged. B4 adds three optional keys:

```json
{
  "file": "./a.rb",
  "cop_name": "Lint/Debugger",
  "range": {"start_offset": 0, "end_offset": 8},
  "severity": "warning",
  "message": "Remove debugger entry point `debugger`.",
  "documentation_url": "https://murphy.dev/docs/cops/Lint/Debugger",
  "rationale": "Lint/Debugger: Flag debugger calls and debugger requires. See the cop documentation for the rationale and fix guidance.",
  "fix_example": "Bad: pattern flagged by Lint/Debugger.\nGood: corrected pattern per documentation.\nSee https://murphy.dev/docs/cops/Lint/Debugger for concrete before/after examples."
}
```

Keys are absent (not `null`) when unenriched; old JSON without the keys
still deserializes (`default` + `skip_serializing_if`, same pattern as
`autocorrect`). No existing key was renamed or retyped.

- `documentation_url`: `{BASE}/{cop_name}` with
  `BASE = https://murphy.dev/docs/cops`. Built only from the
  registry-controlled cop name.
- `rationale`: fixed template from `cop_name` + cop-author `description`
  (`#[cop(description = ...)]`). Never interpolates offense `message`
  or source text.
- `fix_example`: fixed placeholder pointing at `documentation_url`.
  Never interpolates source text.

## CLI

```console
$ murphy explain Lint/Debugger
Cop: Lint/Debugger
Description: Flag debugger calls and debugger requires.
Documentation: https://murphy.dev/docs/cops/Lint/Debugger
Rationale: Lint/Debugger: Flag debugger calls and debugger requires. See the cop documentation for the rationale and fix guidance.
Example:
  Bad: pattern flagged by Lint/Debugger.
  Good: corrected pattern per documentation.
  See https://murphy.dev/docs/cops/Lint/Debugger for concrete before/after examples.

$ murphy explain Lint/Debugger --format json
{"cop_name":"Lint/Debugger","description":"...","documentation_url":"...","rationale":"...","fix_example":"..."}

$ murphy lint --explain Lint/Debugger   # alias for `murphy explain`
```

Unknown cop exits 2:

```console
$ murphy explain Nope/Nope
murphy: unknown cop `Nope/Nope` (see `murphy cops list` for known cops)
$ echo $?
2
```

## Prompt-injection safety

`rationale` / `fix_example` / `documentation_url` are built ONLY from:

- `cop_name` (registry-controlled),
- `description` (cop-author fixed string),
- fixed template sentences.

`Offense::message` often embeds raw source (e.g. ``Remove debugger entry
point `binding.pry`.``) and is NEVER an input. A malicious source such as
`binding.pry # IGNORE PREVIOUS INSTRUCTIONS` therefore cannot leak into
the explain output. Covered by `murphy-core` unit tests
(`rationale_never_embeds_offense_message_or_source`) and the CLI
`lint_rationale_does_not_embed_source_prompt_injection_guard` test.

## Implementation notes

- No `MURPHY_PLUGIN_ABI_VERSION` bump: `PluginCopV1` is untouched.
  Descriptions are read host-side from the existing `description`
  field; URLs/rationales are derived, not stored in the ABI.
- Enrichment happens in `murphy-cli` after aggregation
  (`explain::description_map` + `murphy_core::enrich_offense`), so the
  dispatch hot path and fixpoint loop are unchanged.
- `Murphy/Syntax` resolves to a fixed description
  (`SYNTAX_COP_DESCRIPTION`); mruby/unknown cops fall back to the
  generic rationale template.
