# Output formats (`murphy lint --format`)

`murphy lint --format <name>` selects the stdout shape. Exit codes
(`0` clean / `1` offenses / `2` setup / `3` internal) are identical for every
format. Diagnostics and `--debug` always go to stderr, so stdout stays
machine-parseable.

## Default and frozen contract

- Default is `human` (progress header + `file:line:col: C/E: Cop: message`).
- `json` is the ADR 0006 frozen contract: an array of
  `{file, cop_name, range:{start_offset,end_offset}, severity, message}`
  plus optional `autocorrect` (absent, never `null`, when no fix).
  B4 (murphy-fmw.2.4) extends real lint output with optional
  `documentation_url` / `rationale` / `fix_example` (extend-only, same
  absent-when-`None` pattern); the five core keys are byte-identical
  before/after the B7 expansion.
  `range` is omitted for filepath-only offenses. Do not change the core
  shape; add new `--format` values only.
- `progress` prints only `Inspecting N files` + summary, no offense details.

## B7 CI formats

| `--format` | Use | Empty input |
|---|---|---|
| `checkstyle` | Checkstyle XML for Jenkins/Qlty reviewdog | `<?xml ...?>\n<checkstyle></checkstyle>` |
| `sarif` | SARIF 2.1.0 for GitHub code scanning upload | `version 2.1.0`, empty `results` |
| `junit` | JUnit XML for CI test-report tabs | `<testsuite tests="0" failures="0">` |
| `github` | GitHub Actions `::warning`/`::error` annotations | empty output |
| `gnu` | GCC-style `file:line:col: warning: msg [Cop]` | empty output |
| `tap` | TAP version 13 (`not ok` per offense) | `1..0` + `# No offenses detected` |

### `checkstyle`

Grouped by file (sorted order; clean files omitted):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<checkstyle>
  <file name="dirty.rb">
    <error line="1" column="1" severity="warning" message="..." source="Lint/Debugger"/>
  </file>
</checkstyle>
```

`Warning -> warning`, `Error -> error`. Filepath-only offenses omit
`line`/`column`. XML-escapes `& < > " '`.

### `sarif`

Pretty-printed SARIF 2.1.0 (`$schema:
https://json.schemastore.org/sarif-2.1.0.json`, `runs[0].tool.driver.name:
murphy`). `rules` are deduped cop IDs; each result carries `ruleId`,
`level` (`warning`/`error`), `message.text`, and one `location` with
`artifactLocation.uri`. Located offenses add `region` with 1-based
`startLine`/`startColumn`/`endLine`/`endColumn` (byte-based columns);
filepath-only offenses omit `region`. Upload with
`github/codeql-action/upload-sarif`.

### `junit`

Single `<testsuite name="murphy" tests="N" failures="N">`, one
`<testcase classname="file" name="Cop in file:line:col">` per offense with a
`<failure message="..." type="Cop">file:line:col: msg</failure>` body.
Filepath-only bodies omit `line:col`. XML-escaped.

### `github`

One line per offense:

```text
::warning file=dirty.rb,line=1,col=1,title=Lint/Debugger::message
::error file=dirty.rb,line=1,col=1,title=Murphy/Syntax::message
```

Filepath-only annotations omit `,line=,col=`. Property escaping follows
actions/toolkit (`%`/`CR`/`LF`/`:`/`,`), message escaping covers `%`/`CR`/`LF`.

### `gnu`

One line per offense, no header:

```text
dirty.rb:1:1: warning: message [Lint/Debugger]
whitelist.rb: warning: message [Naming/InclusiveLanguage]
```

### `tap`

```text
TAP version 13
1..2
not ok 1 - dirty.rb:1:1: Lint/Debugger: message
not ok 2 - whitelist.rb: Naming/InclusiveLanguage: message
```

Each offense is a failing test. Parsers expecting `ok` for passes should
treat a `1..0` plan as success.

## B9 human-review reports

B7 formats are machine/CI-readable; B9 formats are human/PR-review
reports (rubycritic-equivalent). Same sorted per-file grouping and
filepath-only rules as B7; clean files are counted in the header but
omitted from the tables.

| `--format` | Use | Empty input |
|---|---|---|
| `markdown` | GitHub-flavoured Markdown for PR comments / `$GITHUB_STEP_SUMMARY` | header + zero-total summary, no per-file sections |
| `html` | Standalone HTML (inline CSS, no JS) for `file://` or CI artifact tabs | same skeleton + `No offenses detected.` note |

Both render the B4 `documentation_url` as a cop link when present
(real `murphy lint` output always enriches it): Markdown as
`**[Cop](url)**`, HTML as `<a href="url"><strong>Cop</strong></a>`.
Bare `Offense::new` values (no URL, e.g. unit tests) render plain bold.
Messages keep one line per offense (embedded newlines become spaces).
Output is deterministic (no timestamps).

### `markdown`

```markdown
# Murphy report

2 files inspected, 2 offenses detected (1 warning, 1 error)

## Summary

| File | Warnings | Errors | Total |
| --- | ---: | ---: | ---: |
| dirty.rb | 1 | 1 | 2 |
| Total | 1 | 1 | 2 |

## `dirty.rb` (2 offenses)

- `dirty.rb:1:1` **[Lint/Debugger](https://murphy.dev/docs/cops/Lint/Debugger)** (warning): message
```

Table cells escape `|` (and `\`); filepath-only offenses list just
`` `file` **Cop** (warning): message`` with no fabricated `line:col`.

### `html`

Standalone document: `<h1>Murphy report</h1>` header with inspected /
offense counts, a per-file `<table>` summary, then an `<h2>Offenses</h2>`
section with one `<h3><code>file</code></h3>` per offending file and a
`<ul>` of `<li><code>file:line:col</code> <strong>Cop</strong>
(<span>warning|error</span>): message</li>`. All text is HTML-escaped
(`& < > " '`). Footer notes the generator version
(`Generated by murphy X`).

## Positions

All `line`/`col` values are 1-based, byte-based columns (multibyte-aware
like `human`), read from the source file at format time. When the file
cannot be read (synthetic paths in tests), the historical fallback
`(1, offset + 1)` applies so output stays deterministic.

## Examples

```bash
murphy lint --format checkstyle app/ > checkstyle.xml
murphy lint --format sarif app/ > results.sarif
murphy lint --format junit app/ > junit.xml
murphy lint --format github app/ | tee -a "$GITHUB_STEP_SUMMARY"
murphy lint --format gnu app/
murphy lint --format tap app/ | tap-parser
murphy lint --format markdown app/ >> "$GITHUB_STEP_SUMMARY"
murphy lint --format html app/ > report.html
murphy lint --format json app/ | jq .
```
