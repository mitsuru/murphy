//! `murphy` command-line entry point.
//!
//! Post-reboot (murphy-9cr.22): dispatch is over an arena AST through the
//! single `PluginCopV1` surface (ADR 0038). `parse(source, file)` returns
//! an owned `murphy_ast::Ast`; `dispatch::run_cops` walks it once and
//! routes matched nodes to every registered cop. The CLI's lint
//! pipeline, fixpoint loop, autocorrect write-back, and inline-directive
//! handling are unchanged in observable contract; only the engine
//! underneath has been replaced.
//!
//! Sub-commands:
//!
//! - `murphy lint [flags] [paths]…` — the main lint loop.
//! - `murphy migrate <.rubocop.yml>` — one-way config migration.
//! - `murphy ast --format sexp <path|->` — dump the arena AST as
//!   S-expression text. `-` reads from stdin. The printer lives in
//!   `murphy_ast::ast_to_sexp` (re-exported via `murphy_core`).
//! - `murphy lsp` — JSON-RPC LSP server (see `lsp.rs`).
//! - `murphy install --git-hook [--tool lefthook|pre-commit|overcommit|all]` — scaffold git-hook configs (see `install.rs`).
//! - `murphy add <pack> [--registry PATH] [--dry-run]` — add a registry pack to `.murphy.yml` (see `add.rs`).
//! - `murphy init [--preset <name>] [--force] [--hook [TOOL]] [--from <.rubocop.yml>]` — scaffold `.murphy.yml` + `.murphyignore` (see `init.rs`).
//!
//! `murphy lint --profile [--profile-format summary|speedscope]` emits the
//! Phase 9 B6 profile (per-cop wall time + p95 + cop x file matrix + hot
//! files as JSON) on stdout instead of lint output. Per-cop timing comes from
//! the dispatcher's timed path
//! (`murphy_core::dispatch::run_cops_with_options_context_and_diagnostics_timed`),
//! re-introduced on the new dispatcher after the .22 perf-gate follow-up.

mod add;
mod cops;
mod explain;
mod init;
mod install;
mod lsp;
mod plugins;
mod profile;
mod since;
mod watch;

use clap::{Parser, Subcommand, ValueEnum};
use murphy_ast::content_hash;
use murphy_cache::{Cache, ResultCache};
#[cfg(feature = "mruby-user-cops")]
use murphy_core::{AstContext, run_mruby_cop_isolated};
use murphy_core::{
    Baseline, CopRegistry, FixpointStatus, MurphyConfig, Offense, SYNTAX_COP_NAME, Severity,
    aggregate_with_config, ast_to_sexp, discover_with_config, dispatch, lint_fingerprint,
    migrate_rubocop_yml_to_murphy_yml, parse, parse_with_cache, run_to_fixpoint,
};
use murphy_plugin_api::{
    PluginCopV1, PluginRegistration, Range, RawSlice, SourceToken, SourceTokenKind,
    tristate_from_wire,
};
use murphy_reporting::{OutputFormat, format_lint_output};
use profile::ProfileSummary;

/// The standard built-in cop pack (`murphy-std`), unpacked once and
/// shared by every `CopRegistry` constructed in this process.
///
/// `murphy-std` is statically linked through its `register_cops!(mode =
/// static, …)`-generated [`murphy_std::murphy_plugin_register`]. We call
/// that exactly the way a `.so` loader would, then bridge the resulting
/// `PluginRegistration` into the `&[&'static PluginCopV1]` the registry
/// wants. The cop tables behind the registration are `pub static`, so
/// they are `'static` for free.
fn builtin_pack() -> &'static [&'static PluginCopV1] {
    use std::sync::OnceLock;
    static BUILTINS: OnceLock<Vec<&'static PluginCopV1>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        let mut reg = PluginRegistration {
            abi_version: 0,
            cops_ptr: std::ptr::null(),
            cops_len: 0,
        };
        // Safety: `&mut reg` is non-null and writable for the duration
        // of the call (the only contract `murphy_plugin_register`
        // requires; see its docs). The Rust path matches the dynamic-mode
        // `extern "C"` shape so this code can move unchanged if
        // `murphy-std` is ever switched to `mode = dynamic`.
        let rc = unsafe { murphy_std::murphy_plugin_register(&mut reg) };
        assert_eq!(
            rc, 0,
            "murphy-std's static register entry must return 0 on success"
        );
        // Safety: `reg.cops_ptr` points at `murphy_std::PACK_COPS`, a
        // `#[linkme::distributed_slice]`-managed `pub static [PluginCopV1]`
        // with `'static` lifetime; the slice we hand out is a view into it.
        unsafe { std::slice::from_raw_parts(reg.cops_ptr, reg.cops_len) }
            .iter()
            .collect()
    })
}
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::thread;
use std::time::Instant;

/// Exit code: clean — zero offenses.
const EXIT_OK: u8 = 0;
/// Exit code: lint found one or more offenses.
const EXIT_OFFENSES: u8 = 1;
/// Exit code: config/cop/file-setup error.
const EXIT_SETUP_ERROR: u8 = 2;
/// Exit code: internal failure (a caught panic).
const EXIT_INTERNAL: u8 = 3;

/// Maximum autocorrect fixpoint iterations per file.
const MAX_FIX_ITERATIONS: u32 = 10;

/// Global monotonic counter for unique sibling-temp filenames.
static FIX_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Parser)]
#[command(
    name = "murphy",
    about = "Fast Ruby linting with Murphy cops",
    subcommand_required = true,
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Lint Ruby files or discover files from the current directory.
    Lint(LintArgs),
    /// Explain one cop (docs URL + rationale + fix example, AI-readable).
    Explain(ExplainArgs),
    /// Convert a .rubocop.yml file to Murphy TOML.
    Migrate(MigrateArgs),
    /// Inspect Murphy's arena AST.
    Ast(AstArgs),
    /// Inspect available cops.
    Cops(CopsArgs),
    /// Run the JSON-RPC language server.
    Lsp(LspArgs),
    /// Scaffold a new mruby cop and spec file.
    #[command(name = "new-cop")]
    NewCop(NewCopArgs),
    /// Run mruby cop spec files.
    #[command(name = "test-cop")]
    TestCop(TestCopArgs),
    /// Maintain staged plugin packs in `.murphy/plugins/`.
    Plugins(PluginsArgs),
    /// Add a cop pack from the official registry to `.murphy.yml` (C1).
    Add(AddArgs),
    /// Scaffold git-hook configs (lefthook / pre-commit / overcommit).
    Install(InstallArgs),
    /// Scaffold `.murphy.yml` + `.murphyignore` for an existing repo (C5).
    Init(InitArgs),
    /// Inspect or maintain the on-disk lint cache (A5).
    Cache(CacheArgs),
    /// Stay resident and re-lint changed files on every save (Phase 9 B1).
    Watch(WatchArgs),
}

#[derive(Debug, clap::Args)]
struct LintArgs {
    /// Apply safe autocorrections and write files back.
    #[arg(short = 'a', long = "fix", conflicts_with = "fix_all")]
    fix: bool,
    /// Apply all autocorrections, including unsafe ones, and write files back.
    #[arg(short = 'A', long = "fix-all", conflicts_with = "fix")]
    fix_all: bool,
    /// Print developer timing and pipeline diagnostics to stderr.
    #[arg(long)]
    debug: bool,
    /// Disable both on-disk caches (arena AST + lint results) for this run.
    #[arg(long)]
    no_cache: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: LintOutputFormatArg,
    /// Emit per-cop profiling JSON to stdout instead of lint output.
    /// Stdout is the profile summary (Phase 9 gate 5 shape); the exit code
    /// still reflects lint offenses. `--format` is ignored with `--profile`.
    #[arg(long)]
    profile: bool,
    /// Profile output shape: `summary` (cop wall + p95 + matrix + hot files)
    /// or `speedscope` (traceEvents). Requires `--profile`.
    #[arg(long, value_enum, value_name = "FORMAT")]
    profile_format: Option<ProfileFormatArg>,
    /// Explain one cop instead of linting (docs URL + rationale + example).
    /// Alias for `murphy explain <COP>`; kept as a lint flag per B4 spec
    /// (`--explain <cop_id>`).
    #[arg(long, value_name = "COP")]
    explain: Option<String>,
    /// Suppress offenses frozen in a baseline TOML file (`.murphy-baseline.toml`).
    /// Only new offenses — not in the baseline, or over its per-entry count —
    /// are reported. All `--format` outputs see the filtered list.
    #[arg(long, value_name = "PATH", conflicts_with = "generate_baseline")]
    baseline: Option<PathBuf>,
    /// Diff-driven lint: only lint files changed since <REF> (PR/CI fast path).
    /// Diffs the worktree (staged, unstaged, plus untracked files) against the
    /// merge-base with HEAD when available, else directly against <REF>.
    /// Intersects with explicit paths / discovery roots; a non-git directory
    /// or unknown ref fails with exit 2.
    #[arg(long, value_name = "REF")]
    since: Option<String>,
    /// Freeze current offenses into a baseline TOML file (legacy adoption:
    /// generate once, then lint with `--baseline`). The run still reports
    /// all offenses; the file records them for the next run.
    #[arg(long, value_name = "PATH")]
    generate_baseline: Option<PathBuf>,
    /// Builtin config preset (`minimal`, `recommended`, `shopify`,
    /// `rails-strict`, optionally as `murphy:<name>`). Layers below
    /// `.murphy.yml` user config; `--preset` wins over file `extends:`
    /// (explicit flag beats file), user `Enabled:`/options always win.
    /// Unknown names fail with exit 2 (C3; ADR 0050).
    #[arg(long, value_name = "PRESET")]
    preset: Option<String>,
    /// Files or directories to lint. With no paths, Murphy discovers from cwd.
    #[arg(value_name = "PATH", num_args = 0.., trailing_var_arg = true)]
    paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LintOutputFormatArg {
    Human,
    Json,
    Progress,
    Checkstyle,
    Sarif,
    Junit,
    Github,
    Gnu,
    Tap,
    Html,
    Markdown,
}

impl From<LintOutputFormatArg> for OutputFormat {
    fn from(format: LintOutputFormatArg) -> Self {
        match format {
            LintOutputFormatArg::Human => OutputFormat::Human,
            LintOutputFormatArg::Json => OutputFormat::Json,
            LintOutputFormatArg::Progress => OutputFormat::Progress,
            LintOutputFormatArg::Checkstyle => OutputFormat::Checkstyle,
            LintOutputFormatArg::Sarif => OutputFormat::Sarif,
            LintOutputFormatArg::Junit => OutputFormat::Junit,
            LintOutputFormatArg::Github => OutputFormat::Github,
            LintOutputFormatArg::Gnu => OutputFormat::Gnu,
            LintOutputFormatArg::Tap => OutputFormat::Tap,
            LintOutputFormatArg::Html => OutputFormat::Html,
            LintOutputFormatArg::Markdown => OutputFormat::Markdown,
        }
    }
}

/// Profile output shape for `murphy lint --profile`.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProfileFormatArg {
    Summary,
    Speedscope,
}

