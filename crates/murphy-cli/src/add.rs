//! `murphy add <pack>` — install a cop pack from the thin registry (C1; ADR 0049).
//!
//! The registry is a static catalogue over RubyGems (C2; ADR 0048):
//! Bundler owns download + versioning + install, Murphy only overlays
//! compat metadata (ABI, minimum core version) and edits `.murphy.yml`.
//! This subcommand therefore never shells out to `bundle` / `gem` — it
//! resolves the name, checks compat, appends `plugins: - <name>` when
//! missing, and prints the Gemfile next step.

use std::path::{Path, PathBuf};

use murphy_core::pack_registry::{
    PackRegistryIndex, check_compat, config_text_has_plugin, ensure_plugin_in_config_text,
    gem_install_hint,
};

use super::AppError;

/// `murphy add` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct AddOptions {
    pub pack: String,
    pub registry: Option<PathBuf>,
    pub dry_run: bool,
}

pub fn run_add(opts: &AddOptions) -> Result<u8, AppError> {
    run_add_in(Path::new("."), opts)
}

fn run_add_in(project_root: &Path, opts: &AddOptions) -> Result<u8, AppError> {
    murphy_core::plugin_resolver::validate_plugin_name(&opts.pack)
        .map_err(|e| AppError::setup(e.to_string()))?;
    let index =
        PackRegistryIndex::load_with_override(opts.registry.as_deref()).map_err(AppError::setup)?;
    let entry = index.find(&opts.pack).ok_or_else(|| {
        AppError::setup(format!(
            "unknown pack `{}` (available: {})",
            opts.pack,
            index
                .pack_names()
                .join(", ")
                .chars()
                .take(500)
                .collect::<String>()
        ))
    })?;
    check_compat(
        entry,
        murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
        murphy_core::version(),
    )
    .map_err(|e| AppError::setup(e.to_string()))?;

    let config_path = project_root.join(".murphy.yml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    if !existing.is_empty() && config_text_has_plugin(&existing, &entry.name) {
        println!(
            "pack `{}` is already in `{}` — nothing to do",
            entry.name,
            config_path.display()
        );
        println!("{}", gem_install_hint(entry));
        return Ok(super::EXIT_OK);
    }
    // Authoritative check via the parsed config (covers `name:`/`path:`
    // detailed entries the text scan may miss).
    if !existing.trim().is_empty()
        && let Ok(cfg) = murphy_core::MurphyConfig::from_yaml_str(&existing)
        && cfg.plugins.iter().any(|p| match p {
            murphy_core::PluginConfig::Name(n) => n == &entry.name,
            murphy_core::PluginConfig::Detailed(d) => d.name == entry.name,
        })
    {
        println!(
            "pack `{}` is already in `{}` — nothing to do",
            entry.name,
            config_path.display()
        );
        println!("{}", gem_install_hint(entry));
        return Ok(super::EXIT_OK);
    }
    let updated = ensure_plugin_in_config_text(&existing, &entry.name).ok_or_else(|| {
        AppError::setup(format!(
            "pack `{}` is already in `{}`",
            entry.name,
            config_path.display()
        ))
    })?;
    // Validate the edited document parses before writing.
    murphy_core::MurphyConfig::from_yaml_str(&updated)
        .map_err(|e| AppError::setup(format!("generated config is invalid: {e}")))?;
    if opts.dry_run {
        println!(
            "dry run: would add pack `{}` to `{}`",
            entry.name,
            config_path.display()
        );
        println!("{}", gem_install_hint(entry));
        return Ok(super::EXIT_OK);
    }
    std::fs::write(&config_path, &updated)
        .map_err(|e| AppError::setup(format!("cannot write `{}`: {e}", config_path.display())))?;
    println!(
        "added pack `{}` to `{}` (plugins)",
        entry.name,
        config_path.display()
    );
    println!("{}", gem_install_hint(entry));
    Ok(super::EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(pack: &str, registry: Option<PathBuf>, dry_run: bool) -> AddOptions {
        AddOptions {
            pack: pack.to_string(),
            registry,
            dry_run,
        }
    }

    fn write_registry(dir: &Path) -> PathBuf {
        let path = dir.join("registry.toml");
        std::fs::write(
            &path,
            "[registry]\nversion = 1\n\n\
             [[packs]]\nname = \"murphy-rails\"\ngem = \"murphy-rails\"\n\
             version = \"0.1.0\"\ndescription = \"x\"\nhomepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
        )
        .expect("write registry");
        path
    }

    fn unwrap_code(r: Result<u8, super::super::AppError>) -> u8 {
        match r {
            Ok(code) => code,
            Err(e) => panic!("expected Ok, got setup error: {}", e.message),
        }
    }

    fn unwrap_err_msg(r: Result<u8, super::super::AppError>) -> String {
        match r {
            Ok(code) => panic!("expected Err, got Ok({code})"),
            Err(e) => e.message,
        }
    }

    #[test]
    fn add_creates_config_and_is_idempotent() {
        let project = tempfile::tempdir().expect("tempdir");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let reg = write_registry(reg_dir.path());
        let o = opts("murphy-rails", Some(reg), false);
        let code = unwrap_code(run_add_in(project.path(), &o));
        assert_eq!(code, super::super::EXIT_OK);
        let text = std::fs::read_to_string(project.path().join(".murphy.yml")).expect("config");
        assert!(text.contains("murphy-rails"), "config:\n{text}");
        // Second run is a no-op success.
        let code2 = unwrap_code(run_add_in(project.path(), &o));
        assert_eq!(code2, super::super::EXIT_OK);
    }

    #[test]
    fn dry_run_does_not_write() {
        let project = tempfile::tempdir().expect("tempdir");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let reg = write_registry(reg_dir.path());
        let o = opts("murphy-rails", Some(reg), true);
        unwrap_code(run_add_in(project.path(), &o));
        assert!(
            !project.path().join(".murphy.yml").exists(),
            "dry run must not write"
        );
    }

    #[test]
    fn unknown_pack_errors_with_available_list() {
        let project = tempfile::tempdir().expect("tempdir");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let reg = write_registry(reg_dir.path());
        let o = opts("murphy-nope", Some(reg), false);
        let msg = unwrap_err_msg(run_add_in(project.path(), &o));
        assert!(msg.contains("murphy-rails"), "lists packs: {msg}");
    }

    #[test]
    fn invalid_plugin_name_rejected_before_io() {
        let project = tempfile::tempdir().expect("tempdir");
        let o = opts("../evil", None, false);
        let msg = unwrap_err_msg(run_add_in(project.path(), &o));
        assert!(
            msg.contains("invalid character") || msg.contains(".."),
            "got: {msg}"
        );
    }
}
