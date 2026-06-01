# TargetRailsVersion Design

## Goal

Add `.murphy.yml` support for `AllCops.TargetRailsVersion` and expose it to native cops so Rails-version-specific cops can avoid false positives on unsupported Rails versions.

## Approach

Murphy already models `TargetRubyVersion` as a major/minor version. Reuse that representation for Rails by adding a `target_rails_version: Option<RubyVersion>` field to `MurphyConfig`; `None` means the user did not configure a Rails target. Parse `AllCops.TargetRailsVersion` with the same major/minor parser used for Ruby, including truncating teeny versions.

Expose the configured value to native cops through a tail field on `CxRaw` and safe helpers on `Cx<'_>`. Do not bump `MURPHY_PLUGIN_ABI_VERSION`; the project policy says ABI number changes require explicit approval, and current native plugins are lockstep during ABI evolution.

## Cop API

Add `Cx::target_rails_version() -> Option<RubyVersion>` and `Cx::rails_version_at_least(major, minor) -> bool`. The helper returns `true` when Rails target is unset, preserving existing behavior unless a project opts into Rails gating.

## Rails/Pick

Guard `Rails/Pick` with `rails_version_at_least(6, 0)`. This preserves current behavior for missing `TargetRailsVersion`, keeps Rails 6+ behavior unchanged, and suppresses the cop for Rails 5.x projects where `pick` does not exist.

## Testing

Add tests for config parsing, `Cx` helper behavior, dispatch propagation from `MurphyConfig` into `CxRaw`, and `Rails/Pick` suppression under `TargetRailsVersion: 5.2`.