#[derive(Debug, clap::Args)]
struct ExplainArgs {
    /// Fully-qualified cop name, e.g. `Lint/Debugger`.
    #[arg(value_name = "COP")]
    cop: String,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: ExplainFormatArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExplainFormatArg {
    Human,
    Json,
}

impl From<ExplainFormatArg> for explain::Format {
    fn from(format: ExplainFormatArg) -> Self {
        match format {
            ExplainFormatArg::Human => explain::Format::Human,
            ExplainFormatArg::Json => explain::Format::Json,
        }
    }
}

#[derive(Debug, clap::Args)]
struct MigrateArgs {
    /// RuboCop YAML configuration file to migrate.
    #[arg(value_name = ".rubocop.yml")]
    path: String,
}

#[derive(Debug, clap::Args)]
struct AstArgs {
    /// AST output format.
    #[arg(long, value_enum)]
    format: AstFormatArg,
    /// Ruby source path, or '-' to read from stdin.
    #[arg(value_name = "path|-")]
    path: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AstFormatArg {
    Sexp,
}

#[derive(Debug, clap::Args)]
struct CopsArgs {
    #[command(subcommand)]
    command: CopsCommand,
}

#[derive(Debug, Subcommand)]
enum CopsCommand {
    /// List all known cops and their status.
    List(CopsListArgs),
}

#[derive(Debug, clap::Args)]
struct CopsListArgs {
    /// Output format.
    #[arg(long, value_enum, default_value = "table")]
    format: CopsFormatArg,
    /// Builtin config preset applied before status evaluation
    /// (same semantics as `murphy lint --preset`; C3).
    #[arg(long, value_name = "PRESET")]
    preset: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CopsFormatArg {
    Table,
    Json,
}

impl From<CopsFormatArg> for cops::Format {
    fn from(format: CopsFormatArg) -> Self {
        match format {
            CopsFormatArg::Table => cops::Format::Table,
            CopsFormatArg::Json => cops::Format::Json,
        }
    }
}

#[derive(Debug, clap::Args)]
struct LspArgs {
    /// Arguments forwarded to the LSP server.
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<String>,
}

#[derive(Debug, clap::Args)]
struct NewCopArgs {
    /// Cop name in Namespace/CopName format.
    #[arg(value_name = "Namespace/CopName")]
    cop: String,
}

#[derive(Debug, clap::Args)]
struct TestCopArgs {
    /// Spec files to run.
    #[arg(value_name = "spec_file", num_args = 1..)]
    spec_files: Vec<String>,
}

#[derive(Debug, clap::Args)]
struct PluginsArgs {
    #[command(subcommand)]
    command: PluginsCommand,
}

#[derive(Debug, Subcommand)]
enum PluginsCommand {
    /// Refresh staged `.so` packs in `.murphy/plugins/` from fresher builds.
    Sync(PluginsSyncArgs),
}

#[derive(Debug, clap::Args)]
struct PluginsSyncArgs {
    /// Extra dirs to search for fresher builds (repeatable).
    /// `MURPHY_PLUGIN_PATH`, the `murphy` binary's dir, and
    /// `<project>/target/{debug,release}/` are always searched.
    #[arg(long = "from", value_name = "DIR")]
    from: Vec<PathBuf>,
    /// Dry run for CI: warn on stale without copying (exit 2 when stale).
    #[arg(long)]
    check: bool,
}

#[derive(Debug, clap::Args)]
struct AddArgs {
    /// Pack name from the official registry (e.g. `murphy-rails`).
    #[arg(value_name = "PACK")]
    pack: String,
    /// Custom registry index file (overrides the bundled official index;
    /// `$MURPHY_REGISTRY_PATH` does the same without a flag).
    #[arg(long, value_name = "PATH")]
    registry: Option<PathBuf>,
    /// Show what would change without writing `.murphy.yml`.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, clap::Args)]
struct InstallArgs {
    /// Generate git-hook scaffold files.
    #[arg(long = "git-hook")]
    git_hook: bool,
    /// Which hook-framework scaffold to generate.
    #[arg(long, value_enum, default_value = "lefthook")]
    tool: InstallToolArg,
    /// Overwrite existing scaffold files.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InstallToolArg {
    Lefthook,
    PreCommit,
    Overcommit,
    All,
}

impl From<InstallToolArg> for install::HookTool {
    fn from(tool: InstallToolArg) -> Self {
        match tool {
            InstallToolArg::Lefthook => install::HookTool::Lefthook,
            InstallToolArg::PreCommit => install::HookTool::PreCommit,
            InstallToolArg::Overcommit => install::HookTool::Overcommit,
            InstallToolArg::All => install::HookTool::All,
        }
    }
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    /// Builtin config preset for the generated `.murphy.yml`
    /// (`minimal`, `recommended`, `shopify`, `rails-strict`,
    /// optionally as `murphy:<name>`). Default: `recommended`.
    #[arg(long, value_name = "PRESET", default_value = "recommended")]
    preset: String,
    /// Overwrite existing `.murphy.yml` / `.murphyignore` (and hook files).
    #[arg(long)]
    force: bool,
    /// Also scaffold a git-hook config (B8 templates). Bare `--hook`
    /// defaults to `lefthook`; pass a tool for the others.
    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "lefthook")]
    hook: Option<InstallToolArg>,
    /// Migrate this `.rubocop.yml` into `.murphy.yml` instead of writing
    /// the fresh template (chosen `extends:` is prepended; migrated cop
    /// rules always win over the preset layer).
    #[arg(long, value_name = "PATH")]
    from: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct CacheArgs {
    #[command(subcommand)]
    command: CacheCommand,
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Show cache location, entry counts and total bytes.
    Stat,
    /// Remove all cached AST and result entries.
    Clean,
}

#[derive(Debug, clap::Args)]
struct WatchArgs {
    /// Output format (same shapes as `murphy lint --format`).
    #[arg(long, value_enum, default_value = "human")]
    format: LintOutputFormatArg,
    /// Disable both on-disk caches (arena AST + lint results) for this run.
    #[arg(long)]
    no_cache: bool,
    /// Suppress offenses frozen in a baseline TOML file (reloaded every pass).
    #[arg(long, value_name = "PATH")]
    baseline: Option<PathBuf>,
    /// Poll interval in seconds (0.05–60, default 0.5).
    #[arg(long, default_value_t = watch::DEFAULT_INTERVAL_SECS)]
    interval: f64,
    /// Clear the screen before each pass.
    #[arg(long)]
    clear: bool,
    /// Lint once and exit (uses the watch pipeline; for CI/tests).
    #[arg(long)]
    once: bool,
    /// Builtin config preset (same semantics as `murphy lint --preset`; C3).
    #[arg(long, value_name = "PRESET")]
    preset: Option<String>,
    /// Files or directories to watch. With no paths, Murphy discovers from cwd.
    #[arg(value_name = "PATH", num_args = 0.., trailing_var_arg = true)]
    paths: Vec<String>,
}

#[cfg_attr(not(feature = "mruby-user-cops"), allow(dead_code))]
struct MrubyCopSource {
    name: String,
    source: String,
}

struct AppError {
    code: u8,
    message: String,
}

impl AppError {
    fn setup(message: impl Into<String>) -> AppError {
        AppError {
            code: EXIT_SETUP_ERROR,
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let outcome = catch_unwind(AssertUnwindSafe(|| run(&args)));
    let code = match outcome {
        Ok(Ok(code)) => code,
        Ok(Err(err)) => {
            let _ = writeln!(std::io::stderr(), "murphy: {}", err.message);
            err.code
        }
        Err(_panic) => {
            let _ = writeln!(
                std::io::stderr(),
                "murphy: internal failure (panic). \
                 Please file an issue."
            );
            EXIT_INTERNAL
        }
    };
    ExitCode::from(code)
}

fn read_source(path: &str) -> Result<String, AppError> {
    std::fs::read_to_string(Path::new(path))
        .map_err(|e| AppError::setup(format!("cannot read {path:?}: {e}")))
}

/// Like [`read_source`] but accepts `-` as a stdin sentinel. Used by
/// `murphy ast --format sexp <path|->`.
fn read_ast_source(path: &str) -> Result<String, AppError> {
    if path == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)
            .map_err(|e| AppError::setup(format!("cannot read stdin: {e}")))?;
        return Ok(source);
    }
    read_source(path)
}

/// Read a bounded batch in parallel with a cancellation token. Returns
/// `Err` on the first setup error (the remaining workers stop ASAP).
fn read_batch_sources(
    paths: &[String],
    worker_count: usize,
) -> Result<Vec<(String, String)>, AppError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let shared_paths = Arc::new(paths.to_vec());
    let shared_next = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let results: Arc<Mutex<Vec<(String, String)>>> =
        Arc::new(Mutex::new(Vec::with_capacity(paths.len())));
    let first_error: Arc<Mutex<Option<AppError>>> = Arc::new(Mutex::new(None));

    let workers = worker_count.max(1).min(paths.len());
    let mut handles = Vec::with_capacity(workers);

