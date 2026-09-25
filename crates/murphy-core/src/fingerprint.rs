//! Lint fingerprint for the persistent result cache (A5, murphy-fmw.1.1).
//!
//! [`lint_fingerprint`] hashes everything that can change a file's lint
//! result so the on-disk [`murphy_cache::ResultCache`] key misses instead
//! of returning stale offenses:
//!
//! - the cop set (sorted dispatch cop names + pack names + plugin ABI),
//! - the resolved per-cop options (`cop_options_json`), enablement and
//!   severity,
//! - the run-wide `AllCops` context (target ruby/rails, extensions flag,
//!   indentation / style scalars baked into dispatch).
//!
//! File discovery (`files.include/exclude`) is intentionally excluded: it
//! decides *which* files are linted, never a single file's offenses.
//! Per-file `Include`/`Exclude` cop scopes are also excluded from the
//! global key — the CLI keys result entries per path (content + path +
//! this fingerprint), so identical content at differently-scoped paths
//! never shares an entry and a scope change cannot leak across files.
//!
//! Dynamic `.so` packs are covered by pack + cop names plus the murphy
//! binary version. A same-name pack rebuilt with different cop logic but
//! an unchanged version string is *not* detected — the documented remedy
//! is `murphy cache clean` (same policy as RuboCop's cache). A future
//! improvement is hashing `.so` bytes at load time; the key shape already
//! supports it (extra fingerprint bytes).

use murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION;
use sha2::{Digest, Sha256};

use crate::{CopRegistry, MurphyConfig};

/// Compute the 32-byte lint fingerprint for this run.
///
/// Deterministic for identical registry + config; any lint-affecting
/// change flips bytes. Never panics.
pub fn lint_fingerprint(registry: &CopRegistry, config: &MurphyConfig) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(b"\0");
    h.update(MURPHY_PLUGIN_ABI_VERSION.to_le_bytes());
    h.update(b"\0");

    // Cop set: dispatch names in sorted order (registration order is
    // already deterministic, but sorting makes the key robust to
    // registration-order refactors that do not change the set).
    let mut names: Vec<String> = registry
        .cops()
        .iter()
        .map(|cop| {
            // Safety: every registry entry points at immutable `PluginCopV1`
            // data valid for `&registry`'s lifetime; `RawSlice::as_bytes`
            // is `unsafe` because the pointer/length must be valid, which
            // `register_cops!` statics and loaded packs guarantee.
            let bytes = unsafe { cop.name.as_bytes() };
            String::from_utf8_lossy(bytes).into_owned()
        })
        .collect();
    names.sort();
    h.update((names.len() as u64).to_le_bytes());
    for n in &names {
        h.update((n.len() as u64).to_le_bytes());
        h.update(n.as_bytes());
        h.update(b"\0");
        // Resolved options + severity per cop: the two config channels
        // that change offenses without changing the cop set.
        let opts = config.cop_options_json(n);
        h.update((opts.len() as u64).to_le_bytes());
        h.update(&opts);
        h.update(b"\0");
        let sev = config
            .severity_override(n)
            .map(|s| format!("{s:?}"))
            .unwrap_or_default();
        h.update(sev.as_bytes());
        h.update(b"\0");
    }

    // Pack composition (builtin + dynamic pack names, registration order).
    for pack in registry.pack_names() {
        h.update((pack.len() as u64).to_le_bytes());
        h.update(pack.as_bytes());
        h.update(b"\0");
    }
    h.update(b"\0");

    // Run-wide AllCops context + target versions + extensions flag.
    let ctx = config.allcops_context();
    h.update(format!("{ctx:?}").as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", config.target_ruby_version).as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", config.target_rails_version).as_bytes());
    h.update(b"\0");
    h.update([u8::from(config.active_support_extensions_enabled)]);
    h.update(b"\0");

    // User + base-default cop rules that are *not* per-cop options above:
    // enablement flips that remove a cop from dispatch are already covered
    // by the name set, but hashing the full rule maps makes the key robust
    // to future channels (e.g. new rule fields) without revisiting this fn.
    // `BTreeMap` iterates sorted, so `Debug` is deterministic.
    h.update(format!("{:?}", config.cops.rules).as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", config.base_defaults.cop_rules).as_bytes());

    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MurphyConfig {
        MurphyConfig::from_yaml_str("AllCops:\n  TargetRubyVersion: 3.4\n")
            .expect("test config parses")
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let cfg = test_config();
        let reg = CopRegistry::native_only(&[]);
        assert_eq!(lint_fingerprint(&reg, &cfg), lint_fingerprint(&reg, &cfg));
    }

    #[test]
    fn fingerprint_changes_with_config() {
        let a = test_config();
        let mut b = a.clone();
        b.active_support_extensions_enabled = !a.active_support_extensions_enabled;
        let reg = CopRegistry::native_only(&[]);
        assert_ne!(lint_fingerprint(&reg, &a), lint_fingerprint(&reg, &b));
    }

    #[test]
    fn fingerprint_changes_with_options() {
        let a = MurphyConfig::from_yaml_str(
            "AllCops:\n  TargetRubyVersion: 3.4\nStyle/Foo:\n  Bar: 1\n",
        )
        .expect("test config parses");
        let b = MurphyConfig::from_yaml_str(
            "AllCops:\n  TargetRubyVersion: 3.4\nStyle/Foo:\n  Bar: 2\n",
        )
        .expect("test config parses");
        let reg = CopRegistry::native_only(&[]);
        assert_ne!(lint_fingerprint(&reg, &a), lint_fingerprint(&reg, &b));
    }
}
