//! `murphy cops` subcommand — introspection for the cop catalogue.
//!
//! Subcommands:
//!
//! - `murphy cops list [--format=table|json]` — print every cop the
//!   process knows about, including ones held in the disabled registry
//!   for arena migration (§12c). The output is a public CLI contract:
//!   four columns (NAME / NAMESPACE / STATUS / SOURCE PACK) in the
//!   default table form, and a flat JSON array with stable, snake_case
//!   keys under `--format=json`. Exit code is always `0` (informational
//!   command; config errors still exit `2` through the existing
//!   `AppError::setup` path).
//!
//! Disabled-registry sources are folded in here rather than in
//! `murphy-core` so the registry itself stays focused on runtime
//! dispatch (design §5 single-surface ABI: `CopRegistry` knows only
//! about active cops). The merged view lives in the CLI because the
//! disabled list is intrinsically a host concern: it influences user
//! diagnostics (`murphy cops list`, the §12c warning) but never the
//! dispatch hot path.

use murphy_core::{CopRegistry, MurphyConfig};
use serde_json::json;
use std::io::Write;
use std::path::Path;

use super::AppError;

/// One row in the `murphy cops list` output.
#[derive(Debug, Clone)]
struct Listing {
    name: String,
    namespace: String,
    status: Status,
    source_pack: String,
}

/// Stable wire form of a cop's runtime status. `Display` produces the
/// human-readable string used in the table and the JSON `"status"`
/// field; these are part of the CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Enabled,
    DisabledDefault,
    DisabledArenaMigration,
    DisabledUserConfig,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Enabled => f.write_str("enabled"),
            Status::DisabledDefault => f.write_str("disabled: default"),
            Status::DisabledArenaMigration => f.write_str("disabled: arena migration"),
            Status::DisabledUserConfig => f.write_str("disabled: user config"),
        }
    }
}

/// Output shape selected by `--format=`. Default is `table`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
}

/// List cops with a builtin preset layer (C3).
/// Precedence: bundled defaults < file `extends:` < `--preset` < user config.
pub fn list_with_format_and_preset(format: Format, preset: Option<&str>) -> Result<u8, AppError> {
    let config = MurphyConfig::load_with_defaults_and_preset(
        Path::new("."),
        murphy_std::BUNDLED_DEFAULTS_YAML,
        preset,
    )
    .map_err(|e| AppError::setup(e.to_string()))?;

    // Build the same registry the lint flow uses (builtin pack +
    // configured `.so` cop packs), then enumerate via
    // `all_cops_with_packs` so the catalogue includes user-disabled
    // entries with a `disabled: user config` status. Using the
    // pre-filter view is what lets us surface every cop the process
    // knows about — including ones contributed by `[[plugins]]`,
    // not just `murphy-std`.
    let registry =
        CopRegistry::discover_with_config(Path::new("."), &config, super::builtin_pack())
            .map_err(|e| AppError::setup(e.to_string()))?;

    let mut listings: Vec<Listing> = Vec::new();

    for (cop, pack_name) in registry.all_cops_with_packs() {
        let name = String::from_utf8_lossy(unsafe { cop.name.as_bytes() }).into_owned();
        let namespace = namespace_of(&name).to_owned();
        let cop_default = murphy_plugin_api::tristate_from_wire(cop.default_enabled);
        let status = if config.cop_enabled_with_cop_default(&name, cop_default) {
            Status::Enabled
        } else if config.cops.rules.get(name.as_str()).and_then(|r| r.enabled) == Some(false) {
            Status::DisabledUserConfig
        } else {
            Status::DisabledDefault
        };
        listings.push(Listing {
            name,
            namespace,
            status,
            source_pack: pack_name.to_owned(),
        });
    }

    // Disabled built-in cops — listed by name only (no `PluginCopV1` yet,
    // by design). The status here always wins over `disabled: user
    // config` even if the user wrote `enabled = false`, because the
    // arena-migration state is the structural reason the cop is absent.
    for name in murphy_std::DISABLED_COPS {
        let namespace = namespace_of(name).to_owned();
        listings.push(Listing {
            name: (*name).to_owned(),
            namespace,
            status: Status::DisabledArenaMigration,
            source_pack: murphy_std::PACK_NAME.to_owned(),
        });
    }

    // Pack registration order is the lint-dispatch order; for the
    // catalogue view sort by (namespace, name) so the output is stable
    // across pack ordering. This sort is a CLI presentation contract,
    // not the dispatch order.
    listings.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut stdout = std::io::stdout().lock();
    let write_result = match format {
        Format::Table => write_table(&mut stdout, &listings),
        Format::Json => write_json(&mut stdout, &listings),
    };
    if let Err(e) = write_result {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(0);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }

    Ok(0)
}

/// Everything before the first `/` is the namespace; cop names without a
/// `/` are reported under an empty namespace (the table aligns to that).
fn namespace_of(name: &str) -> &str {
    match name.find('/') {
        Some(idx) => &name[..idx],
        None => "",
    }
}