    for _ in 0..workers {
        let shared_paths = Arc::clone(&shared_paths);
        let shared_next = Arc::clone(&shared_next);
        let cancel = Arc::clone(&cancel);
        let results = Arc::clone(&results);
        let first_error = Arc::clone(&first_error);

        let handle = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                let index = shared_next.fetch_add(1, Ordering::AcqRel);
                if index >= shared_paths.len() {
                    return;
                }

                let path = &shared_paths[index];
                match read_source(path) {
                    Ok(source) => {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        results
                            .lock()
                            .expect("result sink lock poisoned")
                            .push((path.clone(), source));
                    }
                    Err(err) => {
                        let was_set = {
                            let mut lock = first_error.lock().expect("first error lock poisoned");
                            let was_set = lock.is_some();
                            if lock.is_none() {
                                *lock = Some(err);
                            }
                            was_set
                        };
                        if !was_set {
                            cancel.store(true, Ordering::Release);
                        }
                        return;
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("failed to join read worker thread");
    }

    if let Some(error) = first_error
        .lock()
        .expect("first error lock poisoned")
        .take()
    {
        return Err(error);
    }

    let mut source_paths = results
        .lock()
        .expect("result sink lock poisoned")
        .drain(..)
        .collect::<Vec<_>>();
    // Restore input order. Workers race; results' order is arbitrary.
    let index_by_path: BTreeMap<&str, usize> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    source_paths.sort_by_key(|(p, _)| index_by_path.get(p.as_str()).copied().unwrap_or(usize::MAX));
    Ok(source_paths)
}

/// Run every cop in `cops` over `source` (parsed for the given `file`),
/// applying inline-directive filtering. Syntax errors degrade to a single
/// `Murphy/Syntax` offense; cops are skipped on a parse failure.
fn lint_source(
    source: &str,
    file: &str,
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
) -> Vec<Offense> {
    let (mut offenses, comments) = match parse_with_cache(source, file, cache) {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            let scoped_cops = scoped_native_cops(cops, config, file);
            // Borrows config-owned cop-name strings; must outlive the dispatch call.
            let disabled_names: Vec<RawSlice> = config
                .disabled_cop_names()
                .map(RawSlice::borrowed)
                .collect();
            dispatch::run_cops_with_options_and_context(
                &ast,
                &scoped_cops,
                &mut sink,
                config.allcops_context(),
                &disabled_names,
                |name| config.cop_options_json(name),
            );
            let mut offenses = sink.into_offenses();
            offenses.extend(run_mruby_user_cops(source, file, mruby_cops, config));
            (offenses, comment_ranges(ast.sorted_tokens()))
        }
        Err(err) => (
            vec![Offense::new(
                file,
                SYNTAX_COP_NAME,
                err.range,
                Severity::Error,
                &err.message,
            )],
            Vec::new(),
        ),
    };
    offenses = apply_inline_directive_filter(offenses, source, &comments);
    offenses
}

#[cfg(feature = "mruby-user-cops")]
fn run_mruby_user_cops(
    source: &str,
    file: &str,
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
) -> Vec<Offense> {
    run_mruby_user_cops_profiled(source, file, mruby_cops, config).0
}

/// mruby user-cop run with per-cop wall times for `--profile`. Offenses are
/// identical to [`run_mruby_user_cops`]; the timings feed the profile matrix
/// alongside native cop timings.
#[cfg(feature = "mruby-user-cops")]
fn run_mruby_user_cops_profiled(
    source: &str,
    file: &str,
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
) -> (Vec<Offense>, Vec<(String, u64)>) {
    if mruby_cops.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let applicable_cops: Vec<_> = mruby_cops
        .iter()
        .filter(|cop| config.cop_applies_to_file(&cop.name, Path::new(file)))
        .collect();
    if applicable_cops.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let ctx = AstContext::new(source.as_bytes().to_vec());
    let mut offenses = Vec::new();
    let mut timings = Vec::with_capacity(applicable_cops.len());
    for cop in applicable_cops {
        let started = Instant::now();
        offenses.extend(run_mruby_cop_isolated(&ctx, &cop.source, &cop.name, file));
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        timings.push((cop.name.clone(), micros));
    }
    (offenses, timings)
}

#[cfg(not(feature = "mruby-user-cops"))]
fn run_mruby_user_cops(
    _source: &str,
    _file: &str,
    _mruby_cops: &[MrubyCopSource],
    _config: &MurphyConfig,
) -> Vec<Offense> {
    Vec::new()
}

#[cfg(not(feature = "mruby-user-cops"))]
fn run_mruby_user_cops_profiled(
    _source: &str,
    _file: &str,
    _mruby_cops: &[MrubyCopSource],
    _config: &MurphyConfig,
) -> (Vec<Offense>, Vec<(String, u64)>) {
    (Vec::new(), Vec::new())
}

fn scoped_native_cops<'a>(
    cops: &'a [&'a PluginCopV1],
    config: &MurphyConfig,
    file: &str,
) -> Vec<&'a PluginCopV1> {
    cops.iter()
        .copied()
        .filter(|cop| config.cop_applies_to_file(plugin_cop_name(cop), Path::new(file)))
        .collect()
}

fn plugin_cop_name(cop: &PluginCopV1) -> &str {
    std::str::from_utf8(unsafe { cop.name.as_bytes() }).unwrap_or("")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixMode {
    Safe,
    All,
}

fn cops_for_fix_mode<'a>(cops: &'a [&'a PluginCopV1], mode: FixMode) -> Vec<&'a PluginCopV1> {
    match mode {
        FixMode::All => cops.to_vec(),
        FixMode::Safe => cops
            .iter()
            .copied()
            .filter(|cop| tristate_from_wire(cop.safe_autocorrect).unwrap_or(true))
            .collect(),
    }
}

#[cfg(feature = "mruby-user-cops")]
fn load_mruby_cop_sources(paths: &[PathBuf]) -> Result<Vec<MrubyCopSource>, AppError> {
    paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path).map_err(|e| {
                AppError::setup(format!("cannot read mruby cop {}: {e}", path.display()))
            })?;
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("user_cop");
            Ok(MrubyCopSource {
                name: format!("Murphy/Mruby/{stem}"),
                source,
            })
        })
        .collect()
}

#[cfg(not(feature = "mruby-user-cops"))]
fn load_mruby_cop_sources(_paths: &[PathBuf]) -> Result<Vec<MrubyCopSource>, AppError> {
    Ok(Vec::new())
}

/// Per-file timed result used by `--debug` output. We measure parse +
/// dispatch totals only; per-cop timing requires a re-introduced timing
/// path that lands in a follow-up issue (.22 perf-gate follow-up).
struct TimedOffenses {
    offenses: Vec<Offense>,
    parse_micros: u128,
    cops_micros: u128,
}

fn lint_source_timed(
    source: &str,
    file: &str,
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
) -> TimedOffenses {
    let parse_started = Instant::now();
    let parsed = parse_with_cache(source, file, cache);
    let parse_micros = parse_started.elapsed().as_micros();
    let cops_started = Instant::now();
    let (offenses, comments) = match parsed {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            let scoped_cops = scoped_native_cops(cops, config, file);
            // Borrows config-owned cop-name strings; must outlive the dispatch call.
            let disabled_names: Vec<RawSlice> = config
                .disabled_cop_names()
                .map(RawSlice::borrowed)
                .collect();
            dispatch::run_cops_with_options_and_context(
                &ast,
                &scoped_cops,
                &mut sink,
                config.allcops_context(),
                &disabled_names,
                |name| config.cop_options_json(name),
            );
            let mut offenses = sink.into_offenses();
            offenses.extend(run_mruby_user_cops(source, file, mruby_cops, config));
            (offenses, comment_ranges(ast.sorted_tokens()))
        }
        Err(err) => (
            vec![Offense::new(
                file,
                SYNTAX_COP_NAME,
                err.range,
                Severity::Error,
                &err.message,
            )],
            Vec::new(),
        ),
    };
    let cops_micros = cops_started.elapsed().as_micros();
    TimedOffenses {
        offenses: apply_inline_directive_filter(offenses, source, &comments),
        parse_micros,
        cops_micros,
    }
}

#[derive(Debug, Clone)]
enum InlineDirectiveKind {
    Disable,
    Enable,
    Todo,
}

#[derive(Debug, Clone)]
struct InlineDirective {
    kind: InlineDirectiveKind,
    /// Cop names targeted by the directive. Empty means "all cops" (a bare
    /// directive or the `all` keyword).
    cops: Vec<String>,
}

#[derive(Debug, Clone)]
struct DirectiveState {
    disable_all: bool,
    disabled_cops: BTreeSet<String>,
    /// Cops re-enabled inside a `disable all` region via `# rubocop:enable <Cop>`.
    /// While `disable_all` is set, an offense is still reported if its cop matches
    /// an entry here, mirroring RuboCop's "disable all, then opt one back in".
    enabled_exceptions: BTreeSet<String>,
    todo_all: bool,
    todo_cops: BTreeSet<String>,
    line_start: usize,
    line_end: usize,
}

/// Byte ranges of the source's real comment tokens. A directive is only honored
/// when it sits in an actual comment — a `#` inside a string or heredoc (e.g.
/// `log("# rubocop:disable Foo")`) is NOT a comment token, so it can never
/// disable a cop. Mirrors RuboCop, which reads its directives from the comment
/// table, not raw line text.
fn comment_ranges(tokens: &[SourceToken]) -> Vec<Range> {
    tokens
        .iter()
        .filter(|t| t.kind == SourceTokenKind::Comment)
        .map(|t| t.range)
        .collect()
}

/// Parse a `# murphy:…` / `# rubocop:…` inline directive from a comment's source
/// text. Mirrors the canonical engine in
/// `murphy_plugin_api::cx::parse_comment_directive`: both prefixes are honored
/// (RuboCop-annotated codebases lint without rewrites), a `-- reason` suffix is
/// stripped, the cop list is comma-separated, and an empty list or `all` targets
/// every cop.
fn parse_inline_directive(comment_src: &str) -> Option<InlineDirective> {
    // RuboCop honors an "inner directive" — a `# rubocop:…` that follows other
    // comment text on the same comment line, e.g.
    // `# coding: utf-8 # rubocop:disable Style/Encoding`. Scan every `#`, not
    // just the first, for a `murphy:`/`rubocop:` prefix.
    let rest = comment_src.match_indices('#').find_map(|(hash_pos, _)| {
        let comment = comment_src[hash_pos + 1..].trim_start();
        comment
            .strip_prefix("murphy:")
            .or_else(|| comment.strip_prefix("rubocop:"))
            .map(str::trim_start)
    })?;
    let (keyword, tail) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(keyword, tail)| (keyword, tail));
    let kind = match keyword {
        "disable" => InlineDirectiveKind::Disable,
        "enable" => InlineDirectiveKind::Enable,
        "todo" => InlineDirectiveKind::Todo,
        _ => return None,
    };
    // The cop list ends at a `-- reason` suffix; an empty list or `all` means
    // every cop.
    let cops_text = tail.split_once("--").map_or(tail, |(cops, _)| cops).trim();
    let cops = if cops_text.is_empty() || cops_text.eq_ignore_ascii_case("all") {
        Vec::new()
    } else {
        cops_text
            .split(',')
            .map(str::trim)
            .filter(|cop| !cop.is_empty())
            .map(str::to_string)
            .collect()
    };
    Some(InlineDirective { kind, cops })
}

fn directive_states_by_line(source: &str, comments: &[Range]) -> Vec<DirectiveState> {
    let mut states = Vec::new();
    let mut disable_all = false;
    let mut disabled_cops: BTreeSet<String> = BTreeSet::new();
    let mut enabled_exceptions: BTreeSet<String> = BTreeSet::new();

    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        let mut todo_all = false;
        let mut todo_cops: BTreeSet<String> = BTreeSet::new();

        // Only a real comment token on this line can carry a directive — a `#`
        // inside a string/heredoc is not a comment and must be ignored.
        let comment = comments
            .iter()
            .find(|r| (r.start as usize) >= line_start && (r.start as usize) < line_end);
        let directive =
            comment.and_then(|r| parse_inline_directive(&source[r.start as usize..r.end as usize]));

        if let (Some(comment), Some(directive)) = (comment, directive) {
            let all = directive.cops.is_empty();
            // A directive on its own line (only whitespace before the `#`) is a
            // range directive: it persists until a matching `enable`. A trailing
            // directive (code before the `#`) scopes to its own line only. This
            // mirrors RuboCop's `disable`/`todo` comment-directive scope; the
            // line-local channel reuses the `todo_*` fields.
            let is_full_line = source.as_bytes()[line_start..comment.start as usize]
                .iter()
                .all(u8::is_ascii_whitespace);
            // RuboCop treats `todo` as an alias of `disable`; the scope (range vs
            // line-local) is decided purely by whether the directive sits on its
            // own line (range) or trails code (line-local) — identically for both
            // keywords.
            match (&directive.kind, is_full_line, all) {
                (InlineDirectiveKind::Enable, _, true) => {
                    disable_all = false;
                    disabled_cops.clear();
                    enabled_exceptions.clear();
                }
                (InlineDirectiveKind::Enable, _, false) => {
                    for cop in &directive.cops {
                        disabled_cops.remove(cop);
                        // Re-enabling inside a `disable all` region opts this cop
                        // back in even though the blanket disable stays active.
                        if disable_all {
                            enabled_exceptions.insert(cop.clone());
                        }
                    }
                }
                // A full-line `disable`/`todo` is a range directive that persists
                // until a matching `enable`.
                (InlineDirectiveKind::Disable | InlineDirectiveKind::Todo, true, true) => {
                    disable_all = true;
                    // A fresh blanket disable resets prior per-cop opt-ins.
                    enabled_exceptions.clear();
                }
                (InlineDirectiveKind::Disable | InlineDirectiveKind::Todo, true, false) => {
                    for cop in &directive.cops {
                        enabled_exceptions.remove(cop);
                    }
                    disabled_cops.extend(directive.cops);
                }
                // A trailing `disable`/`todo` (code before the `#`) scopes to its
                // own line only; the line-local channel reuses the `todo_*` fields.
                (InlineDirectiveKind::Disable | InlineDirectiveKind::Todo, false, true) => {
                    todo_all = true;
                }
                (InlineDirectiveKind::Disable | InlineDirectiveKind::Todo, false, false) => {
                    todo_cops.extend(directive.cops);
                }
            }
        }

        states.push(DirectiveState {
            disable_all,
            disabled_cops: disabled_cops.clone(),
            enabled_exceptions: enabled_exceptions.clone(),
            todo_all,
            todo_cops,
            line_start,
            line_end,
        });

        offset = line_end;
    }
    states
}

