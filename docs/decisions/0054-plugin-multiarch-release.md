# ADR 0054 — Plugin multi-arch release matrix + slot auto-selection

- Date: 2026-09-26
- Status: Accepted
- Issue: `murphy-uk7.3` (uk7: multi-arch binary release automation)
- Parent: `murphy-uk7` (Plugin ecosystem post-MVP)
- Depends on: ADR 0046 (pack format), ADR 0048 (gem distribution C2)
- Related: ADR 0038 (single-surface plugin ABI), ADR 0042 (name resolution)
- Supersedes (in part): ADR 0046 §2/§5 (slot naming reserved, auto-select deferred)

## Context

ADR 0046 reserved `lib/<arch>/` slot names but left auto-selection as the
narrow rule (first sorted entry with the host extension, else sorted-first)
and left matrix releases as future work. The template CI (`murphy-1f7`)
built and tested the host arch only. `release-gem.yml` (C2, ADR 0048)
already ships a 4-platform matrix for the `murphy` binary itself
(`x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `arm64-darwin`); packs
had no equivalent, so a pack built on one host could not be distributed to
the other three.

## Decision

### 1. Canonical slots (4)

`lib/<platform>/` uses `os-arch` order, matching ADR 0046's examples:

| Slot | Runner (template CI) | Rust target | cdylib ext |
|---|---|---|---|
| `linux-x86_64` | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | `.so` |
| `linux-aarch64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.so` |
| `darwin-x86_64` | `macos-13` | `x86_64-apple-darwin` | `.dylib` |
| `darwin-arm64` | `macos-latest` | `aarch64-apple-darwin` | `.dylib` |

`SUPPORTED_PLATFORM_SLOTS` in `murphy-core::plugin_manifest` is the source
of truth; the template matrix must cover all four (guarded by
`template_ci_matrix_covers_all_supported_slots`).

Aliases (accepted wherever a slot dir is read): gem tags
(`x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `arm64-darwin`),
tolerant spellings (`linux-arm64`, `darwin-aarch64`), and Rust triples
(`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, plus `-musl` variants).
`normalize_platform_slot` maps all of them to canonical.

Note the `arm64` vs `aarch64` split is intentional: Darwin slots follow the
Apple / RubyGems spelling (`arm64-darwin`, `darwin-arm64`), Linux slots
follow Rust / kernel (`aarch64`). The alias table accepts both spellings
on both OSes so neither side mis-stages.

### 2. Auto-selection (3 tiers, deterministic)

`resolve_pack_cdylib` collects all `*.so` / `*.dylib` under `lib/` (any
depth), sorts, then picks:

1. **Exact slot** — first sorted candidate whose `lib/<slot>/` dir (first
   component below `lib/`, alias-normalized) equals the host slot
   (`host_platform_slot`). A bare `lib/<file>.so` (no slot dir) never wins
   this tier.
2. **Platform extension** — first sorted candidate with the host extension
   (`dylib` on `darwin-*`, else `so`). This is the pre-uk7.3 narrow rule,
   preserved as the fallback.
3. **Sorted first** — so single-arch packs load anywhere.

Single-slot packs are unaffected; multi-slot packs resolve to the host
binary regardless of sort order. Broken / missing slots fall through
without shadowing (same posture as ADR 0046 §3).

### 3. Template CI matrix

`.github/workflows/ci.yml.liquid` keeps the host-only `check` job
(`cargo test` + `clippy -D warnings` + `fmt --check`) and adds:

- `build (slot)` — 4-way matrix (runners/targets per §1, mirroring
  `release-gem.yml`). Each leg `cargo build --target <triple>`, stages
  `dist/<pack>/lib/<slot>/lib<crate>.<ext>` + `murphy-plugin.toml`,
  verifies the file exists, and uploads `pack-<slot>`.
- `pack (all slots)` — downloads the four `pack-<slot>` artifacts,
  assembles `pack-all/<pack>/` (`murphy-plugin.toml` + all four `lib/`
  slots), verifies every slot is present, tars
  `<pack>.tar.gz`, and uploads `<pack>-multi-arch`.

Publishers upload that tarball wherever their static index points
(`source-url` + `source-sha256`, ADR 0053); no new service, no new Cargo
deps, no ABI bump.

## Consequences

- Pack authors get cross-built `lib/<platform>/` binaries from a stock
  template with no extra setup; consumers get the right binary with no
  config.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` stays `4`); single-arch packs
  and direct-`.so` references keep working (tiers 2–3 are the old rule).
- `git`/`curl`/`tar` remain the only fetch deps (ADR 0053); CI cross
  targets are stock `rustup` triples.

## Alternatives considered

- Rust-triple slot names (`x86_64-unknown-linux-gnu`, …): rejected —
  verbose, and ADR 0046 examples plus gem-tag practice already point at
  short names; triples stay as aliases.
- Gem-tag order (`x86_64-linux`, …) as canonical: rejected — flips ADR
  0046's documented `linux-x86_64` examples for no functional gain;
  accepted as aliases instead.
- Fat single-`.so` picked by `dlopen` fallback order only (no slot tier):
  rejected — sort order is host-independent, so the wrong arch could win
  when both extensions coincide (both Linux slots are `.so`).
- New HTTP/tar Cargo deps for fetch/pack: rejected — same rationale as
  ADR 0053 (system tools already on dev/CI hosts).