fn write_table<W: Write>(out: &mut W, listings: &[Listing]) -> std::io::Result<()> {
    const HDR_NAME: &str = "NAME";
    const HDR_NAMESPACE: &str = "NAMESPACE";
    const HDR_STATUS: &str = "STATUS";
    const HDR_PACK: &str = "SOURCE PACK";

    let w_name = listings
        .iter()
        .map(|l| l.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());
    let w_ns = listings
        .iter()
        .map(|l| l.namespace.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAMESPACE.len());
    let w_status = listings
        .iter()
        .map(|l| l.status.to_string().len())
        .max()
        .unwrap_or(0)
        .max(HDR_STATUS.len());

    writeln!(
        out,
        "{:<name$}  {:<ns$}  {:<status$}  {}",
        HDR_NAME,
        HDR_NAMESPACE,
        HDR_STATUS,
        HDR_PACK,
        name = w_name,
        ns = w_ns,
        status = w_status
    )?;
    for l in listings {
        writeln!(
            out,
            "{:<name$}  {:<ns$}  {:<status$}  {}",
            l.name,
            l.namespace,
            l.status.to_string(),
            l.source_pack,
            name = w_name,
            ns = w_ns,
            status = w_status
        )?;
    }
    Ok(())
}

fn write_json<W: Write>(out: &mut W, listings: &[Listing]) -> std::io::Result<()> {
    let payload: Vec<_> = listings
        .iter()
        .map(|l| {
            json!({
                "name": l.name,
                "namespace": l.namespace,
                "status": l.status.to_string(),
                "source_pack": l.source_pack,
            })
        })
        .collect();
    let body = serde_json::to_string(&payload).expect("serialize cop listings");
    writeln!(out, "{body}")
}

/// Emit a warning for explicitly enabled dynamic-pack stubs. Built-in cops
/// may be disabled by default while still having a real implementation, so
/// default enablement alone is not enough to classify them as stubs. Called
/// once per lint run (after config load, before any file is parsed) so the
/// diagnostic surfaces even on a zero-file run.
pub fn warn_user_enabled_disabled(config: &MurphyConfig, registry: &murphy_core::CopRegistry) {
    // Warn only for stubs from dynamic packs (e.g. murphy-rails): disabled by
    // default AND still carrying the arena-migration stub description.
    // Fully-implemented opt-in cops (e.g. Rails/UnusedIgnoredColumns, upstream
    // Enabled:false but real) must not warn — murphy-s0vb.
    for (cop, pack_name) in registry.all_cops_with_packs() {
        let cop_default = murphy_plugin_api::tristate_from_wire(cop.default_enabled);
        let description = std::str::from_utf8(unsafe { cop.description.as_bytes() }).unwrap_or("");
        if is_disabled_dynamic_pack_stub(pack_name, cop_default, description)
            && let Ok(name) = std::str::from_utf8(unsafe { cop.name.as_bytes() })
            && config.is_explicitly_enabled(name)
        {
            eprintln!(
                "warning: cop `{name}` is a stub in the '{pack_name}' pack \
                 with a no-op check; `Enabled: true` in .murphy.yml is \
                 honoured and the cop is dispatched, but no offenses will \
                 be reported until the implementation ships"
            );
        }
    }
    // Also warn for arena-migration disabled cops (DISABLED_COPS list).
    for name in murphy_std::DISABLED_COPS {
        if config.is_explicitly_enabled(name) {
            eprintln!(
                "warning: cop `{name}` is in the disabled registry \
                 (arena migration in progress); \
                 `Enabled: true` in .murphy.yml is honoured but the cop will not run"
            );
        }
    }
}

/// The registry's `builtin` pack contains real native implementations; its
/// `default_enabled = false` value is a user-facing default, not a stub marker.
/// Dynamic packs use that value for their migration stubs, but real opt-in cops
/// (e.g. Rails/UnusedIgnoredColumns) also carry `false` — so the stub
/// description (`pending arena migration`) disambiguates (murphy-s0vb).
fn is_disabled_dynamic_pack_stub(
    pack_name: &str,
    default_enabled: Option<bool>,
    description: &str,
) -> bool {
    pack_name != "builtin"
        && default_enabled == Some(false)
        && description.contains("pending arena migration")
}

#[cfg(test)]
mod tests {
    use super::is_disabled_dynamic_pack_stub;

    const STUB_DESC: &str = "Rails cop pending arena migration (cf. murphy-au8). Stub registered.";
    const REAL_DESC: &str = "Remove a column that does not exist.";

    #[test]
    fn builtin_cops_disabled_by_default_are_not_stubs() {
        assert!(!is_disabled_dynamic_pack_stub(
            "builtin",
            Some(false),
            STUB_DESC
        ));
    }

    #[test]
    fn disabled_dynamic_pack_cops_are_stub_candidates() {
        assert!(is_disabled_dynamic_pack_stub(
            "murphy-rails",
            Some(false),
            STUB_DESC
        ));
    }

    #[test]
    fn real_opt_in_dynamic_cops_are_not_stubs() {
        // murphy-s0vb: real opt-in cops (e.g. Rails/UnusedIgnoredColumns)
        // carry default_enabled=false but must not warn.
        assert!(!is_disabled_dynamic_pack_stub(
            "murphy-rails",
            Some(false),
            REAL_DESC
        ));
    }

    #[test]
    fn enabled_or_unspecified_dynamic_cops_are_not_stub_candidates() {
        assert!(!is_disabled_dynamic_pack_stub(
            "murphy-rails",
            Some(true),
            STUB_DESC
        ));
        assert!(!is_disabled_dynamic_pack_stub(
            "murphy-rails",
            None,
            STUB_DESC
        ));
    }
}