/// Cops that validate inline directives themselves. RuboCop never lets a
/// directive suppress these — otherwise a dangling `# rubocop:disable Lint`
/// would silence the very `Lint/MissingCopEnableDirective` warning reported on
/// it, since that cop lives in the disabled `Lint` department.
const DIRECTIVE_VALIDATION_COPS: [&str; 4] = [
    "Lint/MissingCopEnableDirective",
    "Lint/RedundantCopDisableDirective",
    "Lint/RedundantCopEnableDirective",
    "Lint/CopDirectiveSyntax",
];

fn is_directive_disabled(offense: &Offense, states: &[DirectiveState]) -> bool {
    if offense.cop_name == SYNTAX_COP_NAME
        || DIRECTIVE_VALIDATION_COPS.contains(&offense.cop_name.as_str())
    {
        return false;
    }
    // Filepath-only offenses carry no source location (murphy-e7bz.41.2);
    // inline `# rubocop:disable` comments cannot suppress them.
    if !offense.has_location() {
        return false;
    }
    let start = offense.range.start_offset as usize;
    for state in states {
        if start >= state.line_start && start < state.line_end {
            // A blanket `disable all` suppresses everything except cops that were
            // explicitly opted back in with a later `# rubocop:enable <Cop>`.
            let blanket_disabled =
                state.disable_all && !cop_set_matches(&state.enabled_exceptions, &offense.cop_name);
            return blanket_disabled
                || cop_set_matches(&state.disabled_cops, &offense.cop_name)
                || state.todo_all
                || cop_set_matches(&state.todo_cops, &offense.cop_name);
        }
    }
    false
}

/// True when `cops` disables `cop_name`, either by an exact cop-name entry or by
/// a RuboCop **department** entry. A slashless entry (e.g. `Lint`) is a
/// department: it matches every cop whose name is `<Department>/...` (e.g.
/// `Lint/Debugger`), mirroring `# rubocop:disable Lint`.
fn cop_set_matches(cops: &BTreeSet<String>, cop_name: &str) -> bool {
    if cops.contains(cop_name) {
        return true;
    }
    let department = cop_name.split('/').next();
    cops.iter()
        .any(|entry| !entry.contains('/') && department == Some(entry.as_str()))
}

fn apply_inline_directive_filter(
    mut offenses: Vec<Offense>,
    source: &str,
    comments: &[Range],
) -> Vec<Offense> {
    if offenses.is_empty() {
        return Vec::new();
    }
    let states = directive_states_by_line(source, comments);
    offenses.retain(|offense| !is_directive_disabled(offense, &states));
    offenses
}

/// Write `corrected` to `target` atomically using a sibling-temp + rename.
///
/// Resolves symlinks (`canonicalize`), preserves the real file's mode,
/// writes a sibling temp `.murphy-fix-<pid>-<N>.tmp` in the real file's
/// directory, sets permissions, then renames over the real path — no
/// truncation window. On any error, best-effort temp cleanup.
fn write_back_atomic(target: &Path, corrected: &str) -> Result<(), AppError> {
    let real = std::fs::canonicalize(target).map_err(|e| {
        AppError::setup(format!(
            "cannot resolve {} for --fix: {e}",
            target.display()
        ))
    })?;
    let perms = std::fs::metadata(&real)
        .map_err(|e| AppError::setup(format!("cannot stat {} for --fix: {e}", real.display())))?
        .permissions();
    let parent = real.parent().unwrap_or_else(|| Path::new("."));
    let counter = FIX_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_name = format!(".murphy-fix-{pid}-{counter}.tmp");
    let tmp_path = parent.join(&tmp_name);
    if let Err(e) = std::fs::write(&tmp_path, corrected) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot write temp file {}: {e}",
            tmp_path.display()
        )));
    }
    if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot set permissions on temp file {}: {e}",
            tmp_path.display()
        )));
    }
    if let Err(e) = std::fs::rename(&tmp_path, &real) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot rename {} → {}: {e}",
            tmp_path.display(),
            real.display()
        )));
    }
    Ok(())
}

/// Build the lint closure for `run_to_fixpoint`: parse, dispatch, aggregate,
/// then collect every `autocorrect.edits` into one flat `Vec<Edit>`.
fn lint_closure_edits<'a>(
    source: &str,
    file: &'a str,
    cops: &'a [&'a PluginCopV1],
    mruby_cops: &'a [MrubyCopSource],
    config: &'a MurphyConfig,
    cache: Option<&'a Cache>,
) -> Vec<murphy_core::Edit> {
    let offenses = lint_source(source, file, cops, mruby_cops, config, cache);
    aggregate_with_config(offenses, config)
        .into_iter()
        .filter_map(|o| o.autocorrect.map(|ac| ac.edits))
        .flatten()
        .collect()
}

struct FileDebugInfo {
    path: String,
    iterations: u32,
    status: FixpointStatus,
}

/// Memoized lint over a batch of files. Identical source content is
/// linted exactly once; results are fanned out per path with `Offense.file`
/// rewritten to each contributor path (preserves ADR 0007 determinism).
///
/// A5 persistent result cache (`murphy-fmw.1.1`): when `result_cache` is
/// `Some` — i.e. no `--no-cache`, no mruby user cops, no `--fix`
/// intermediate pass — each file first probes the on-disk result cache
/// (keyed by `content_hash` + file path + cop-pack + config
/// fingerprint). A hit skips prism parse *and* cop dispatch entirely;
/// a miss lints once per content group and populates each miss path's
/// entry. All failures (missing file, corrupt JSON, oversize payload,
/// deserialize error) degrade to a miss — the cache never changes lint
/// output, only speed.
///
/// Per-path keys (not pure content keys) so identical content at
/// different paths — which can yield different offenses under per-cop
/// `Include`/`Exclude` scopes, including pack-bundled defaults — never
/// shares an entry.
fn lint_files_memoized(
    sources: &[(String, String)],
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
    result_cache: Option<&ResultCache>,
) -> Vec<Offense> {
    if config.has_cop_path_scopes() {
        return sources
            .par_iter()
            .flat_map_iter(|(path, content)| {
                lint_source(content, path, cops, mruby_cops, config, cache)
            })
            .collect();
    }

    // Group paths by content so identical-content files share one lint.
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, content) in sources {
        groups
            .entry(content.as_str())
            .or_default()
            .push(path.as_str());
    }

    let groups_vec: Vec<(&str, Vec<&str>)> = groups.into_iter().collect();
    let mut out: Vec<Offense> = groups_vec
        .par_iter()
        .flat_map_iter(|(content, paths)| {
            // Fast path: persistent result cache (cross-run skip,
            // per-path keys). Probe every path; hits deserialize
            // directly (`file` is already correct in the entry).
            let mut all: Vec<Offense> = Vec::new();
            let mut misses: Vec<&str> = Vec::new();
            if let Some(rc) = result_cache {
                let hash = content_hash(content.as_bytes());
                for &path in paths {
                    match rc.lookup(&hash, path) {
                        Some(bytes) => match serde_json::from_slice::<Vec<Offense>>(&bytes) {
                            Ok(cached) => all.extend(cached),
                            // Corrupt JSON ⇒ miss (re-lint below).
                            Err(_) => misses.push(path),
                        },
                        None => misses.push(path),
                    }
                }
                if misses.is_empty() {
                    return all;
                }
            } else {
                misses.extend(paths.iter().copied());
            }
            // Misses (or no result cache): lint once against the first
            // miss path, fan out with `file` rewritten, and populate
            // each miss path's own entry (pre-aggregate, pre-B4).
            let representative = misses[0];
            let base = lint_source(content, representative, cops, mruby_cops, config, cache);
            if let Some(rc) = result_cache {
                let hash = content_hash(content.as_bytes());
                for &path in &misses {
                    let owned: Vec<Offense> = base
                        .iter()
                        .map(|o| {
                            let mut c = o.clone();
                            c.file = path.to_string();
                            c
                        })
                        .collect();
                    if let Ok(bytes) = serde_json::to_vec(&owned) {
                        rc.put(&hash, path, &bytes);
                    }
                }
            }
            // `base` carries the representative's `file`; rewrite per
            // miss path (representative itself needs no rewrite).
            for o in &base {
                all.push(o.clone());
            }
            for &other in &misses[1..] {
                for o in &base {
                    let mut cloned = o.clone();
                    cloned.file = other.to_string();
                    all.push(cloned);
                }
            }
            all
        })
        .collect();
    // Per-thread order is non-deterministic; aggregator restores
    // determinism by its content-based sort. No sort needed here.
    out.shrink_to_fit();
    out
}

fn lint_files_memoized_debug(
    sources: &[(String, String)],
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
) -> (Vec<Offense>, Vec<(String, u128, u128)>) {
    // Debug variant: keep per-file (parse, cops) timings. No memoization
    // across content — `--debug` is for developer visibility, the cost
    // is acceptable.
    let mut all: Vec<Offense> = Vec::new();
    let mut timings: Vec<(String, u128, u128)> = Vec::new();
    for (path, content) in sources {
        let t = lint_source_timed(content, path, cops, mruby_cops, config, cache);
        timings.push((path.clone(), t.parse_micros, t.cops_micros));
        all.extend(t.offenses);
    }
    (all, timings)
}

/// Profiled single-file lint for `--profile` (Phase 9 B6).
/// Offenses are identical to [`lint_source`] (same dispatch inputs — empty
/// parse diagnostics, same scoping, same inline-directive filter); the
/// extra fields feed the cop x file matrix.
struct ProfiledFile {
    offenses: Vec<Offense>,
    parse_micros: u128,
    cop_timings: Vec<(String, u64)>,
    mruby_timings: Vec<(String, u64)>,
}

fn lint_source_profiled(
    source: &str,
    file: &str,
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
) -> ProfiledFile {
    let parse_started = Instant::now();
    let parsed = parse_with_cache(source, file, cache);
    let parse_micros = parse_started.elapsed().as_micros();
    match parsed {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            let scoped_cops = scoped_native_cops(cops, config, file);
            // Borrows config-owned cop-name strings; must outlive the dispatch call.
            let disabled_names: Vec<RawSlice> = config
                .disabled_cop_names()
                .map(RawSlice::borrowed)
                .collect();
            let timings = dispatch::run_cops_with_options_context_and_diagnostics_timed(
                &ast,
                &scoped_cops,
                &mut sink,
                config.allcops_context(),
                &disabled_names,
                &[],
                |name| config.cop_options_json(name),
            );
            let mut offenses = sink.into_offenses();
            let (mruby_offenses, mruby_timings) =
                run_mruby_user_cops_profiled(source, file, mruby_cops, config);
            offenses.extend(mruby_offenses);
            ProfiledFile {
                offenses: apply_inline_directive_filter(
                    offenses,
                    source,
                    &comment_ranges(ast.sorted_tokens()),
                ),
                parse_micros,
                cop_timings: timings
                    .into_iter()
                    .map(|t| (t.cop_name, t.wall_micros))
                    .collect(),
                mruby_timings,
            }
        }
        Err(err) => ProfiledFile {
            offenses: vec![Offense::new(
                file,
                SYNTAX_COP_NAME,
                err.range,
                Severity::Error,
                &err.message,
            )],
            parse_micros,
            cop_timings: Vec::new(),
            mruby_timings: Vec::new(),
        },
    }
}

/// Profiled batch lint for `--profile`.
///
/// Unlike [`lint_files_memoized`] there is deliberately NO content
/// memoization: profiling attributes wall time per (cop, file), so
/// identical-content files are linted independently and each appears in the
/// matrix and hot-file list. Parallel across files like the fast path
/// (wall times are measured under parallel lint); the returned offenses are
/// still pre-aggregate flat results for the shared pipeline. The per-file
/// `(parse, cops)` totals mirror the `--debug` shape for combined runs.
fn lint_files_profiled(
    sources: &[(String, String)],
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
) -> (Vec<Offense>, ProfileSummary, Vec<(String, u128, u128)>) {
    struct FileProfile {
        path: String,
        offenses: Vec<Offense>,
        parse_micros: u128,
        native: Vec<(String, u64)>,
        mruby: Vec<(String, u64)>,
    }

    let files: Vec<FileProfile> = sources
        .par_iter()
        .map(|(path, content)| {
            let t = lint_source_profiled(content, path, cops, mruby_cops, config, cache);
            FileProfile {
                path: path.clone(),
                offenses: t.offenses,
                parse_micros: t.parse_micros,
                native: t.cop_timings,
                mruby: t.mruby_timings,
            }
        })
        .collect();

    let mut summary = ProfileSummary::default();
    let mut all: Vec<Offense> = Vec::new();
    let mut timings: Vec<(String, u128, u128)> = Vec::with_capacity(files.len());
    for f in files {
        summary.record_parse(&f.path, f.parse_micros);
        let mut cops_sum: u128 = 0;
        for (cop, micros) in &f.native {
            summary.record_native(cop, &f.path, *micros);
            cops_sum += u128::from(*micros);
        }
        for (cop, micros) in &f.mruby {
            summary.record_mruby(cop, &f.path, *micros);
            cops_sum += u128::from(*micros);
        }
        timings.push((f.path.clone(), f.parse_micros, cops_sum));
        all.extend(f.offenses);
    }
    (all, summary, timings)
}

fn to_snake_case(s: &str) -> String {
    let mut res = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                res.push('_');
            }
            res.extend(c.to_lowercase());
        } else {
            res.push(c);
        }
    }
    res.replace("__", "_")
}

fn new_cop_command(cop_arg: &str) -> Result<u8, AppError> {
    let parts: Vec<&str> = cop_arg.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(AppError::setup(
            "Invalid cop name format. Use Namespace/CopName (e.g. Foo/Bar)",
        ));
    }
    let namespace = parts[0];
    let cop_name = parts[1];

    let combined = format!("{}{}", namespace, cop_name);
    let snake_name = to_snake_case(&combined);
    let cop_file_path = format!("cops/{}.rb", snake_name);
    let spec_file_path = format!("spec/{}_spec.rb", snake_name);

    std::fs::create_dir_all("cops")
        .map_err(|e| AppError::setup(format!("failed to create cops directory: {e}")))?;
    std::fs::create_dir_all("spec")
        .map_err(|e| AppError::setup(format!("failed to create spec directory: {e}")))?;

    if Path::new(&cop_file_path).exists() {
        return Err(AppError::setup(format!(
            "File {} already exists",
            cop_file_path
        )));
    }
    if Path::new(&spec_file_path).exists() {
        return Err(AppError::setup(format!(
            "File {} already exists",
            spec_file_path
        )));
    }

    let cop_template = format!(
        "module {}\n  class {} < Murphy::Cop\n    def on_call_node(node)\n      if node.name == :puts && node.receiver_nil?\n        add_offense(node.message_loc, message: \"Use of puts is discouraged\")\n      end\n    end\n  end\nend\n",
        namespace, cop_name
    );

    let spec_template = format!(
        "describe_cop \"{}/{}\" do\n  it \"registers an offense when using puts\" do\n    expect_offense(<<~RUBY)\n      puts \"hello\"\n      ^^^^ Use of puts is discouraged\n    RUBY\n  end\nend\n",
        namespace, cop_name
    );

    std::fs::write(&cop_file_path, cop_template)
        .map_err(|e| AppError::setup(format!("failed to write {cop_file_path}: {e}")))?;
    std::fs::write(&spec_file_path, spec_template)
        .map_err(|e| AppError::setup(format!("failed to write {spec_file_path}: {e}")))?;

    println!("Generated {} and {}", cop_file_path, spec_file_path);
    Ok(EXIT_OK)
}

#[cfg(feature = "mruby-user-cops")]
fn test_cop_command(spec_files: &[String]) -> Result<u8, AppError> {
    let mut cop_sources = Vec::new();
    if Path::new("cops").is_dir() {
        let entries = std::fs::read_dir("cops")
            .map_err(|e| AppError::setup(format!("cannot read cops directory: {e}")))?;
        for entry in entries {
            let entry = entry
                .map_err(|e| AppError::setup(format!("cannot read cops directory entry: {e}")))?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rb") {
                let path_str = path.to_string_lossy().into_owned();
                let source = std::fs::read_to_string(&path).map_err(|e| {
                    AppError::setup(format!("cannot read cop file {path_str}: {e}"))
                })?;
                cop_sources.push((path_str, source));
            }
        }
    }

    let mut spec_sources = Vec::new();
    for spec_file in spec_files {
        let path = Path::new(spec_file);
        if !path.exists() {
            return Err(AppError::setup(format!(
                "Spec file not found: {}",
                spec_file
            )));
        }
        let source = std::fs::read_to_string(path)
            .map_err(|e| AppError::setup(format!("cannot read spec file {spec_file}: {e}")))?;
        spec_sources.push((spec_file.clone(), source));
    }

    let cop_refs: Vec<(&str, &str)> = cop_sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let spec_refs: Vec<(&str, &str)> = spec_sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();

    match murphy_core::run_mruby_test_specs(&cop_refs, &spec_refs) {
        Ok(()) => {
            println!("All specs passed!");
            Ok(EXIT_OK)
        }
        Err(err) => {
            eprintln!("Test execution failed: {err}");
            Err(AppError {
                code: EXIT_OFFENSES,
                message: "Some specs failed".to_string(),
            })
        }
    }
}

#[cfg(not(feature = "mruby-user-cops"))]
fn test_cop_command(_spec_files: &[String]) -> Result<u8, AppError> {
    Err(AppError::setup(
        "test-cop requires the mruby-user-cops feature (rebuild with --features mruby-user-cops)",
    ))
}

/// `murphy ast --format sexp <path|->` — parse and dump the arena AST.
///
/// A parse failure exits `EXIT_OFFENSES` (1, to mirror the lint convention
/// that syntax errors are a kind of finding); IO or bad-usage errors exit
/// `EXIT_SETUP_ERROR` (2). `BrokenPipe` on stdout collapses to `EXIT_OK`.
fn run_ast(args: &AstArgs) -> Result<u8, AppError> {
    match args.format {
        AstFormatArg::Sexp => {}
    }
    let source = read_ast_source(&args.path)?;
    let ast = parse(&source, &args.path).map_err(|err| AppError {
        code: EXIT_OFFENSES,
        message: err.message,
    })?;
    let sexp = ast_to_sexp(&ast);
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{sexp}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(EXIT_OK);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    Ok(EXIT_OK)
}

fn run(args: &[String]) -> Result<u8, AppError> {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let code = err.exit_code() as u8;
            let _ = err.print();
            return Ok(code);
        }
    };

    match cli.command {
        CliCommand::Lint(lint_args) => run_lint(&lint_args),
        CliCommand::Explain(explain_args) => {
            explain::run_explain(&explain_args.cop, explain_args.format.into())
        }
        CliCommand::Migrate(migrate_args) => run_migrate(&migrate_args),
        CliCommand::Ast(ast_args) => run_ast(&ast_args),
        CliCommand::Cops(cops_args) => run_cops(&cops_args),
        CliCommand::Lsp(lsp_args) => lsp::run(&lsp_args.args),
        CliCommand::NewCop(new_cop_args) => new_cop_command(&new_cop_args.cop),
        CliCommand::TestCop(test_cop_args) => test_cop_command(&test_cop_args.spec_files),
        CliCommand::Plugins(plugins_args) => run_plugins(&plugins_args),
        CliCommand::Add(add_args) => run_add(&add_args),
        CliCommand::Install(install_args) => run_install(&install_args),
        CliCommand::Init(init_args) => run_init(&init_args),
        CliCommand::Cache(cache_args) => run_cache(&cache_args),
        CliCommand::Watch(watch_args) => run_watch(&watch_args),
    }
}

fn run_migrate(args: &MigrateArgs) -> Result<u8, AppError> {
    let text = std::fs::read_to_string(&args.path)
        .map_err(|e| AppError::setup(format!("cannot read {:?}: {e}", args.path)))?;
    let yml =
        migrate_rubocop_yml_to_murphy_yml(&text).map_err(|e| AppError::setup(e.to_string()))?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = write!(stdout, "{yml}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(EXIT_OK);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    Ok(EXIT_OK)
}

fn run_cops(args: &CopsArgs) -> Result<u8, AppError> {
    match &args.command {
        CopsCommand::List(list_args) => {
            cops::list_with_format_and_preset(list_args.format.into(), list_args.preset.as_deref())
        }
    }
}

fn run_plugins(args: &PluginsArgs) -> Result<u8, AppError> {
    match &args.command {
        PluginsCommand::Sync(sync_args) => plugins::run_sync(&plugins::SyncOptions {
            from: sync_args.from.clone(),
            check: sync_args.check,
        }),
    }
}

fn run_add(args: &AddArgs) -> Result<u8, AppError> {
    add::run_add(&add::AddOptions {
        pack: args.pack.clone(),
        registry: args.registry.clone(),
        dry_run: args.dry_run,
    })
}

fn run_install(args: &InstallArgs) -> Result<u8, AppError> {
    install::run_install(&install::InstallOptions {
        git_hook: args.git_hook,
        tool: args.tool.into(),
        force: args.force,
    })
}

fn run_init(args: &InitArgs) -> Result<u8, AppError> {
    init::run_init(&init::InitOptions {
        preset: args.preset.clone(),
        force: args.force,
        hook: args.hook.map(|h| h.into()),
        from: args.from.clone(),
    })
}

/// `murphy cache stat| clean` (A5, murphy-fmw.1.1).
///
/// Both operate on the default cache root
/// (`$XDG_CACHE_HOME/murphy/v1`, else `$HOME/.cache/murphy/v1`).
/// `stat` prints entry counts; `clean` removes the whole tree.
/// Neither fails when the root is missing (clean is a no-op, stat
/// reports zeros) and neither consults `MURPHY_NO_CACHE`: an operator
/// asking to inspect or wipe the cache always means it.
fn run_cache(args: &CacheArgs) -> Result<u8, AppError> {
    match &args.command {
        CacheCommand::Stat => {
            let Some(root) = murphy_cache::default_cache_root() else {
                println!("cache: <no cache root (HOME unset)>");
                return Ok(EXIT_OK);
            };
            let (ast, results, bytes) = murphy_cache::cache_stats(&root);
            println!("cache root: {}", root.display());
            println!("ast entries: {ast}");
            println!("result entries: {results}");
            println!("total bytes: {bytes}");
            Ok(EXIT_OK)
        }
        CacheCommand::Clean => {
            let Some(root) = murphy_cache::default_cache_root() else {
                return Ok(EXIT_OK);
            };
            if root.exists() {
                std::fs::remove_dir_all(&root).map_err(|e| {
                    AppError::setup(format!("cannot clean {}: {e}", root.display()))
                })?;
            }
            println!("cache cleaned: {}", root.display());
            Ok(EXIT_OK)
        }
    }
}

/// Owned per-config state for one `murphy watch` process.
///
/// All fields are owned (no borrows across reloads): `cops_vec` is rebuilt
/// from `registry` on every pass, so replacing the whole session on
/// `.murphy.yml` change is safe. `result_cache` embeds the
/// `lint_fingerprint(registry, config)`, so a reloaded session misses old
/// keys by construction and never returns stale offenses (ADR 0047).
struct WatchSession {
    config: MurphyConfig,
    registry: CopRegistry,
    mruby_cops: Vec<MrubyCopSource>,
    cache: Option<Cache>,
    result_cache: Option<ResultCache>,
}

/// Load config + registry + caches for `murphy watch` (mirrors the
/// `run_lint` setup minus `--fix`/`--profile`/`--since`: watch never
/// fixes, never profiles, and computes its own diff by polling).
fn load_watch_session(no_cache: bool, preset: Option<&str>) -> Result<WatchSession, AppError> {
    let mut config = MurphyConfig::load_with_defaults_and_preset(
        Path::new("."),
        murphy_std::BUNDLED_DEFAULTS_YAML,
        preset,
    )
    .map_err(|e| AppError::setup(e.to_string()))?;
    for stale in murphy_core::plugin_sync::check_project_with_config(Path::new("."), &config, &[]) {
        eprintln!("{}", stale.warning());
    }
    let registry = CopRegistry::discover_with_config(Path::new("."), &config, builtin_pack())
        .map_err(|e| AppError::setup(e.to_string()))?;
    config.apply_pack_default_layers(&registry.pack_default_configs());
    cops::warn_user_enabled_disabled(&config, &registry);
    #[cfg(feature = "mruby-user-cops")]
    let mruby_cop_sources = load_mruby_cop_sources(registry.mruby_cop_paths())?;
    #[cfg(not(feature = "mruby-user-cops"))]
    let mruby_cop_sources = load_mruby_cop_sources(&[])?;
    let mruby_cops: Vec<MrubyCopSource> = mruby_cop_sources;
    // Same gating as `run_lint` (minus `--fix`, which watch never does):
    // mruby user cops live outside the fingerprint, so the result cache
    // stays off and the run falls back to the AST cache.
    let cache: Option<Cache> = if no_cache {
        None
    } else {
        Cache::open(murphy_translate::LAYER_VERSION)
    };
    let result_cache: Option<ResultCache> = if no_cache || !mruby_cops.is_empty() {
        None
    } else {
        let extra = lint_fingerprint(&registry, &config);
        ResultCache::open(&extra, murphy_translate::LAYER_VERSION)
    };
    Ok(WatchSession {
        config,
        registry,
        mruby_cops,
        cache,
        result_cache,
    })
}

/// Resolve the watch file list (mirrors the `run_lint` path
/// classification + discovery, without `--since`: the watcher computes
/// its own diff by polling, so no git restriction applies here).
fn discover_watch_paths(
    path_args: &[&str],
    config: &MurphyConfig,
    registry: &CopRegistry,
    preset: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let mut explicit_files: Vec<String> = Vec::new();
    let mut discover_roots: Vec<PathBuf> = Vec::new();
    if path_args.is_empty() {
        discover_roots.push(PathBuf::from("."));
    } else {
        for arg in path_args {
            let p = Path::new(arg);
            if p.is_dir() {
                discover_roots.push(p.to_path_buf());
            } else {
                explicit_files.push((*arg).to_string());
            }
        }
    }
    let mut all_paths: BTreeSet<String> = explicit_files.iter().cloned().collect();
    for root in &discover_roots {
        let discovered = if root == Path::new(".") {
            discover_with_config(root, config).map_err(|e| AppError::setup(e.to_string()))?
        } else {
            let mut local_config = MurphyConfig::load_with_defaults_and_preset(
                root,
                murphy_std::BUNDLED_DEFAULTS_YAML,
                preset,
            )
            .map_err(|e| AppError::setup(e.to_string()))?;
            local_config.apply_pack_default_layers(&registry.pack_default_configs());
            discover_with_config(root, &local_config).map_err(|e| AppError::setup(e.to_string()))?
        };
        for p in discovered {
            all_paths.insert(p.to_string_lossy().into_owned());
        }
    }
    Ok(all_paths.into_iter().collect())
}

/// Lint one watch pass (full initial pass or incremental changed-file
/// subset) and print it in the requested `--format` shape.
///
/// Returns `(exit_code, offense_count)`. Aggregation, B4 enrichment,
/// baseline filtering, and formatting are the shared `run_lint` pipeline
/// — incremental passes only narrow the *file subset*, never the
/// per-file semantics, so the ADR 0006 JSON shape is unchanged.
fn lint_and_print_watch_pass(
    sources: &[(String, String)],
    session: &WatchSession,
    format: LintOutputFormatArg,
    baseline: Option<&PathBuf>,
    clear: bool,
) -> Result<(u8, usize), AppError> {
    // `registry.cops()` allocates a fresh `Vec<&PluginCopV1>` bounded by
    // `&session.registry`; hold it for this pass only (a config reload
    // replaces the whole session between passes).
    let cops_vec = session.registry.cops();
    let cops: &[&PluginCopV1] = &cops_vec;
    let flat = lint_files_memoized(
        sources,
        cops,
        &session.mruby_cops,
        &session.config,
        session.cache.as_ref(),
        session.result_cache.as_ref(),
    );
    let mut offenses = aggregate_with_config(flat, &session.config);
    {
        let desc_map = explain::description_map(&session.registry);
        for offense in &mut offenses {
            let desc =
                explain::lookup_description(&desc_map, &offense.cop_name).unwrap_or_default();
            murphy_core::enrich_offense(offense, &desc);
        }
    }
    if let Some(path) = baseline {
        let loaded = Baseline::load(path).map_err(|e| {
            AppError::setup(format!("cannot load baseline {}: {e}", path.display()))
        })?;
        offenses = loaded.filter_offenses(offenses);
    }
    let exit = if offenses.is_empty() {
        EXIT_OK
    } else {
        EXIT_OFFENSES
    };
    let count = offenses.len();
    let file_paths: Vec<String> = sources.iter().map(|(p, _)| p.clone()).collect();
    let formatted = format_lint_output(&offenses, &file_paths, OutputFormat::from(format))
        .map_err(AppError::setup)?;
    let mut stdout = std::io::stdout().lock();
    if clear {
        // ANSI clear screen + home cursor (no new dependency; ignored
        // when stdout is piped).
        if let Err(e) = write!(stdout, "\x1b[2J\x1b[H") {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok((exit, count));
            }
            return Err(AppError::setup(format!("failed to write stdout: {e}")));
        }
    }
    if let Err(e) = writeln!(stdout, "{formatted}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok((exit, count));
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    if let Err(e) = std::io::Write::flush(&mut stdout) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok((exit, count));
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    Ok((exit, count))
}

/// `murphy watch` (Phase 9 B1, murphy-fmw.2.1).
///
/// Resident polling loop on top of the A5 result cache: the initial pass
/// lints every discovered file (warming `results/*.json`), then each tick
/// re-discovers, diffs `mtime` snapshots, and re-lints only added +
/// modified files. `.murphy.yml` (cwd) and the `--baseline` file are
/// tracked too — a change there triggers a full re-lint (with a config
/// reload for `.murphy.yml`). Transient per-tick failures (a file deleted
/// mid-pass, a discovery hiccup) are reported on stderr without killing
/// the resident loop; only baseline-load failures exit (they would fail
/// every future pass identically, so failing loud matches `murphy lint`).
fn run_watch(args: &WatchArgs) -> Result<u8, AppError> {
    let interval = watch::validate_interval(args.interval).map_err(AppError::setup)?;
    let path_args: Vec<&str> = args.paths.iter().map(String::as_str).collect();
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let mut session = load_watch_session(args.no_cache, args.preset.as_deref())?;
    let mut prev_files = discover_watch_paths(
        &path_args,
        &session.config,
        &session.registry,
        args.preset.as_deref(),
    )?;
    let mut prev_snap = watch::snapshot_files(&prev_files);
    let mut config_sig = watch::file_sig(".murphy.yml");
    let mut baseline_sig = args
        .baseline
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .as_deref()
        .and_then(watch::file_sig);

    // ── initial full pass (also warms the A5 result cache) ──────────────
    let initial_sources = read_batch_sources(&prev_files, worker_count)?;
    let (initial_exit, initial_count) = lint_and_print_watch_pass(
        &initial_sources,
        &session,
        args.format,
        args.baseline.as_ref(),
        args.clear,
    )?;
    eprintln!(
        "murphy watch: initial pass: {} files, {initial_count} offenses",
        prev_files.len()
    );
    if args.once {
        return Ok(initial_exit);
    }
    eprintln!(
        "murphy watch: watching {} files every {:.2}s (Ctrl-C to stop)",
        prev_files.len(),
        args.interval
    );

    // ── resident poll loop ──────────────────────────────────────────────
    loop {
        std::thread::sleep(interval);

        // Config / baseline change → full re-lint (reload first for config:
        // the new fingerprint misses old result-cache keys by construction).
        let cur_config_sig = watch::file_sig(".murphy.yml");
        let cur_baseline_sig = args
            .baseline
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .as_deref()
            .and_then(watch::file_sig);
        if cur_config_sig != config_sig {
            eprintln!(
                "murphy watch: config changed (.murphy.yml) — reloading and re-linting all files"
            );
            session = load_watch_session(args.no_cache, args.preset.as_deref())?;
            config_sig = cur_config_sig;
            baseline_sig = cur_baseline_sig;
            match discover_watch_paths(
                &path_args,
                &session.config,
                &session.registry,
                args.preset.as_deref(),
            ) {
                Ok(files) => {
                    prev_files = files;
                    prev_snap = watch::snapshot_files(&prev_files);
                }
                Err(e) => {
                    eprintln!("murphy watch: discovery failed, retrying: {}", e.message);
                    continue;
                }
            }
            match read_batch_sources(&prev_files, worker_count) {
                Ok(sources) => {
                    match lint_and_print_watch_pass(
                        &sources,
                        &session,
                        args.format,
                        args.baseline.as_ref(),
                        args.clear,
                    ) {
                        Ok((_, count)) => {
                            eprintln!(
                                "murphy watch: full re-lint: {} files, {count} offenses",
                                prev_files.len()
                            );
                        }
                        Err(e) => {
                            eprintln!("murphy watch: {}", e.message);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("murphy watch: {}", e.message);
                }
            }
            continue;
        }
        if cur_baseline_sig != baseline_sig {
            eprintln!("murphy watch: baseline changed — re-linting all files");
            baseline_sig = cur_baseline_sig;
            match read_batch_sources(&prev_files, worker_count) {
                Ok(sources) => {
                    match lint_and_print_watch_pass(
                        &sources,
                        &session,
                        args.format,
                        args.baseline.as_ref(),
                        args.clear,
                    ) {
                        Ok((_, count)) => {
                            eprintln!(
                                "murphy watch: full re-lint: {} files, {count} offenses",
                                prev_files.len()
                            );
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => {
                    eprintln!("murphy watch: {}", e.message);
                }
            }
            continue;
        }

        // Normal tick: re-discover (catches new files) + snapshot diff.
        let curr_files = match discover_watch_paths(
            &path_args,
            &session.config,
            &session.registry,
            args.preset.as_deref(),
        ) {
            Ok(files) => files,
            Err(e) => {
                eprintln!("murphy watch: discovery failed, retrying: {}", e.message);
                continue;
            }
        };
        let curr_snap = watch::snapshot_files(&curr_files);
        let mut diff = watch::diff_snapshots(&prev_snap, &curr_snap);
        // Files the snapshot could not stat (deleted between discovery
        // and stat, or unreadable) never enter the maps — merge them at
        // the path-list level so they still surface as added/removed
        // and get a proper read error instead of silently vanishing.
        for f in &curr_files {
            if !curr_snap.contains_key(f) && !prev_snap.contains_key(f) && !diff.added.contains(f) {
                diff.added.push(f.clone());
            }
        }
        for f in &prev_files {
            if !curr_files.contains(f) && !diff.removed.contains(f) {
                diff.removed.push(f.clone());
            }
        }
        diff.added.sort();
        diff.removed.sort();
        if diff.is_empty() {
            prev_files = curr_files;
            prev_snap = curr_snap;
            continue;
        }

        let summary = watch::format_change_summary(&diff);
        let targets = diff.lint_targets();
        if targets.is_empty() {
            // Removals only — nothing to lint.
            eprintln!("murphy watch: {summary} — nothing to lint");
            prev_files = curr_files;
            prev_snap = curr_snap;
            continue;
        }
        match read_batch_sources(&targets, worker_count) {
            Ok(sources) => {
                match lint_and_print_watch_pass(
                    &sources,
                    &session,
                    args.format,
                    args.baseline.as_ref(),
                    args.clear,
                ) {
                    Ok((_, count)) => {
                        eprintln!("murphy watch: {summary} — {count} offenses");
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(e) => {
                // Transient (file deleted mid-pass, editor half-write):
                // report and keep watching; the next tick re-diffs.
                eprintln!("murphy watch: {}", e.message);
            }
        }
        prev_files = curr_files;
        prev_snap = curr_snap;
    }
}

fn run_lint(args: &LintArgs) -> Result<u8, AppError> {
    // B6 `--profile-format` requires `--profile` (legacy contract, exit 2).
    // Validated before the `--explain` alias so bad usage errors even when
    // combined with `--explain`.
    if args.profile_format.is_some() && !args.profile {
        return Err(AppError::setup(
            "--profile-format requires --profile (use --profile --profile-format summary|speedscope)",
        ));
    }
    // B4 `--explain <cop_id>` alias: behave exactly like
    // `murphy explain <cop_id>` (human format), ignoring lint paths.
    // `--profile` is ignored in this mode (no lint run to profile).
    if let Some(cop_id) = &args.explain {
        return explain::run_explain(cop_id, explain::Format::Human);
    }
    let fix_mode = if args.fix_all {
        Some(FixMode::All)
    } else if args.fix {
        Some(FixMode::Safe)
    } else {
        None
    };
    let debug = args.debug;
    let no_cache = args.no_cache;
    let output_format = OutputFormat::from(args.format);
    let path_args: Vec<&str> = args.paths.iter().map(String::as_str).collect();

    let run_started = Instant::now();
    if debug {
        eprintln!("murphy: debug: config load start elapsed_ms=0");
    }
    let mut config = MurphyConfig::load_with_defaults_and_preset(
        Path::new("."),
        murphy_std::BUNDLED_DEFAULTS_YAML,
        args.preset.as_deref(),
    )
    .map_err(|e| AppError::setup(e.to_string()))?;
    if debug {
        eprintln!(
            "murphy: debug: config load done elapsed_ms={}",
            run_started.elapsed().as_millis()
        );
        eprintln!(
            "murphy: debug: cop registry load start elapsed_ms={}",
            run_started.elapsed().as_millis()
        );
    }
    // Warn when a staged `.murphy/plugins/*.so` is older than a fresher
    // build found in the usual candidate dirs (murphy-ghxy). Runs before
    // the registry `dlopen` so the hint shows even when the stale pack
    // would otherwise fail to load. Non-failing: the stale pack still
    // loads; the user refreshes via `murphy plugins sync --from <dir>`.
    for stale in murphy_core::plugin_sync::check_project_with_config(Path::new("."), &config, &[]) {
        eprintln!("{}", stale.warning());
    }
    let registry = CopRegistry::discover_with_config(Path::new("."), &config, builtin_pack())
        .map_err(|e| AppError::setup(e.to_string()))?;
    // Layer every loaded pack's bundled `default.yml` `AllCops` defaults
    // (e.g. `ActiveSupportExtensionsEnabled`) below user config, so loading
    // a pack such as murphy-rails flips its defaults on unless the user
    // overrode them. Must run before any `lint_source`/`run_to_fixpoint`
    // call, which read `config.active_support_extensions_enabled`.
    config.apply_pack_default_layers(&registry.pack_default_configs());
    // Warn (once per run) if the user opted back into a cop that's disabled
    // by default (via bundled defaults or arena migration). The enable is
    // honoured but the cop does not run until its implementation ships.
    cops::warn_user_enabled_disabled(&config, &registry);
    if debug {
        eprintln!(
            "murphy: debug: cop registry load done elapsed_ms={}, packs={:?}",
            run_started.elapsed().as_millis(),
            registry.pack_names()
        );
    }

    // ── path classification ────────────────────────────────────────────────
    let mut explicit_files: Vec<String> = Vec::new();
    let mut discover_roots: Vec<PathBuf> = Vec::new();
    if path_args.is_empty() {
        discover_roots.push(PathBuf::from("."));
    } else {
        for arg in &path_args {
            let p = Path::new(arg);
            if p.is_dir() {
                discover_roots.push(p.to_path_buf());
            } else {
                // existing file, missing file, or symlink — read_source handles
                explicit_files.push((*arg).to_string());
            }
        }
    }
    if debug {
        eprintln!(
            "murphy: debug: path classification: explicit_files={}, discover_roots={}, elapsed_ms={}",
            explicit_files.len(),
            discover_roots.len(),
            run_started.elapsed().as_millis()
        );
    }

    let mut all_paths: BTreeSet<String> = explicit_files.iter().cloned().collect();
    for root in &discover_roots {
        if debug {
            eprintln!(
                "murphy: debug: discover start root={:?} elapsed_ms={}",
                root,
                run_started.elapsed().as_millis()
            );
        }
        let discovered = if root == Path::new(".") {
            discover_with_config(root, &config).map_err(|e| AppError::setup(e.to_string()))?
        } else {
            // For non-cwd roots, load the root-local .murphy.yml.
            let mut local_config = MurphyConfig::load_with_defaults_and_preset(
                root,
                murphy_std::BUNDLED_DEFAULTS_YAML,
                args.preset.as_deref(),
            )
            .map_err(|e| AppError::setup(e.to_string()))?;
            // Layer the loaded packs' bundled `AllCops` defaults (notably
            // `AllCops.Exclude`, e.g. rails' `db/*schema.rb`) into the root-local
            // config so discovery under a non-cwd root honours pack excludes too.
            local_config.apply_pack_default_layers(&registry.pack_default_configs());
            discover_with_config(root, &local_config).map_err(|e| AppError::setup(e.to_string()))?
        };
        for p in discovered {
            all_paths.insert(p.to_string_lossy().into_owned());
        }
        if debug {
            eprintln!(
                "murphy: debug: discover done root={:?} found={} elapsed_ms={}",
                root,
                all_paths.len(),
                run_started.elapsed().as_millis()
            );
        }
    }

    // ── --since: diff-driven restriction (Phase 9 B5) ──────────────────────
    // Restricts the file list up front, so unchanged files are never parsed
    // or linted. Composes with --baseline/--format (they see the diff subset)
    // and with explicit paths (intersection: an unchanged explicit path is
    // skipped). Output-only restriction — the ADR 0006 JSON shape is unchanged.
    if let Some(git_ref) = &args.since {
        let changed =
            since::resolve_changed_files(git_ref).map_err(|e| AppError::setup(e.to_string()))?;
        if debug {
            eprintln!(
                "murphy: debug: since {git_ref:?} changed={} candidates={}",
                changed.len(),
                all_paths.len(),
            );
        }
        all_paths = since::restrict_to_changed(&all_paths, &changed)
            .into_iter()
            .collect();
    }

    let paths: Vec<String> = all_paths.into_iter().collect();
    if debug {
        eprintln!(
            "murphy: debug: read batch start files={} elapsed_ms={}",
            paths.len(),
            run_started.elapsed().as_millis()
        );
    }
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let sources = read_batch_sources(&paths, worker_count)?;
    if debug {
        eprintln!(
            "murphy: debug: read batch done files={} elapsed_ms={}",
            sources.len(),
            run_started.elapsed().as_millis()
        );
    }

    // `registry.cops()` allocates a fresh `Vec<&PluginCopV1>` bounded
    // by `&registry`'s lifetime; hold it for the rest of the run so the
    // borrowed references stay live across the dispatch + fixpoint loop.
    let cops_vec = registry.cops();
    let cops: &[&PluginCopV1] = &cops_vec;
    #[cfg(feature = "mruby-user-cops")]
    let mruby_cop_sources = load_mruby_cop_sources(registry.mruby_cop_paths())?;
    #[cfg(not(feature = "mruby-user-cops"))]
    let mruby_cop_sources = load_mruby_cop_sources(&[])?;
    let mruby_cops: &[MrubyCopSource] = &mruby_cop_sources;

    // ── arena binary cache (murphy-9cr.26) + persistent result cache (A5) ─
    // `Cache::open` / `ResultCache::open` consult `MURPHY_NO_CACHE`
    // themselves; `--no-cache` is the CLI-side opt-out. Either path
    // collapses to `Option::None`, which `parse_with_cache` understands
    // as "no caching" and `lint_files_memoized` as "no result skip".
    //
    // The result cache is per-path (content + path + cop-pack +
    // config), so per-cop path scopes are safe. It stays disabled for
    // mruby user cops (their sources live outside the fingerprint) and
    // `--fix` (intermediate fixpoint sources must not poison the
    // final-result entries). Otherwise the run falls back to the AST
    // cache — same output, just slower.
    let cache: Option<Cache> = if no_cache {
        None
    } else {
        Cache::open(murphy_translate::LAYER_VERSION)
    };
    let cache_ref = cache.as_ref();
    let result_cache: Option<ResultCache> =
        if no_cache || fix_mode.is_some() || !mruby_cops.is_empty() {
            None
        } else {
            let extra = lint_fingerprint(&registry, &config);
            ResultCache::open(&extra, murphy_translate::LAYER_VERSION)
        };
    let result_cache_ref = result_cache.as_ref();
    if debug {
        eprintln!(
            "murphy: debug: cache active={} result_cache active={} (--no-cache={} MURPHY_NO_CACHE={})",
            cache_ref.is_some(),
            result_cache_ref.is_some(),
            no_cache,
            std::env::var_os("MURPHY_NO_CACHE").is_some()
        );
    }

    let mut fix_debug: Vec<FileDebugInfo> = Vec::new();
    let mut sources_for_lint = sources;

    // ── --fix: fixpoint autocorrect + write-back ───────────────────────────
    if let Some(fix_mode) = fix_mode {
        let fix_cops = cops_for_fix_mode(cops, fix_mode);
        let fix_cops: &[&PluginCopV1] = &fix_cops;
        if debug {
            eprintln!(
                "murphy: debug: fixpoint start elapsed_ms={}",
                run_started.elapsed().as_millis()
            );
        }
        let mut next_sources: Vec<(String, String)> = Vec::with_capacity(sources_for_lint.len());
        for (path, source) in &sources_for_lint {
            let outcome = run_to_fixpoint(
                source,
                |s| lint_closure_edits(s, path, fix_cops, mruby_cops, &config, cache_ref),
                MAX_FIX_ITERATIONS,
            );
            if outcome.corrected != *source {
                write_back_atomic(Path::new(path), &outcome.corrected)?;
            }
            if debug {
                fix_debug.push(FileDebugInfo {
                    path: path.clone(),
                    iterations: outcome.iterations,
                    status: outcome.status,
                });
            }
            next_sources.push((path.clone(), outcome.corrected));
        }
        sources_for_lint = next_sources;
        if debug {
            eprintln!(
                "murphy: debug: fixpoint done elapsed_ms={}",
                run_started.elapsed().as_millis()
            );
            for info in &fix_debug {
                eprintln!(
                    "murphy: debug: fix {} iterations={} status={:?}",
                    info.path, info.iterations, info.status
                );
            }
        }
    }

    if debug {
        eprintln!(
            "murphy: debug: lint pass start files={} elapsed_ms={}",
            sources_for_lint.len(),
            run_started.elapsed().as_millis()
        );
    }
    // B6 `--profile`: per-cop timed lint. The offenses are identical to
    // the normal path (same dispatch inputs); the summary carries the
    // cop x file matrix for the stdout profile JSON below.
    let mut profile_summary: Option<ProfileSummary> = None;
    let flat_offenses: Vec<Offense> = if args.profile {
        let (offenses, summary, timings) =
            lint_files_profiled(&sources_for_lint, cops, mruby_cops, &config, cache_ref);
        if debug {
            for (path, parse_us, cops_us) in &timings {
                eprintln!(
                    "murphy: debug: lint {} parse_us={} cops_us={}",
                    path, parse_us, cops_us
                );
            }
        }
        profile_summary = Some(summary);
        offenses
    } else if debug {
        let (offenses, timings) =
            lint_files_memoized_debug(&sources_for_lint, cops, mruby_cops, &config, cache_ref);
        for (path, parse_us, cops_us) in &timings {
            eprintln!(
                "murphy: debug: lint {} parse_us={} cops_us={}",
                path, parse_us, cops_us
            );
        }
        offenses
    } else {
        lint_files_memoized(
            &sources_for_lint,
            cops,
            mruby_cops,
            &config,
            cache_ref,
            result_cache_ref,
        )
    };
    let mut offenses = aggregate_with_config(flat_offenses, &config);
    // B4 enrichment (murphy-fmw.2.4): attach fixed-template
    // `documentation_url` / `rationale` / `fix_example` to every offense.
    // Descriptions come from the cop registry (author-controlled), never
    // from offense messages or source text, so this is prompt-injection
    // safe. Extend-only: new keys, no existing key changes (ADR 0006).
    {
        let desc_map = explain::description_map(&registry);
        for offense in &mut offenses {
            let desc =
                explain::lookup_description(&desc_map, &offense.cop_name).unwrap_or_default();
            murphy_core::enrich_offense(offense, &desc);
        }
    }
    // ── baseline generate / filter (Phase 9 B3) ────────────────────────────
    // Filtering happens AFTER aggregation and B4 enrichment, BEFORE
    // formatting, so every `--format` sees the filtered list while the ADR
    // 0006 default JSON shape itself is unchanged (output-only filtering).
    // Generating from enriched offenses keeps frozen counts aligned with
    // reported output.
    if let Some(out) = &args.generate_baseline {
        let baseline = Baseline::generate(&offenses);
        baseline.save(out).map_err(|e| {
            AppError::setup(format!("cannot write baseline {}: {e}", out.display()))
        })?;
        eprintln!(
            "murphy: baseline: froze {} offenses in {} entries to {}",
            offenses.len(),
            baseline.entry_count(),
            out.display()
        );
    }
    if let Some(path) = &args.baseline {
        let baseline = Baseline::load(path).map_err(|e| {
            AppError::setup(format!("cannot load baseline {}: {e}", path.display()))
        })?;
        let before = offenses.len();
        offenses = baseline.filter_offenses(offenses);
        if debug {
            eprintln!(
                "murphy: debug: baseline {} suppressed={} remaining={}",
                path.display(),
                before - offenses.len(),
                offenses.len()
            );
        }
    }
    if debug {
        eprintln!(
            "murphy: debug: lint pass done offenses={} elapsed_ms={}",
            offenses.len(),
            run_started.elapsed().as_millis()
        );
    }

    let exit = if offenses.is_empty() {
        EXIT_OK
    } else {
        EXIT_OFFENSES
    };

    // B6 `--profile`: stdout is the profile JSON (summary or speedscope),
    // NOT lint output — `--format` is ignored. The exit code still reflects
    // lint offenses, so `murphy lint --profile ... > profile.json` stays
    // CI-usable (Phase 9 gate 5).
    if let Some(summary) = profile_summary {
        let payload = match args.profile_format.unwrap_or(ProfileFormatArg::Summary) {
            ProfileFormatArg::Summary => summary.to_summary_profile(),
            ProfileFormatArg::Speedscope => summary.to_speedscope(),
        };
        let mut stdout = std::io::stdout().lock();
        if let Err(e) = writeln!(stdout, "{payload}") {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(exit);
            }
            return Err(AppError::setup(format!("failed to write stdout: {e}")));
        }
        return Ok(exit);
    }

    let file_paths: Vec<String> = sources_for_lint.iter().map(|(p, _)| p.clone()).collect();
    let formatted =
        format_lint_output(&offenses, &file_paths, output_format).map_err(AppError::setup)?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{formatted}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(exit);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }

    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::{RawSlice, SEVERITY_UNSET, TRISTATE_UNSET, tristate_to_wire};

    static EMPTY_KINDS: &[murphy_plugin_api::NodeKindTag] = &[];

    unsafe extern "C" fn noop_dispatch(
        _node: murphy_plugin_api::NodeId,
        _cx: *const murphy_plugin_api::CxRaw,
    ) -> i32 {
        0
    }

    const fn test_cop(name: &'static str, safe_autocorrect: u8) -> PluginCopV1 {
        PluginCopV1 {
            size: std::mem::size_of::<PluginCopV1>(),
            name: RawSlice::from_str(name),
            description: RawSlice::EMPTY,
            default_severity: SEVERITY_UNSET,
            default_enabled: TRISTATE_UNSET,
            safe: TRISTATE_UNSET,
            safe_autocorrect,
            minimum_target_ruby_version: 0,
            maximum_target_ruby_version: 0,
            options_ptr: std::ptr::null(),
            options_len: 0,
            kinds_ptr: EMPTY_KINDS.as_ptr(),
            kinds_len: EMPTY_KINDS.len(),
            dispatch: noop_dispatch,
            send_methods_ptr: std::ptr::null(),
            send_methods_len: 0,
        }
    }

    static SAFE_FIX_COP: PluginCopV1 = test_cop("Test/SafeFix", tristate_to_wire(Some(true)));
    static UNSAFE_FIX_COP: PluginCopV1 = test_cop("Test/UnsafeFix", tristate_to_wire(Some(false)));
    static UNSPECIFIED_FIX_COP: PluginCopV1 = test_cop("Test/UnspecifiedFix", TRISTATE_UNSET);

    #[test]
    fn safe_fix_mode_skips_unsafe_autocorrect_cops() {
        let all = [&SAFE_FIX_COP, &UNSAFE_FIX_COP, &UNSPECIFIED_FIX_COP];

        let selected = cops_for_fix_mode(&all, FixMode::Safe);
        let names: Vec<&str> = selected.iter().map(|cop| plugin_cop_name(cop)).collect();

        assert_eq!(names, vec!["Test/SafeFix", "Test/UnspecifiedFix"]);
    }

    #[test]
    fn all_fix_mode_keeps_unsafe_autocorrect_cops() {
        let all = [&SAFE_FIX_COP, &UNSAFE_FIX_COP, &UNSPECIFIED_FIX_COP];

        let selected = cops_for_fix_mode(&all, FixMode::All);
        let names: Vec<&str> = selected.iter().map(|cop| plugin_cop_name(cop)).collect();

        assert_eq!(
            names,
            vec!["Test/SafeFix", "Test/UnsafeFix", "Test/UnspecifiedFix"]
        );
    }
}
