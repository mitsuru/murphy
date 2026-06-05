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
//!
//! `murphy lint --profile / --profile-format` await re-introduction
//! once the new dispatcher carries its own per-cop timing path (.22
//! perf-gate follow-up).

mod cops;
mod lsp;

use clap::{Parser, Subcommand, ValueEnum};
use murphy_cache::Cache;
#[cfg(feature = "mruby-user-cops")]
use murphy_core::{AstContext, run_mruby_cop_isolated};
use murphy_core::{
    CopRegistry, FixpointStatus, MurphyConfig, Offense, SYNTAX_COP_NAME, Severity,
    aggregate_with_config, ast_to_sexp, discover_with_config, dispatch,
    migrate_rubocop_yml_to_murphy_yml, parse, parse_with_cache, run_to_fixpoint,
};
use murphy_plugin_api::{PluginCopV1, PluginRegistration, tristate_from_wire};
use murphy_reporting::{OutputFormat, format_lint_output};

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
    /// Disable the arena AST binary cache for this run.
    #[arg(long)]
    no_cache: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: LintOutputFormatArg,
    /// Files or directories to lint. With no paths, Murphy discovers from cwd.
    #[arg(value_name = "PATH", num_args = 0.., trailing_var_arg = true)]
    paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LintOutputFormatArg {
    Human,
    Json,
    Progress,
}

impl From<LintOutputFormatArg> for OutputFormat {
    fn from(format: LintOutputFormatArg) -> Self {
        match format {
            LintOutputFormatArg::Human => OutputFormat::Human,
            LintOutputFormatArg::Json => OutputFormat::Json,
            LintOutputFormatArg::Progress => OutputFormat::Progress,
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
    let mut offenses = match parse_with_cache(source, file, cache) {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            let scoped_cops = scoped_native_cops(cops, config, file);
            dispatch::run_cops_with_options_and_target_rails_version(
                &ast,
                &scoped_cops,
                &mut sink,
                config.target_rails_version,
                config.active_support_extensions_enabled,
                |name| config.cop_options_json(name),
            );
            let mut offenses = sink.into_offenses();
            offenses.extend(run_mruby_user_cops(source, file, mruby_cops, config));
            offenses
        }
        Err(err) => vec![Offense::new(
            file,
            SYNTAX_COP_NAME,
            err.range,
            Severity::Error,
            &err.message,
        )],
    };
    offenses = apply_inline_directive_filter(offenses, source);
    offenses
}

#[cfg(feature = "mruby-user-cops")]
fn run_mruby_user_cops(
    source: &str,
    file: &str,
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
) -> Vec<Offense> {
    if mruby_cops.is_empty() {
        return Vec::new();
    }
    let applicable_cops: Vec<_> = mruby_cops
        .iter()
        .filter(|cop| config.cop_applies_to_file(&cop.name, Path::new(file)))
        .collect();
    if applicable_cops.is_empty() {
        return Vec::new();
    }
    let ctx = AstContext::new(source.as_bytes().to_vec());
    applicable_cops
        .into_iter()
        .flat_map(|cop| run_mruby_cop_isolated(&ctx, &cop.source, &cop.name, file))
        .collect()
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
    let offenses = match parsed {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            let scoped_cops = scoped_native_cops(cops, config, file);
            dispatch::run_cops_with_options_and_target_rails_version(
                &ast,
                &scoped_cops,
                &mut sink,
                config.target_rails_version,
                config.active_support_extensions_enabled,
                |name| config.cop_options_json(name),
            );
            let mut offenses = sink.into_offenses();
            offenses.extend(run_mruby_user_cops(source, file, mruby_cops, config));
            offenses
        }
        Err(err) => vec![Offense::new(
            file,
            SYNTAX_COP_NAME,
            err.range,
            Severity::Error,
            &err.message,
        )],
    };
    let cops_micros = cops_started.elapsed().as_micros();
    TimedOffenses {
        offenses: apply_inline_directive_filter(offenses, source),
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
    cop: Option<String>,
}

#[derive(Debug, Clone)]
struct DirectiveState {
    disable_all: bool,
    disabled_cops: BTreeSet<String>,
    todo_all: bool,
    todo_cops: BTreeSet<String>,
    line_start: usize,
    line_end: usize,
}

fn parse_inline_directive(line: &str) -> Option<InlineDirective> {
    let hash_pos = line.find('#')?;
    let comment = line[hash_pos + 1..].trim_start();
    let rest = comment.strip_prefix("murphy:")?;
    let mut parts = rest.split_whitespace();
    let keyword = parts.next()?;
    let cop = parts.next().map(str::to_string);
    let kind = match keyword {
        "disable" => InlineDirectiveKind::Disable,
        "enable" => InlineDirectiveKind::Enable,
        "todo" => InlineDirectiveKind::Todo,
        _ => return None,
    };
    Some(InlineDirective { kind, cop })
}

fn directive_states_by_line(source: &str) -> Vec<DirectiveState> {
    let mut states = Vec::new();
    let mut disable_all = false;
    let mut disabled_cops: BTreeSet<String> = BTreeSet::new();

    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        let mut todo_all = false;
        let mut todo_cops: BTreeSet<String> = BTreeSet::new();

        if let Some(directive) = parse_inline_directive(line) {
            match (directive.kind, directive.cop) {
                (InlineDirectiveKind::Disable, Some(cop)) => {
                    disabled_cops.insert(cop);
                }
                (InlineDirectiveKind::Disable, None) => disable_all = true,
                (InlineDirectiveKind::Enable, Some(cop)) => {
                    disabled_cops.remove(&cop);
                }
                (InlineDirectiveKind::Enable, None) => {
                    disable_all = false;
                    disabled_cops.clear();
                }
                (InlineDirectiveKind::Todo, Some(cop)) => {
                    todo_cops.insert(cop);
                }
                (InlineDirectiveKind::Todo, None) => todo_all = true,
            }
        }

        states.push(DirectiveState {
            disable_all,
            disabled_cops: disabled_cops.clone(),
            todo_all,
            todo_cops,
            line_start,
            line_end,
        });

        offset = line_end;
    }
    states
}

fn is_directive_disabled(offense: &Offense, states: &[DirectiveState]) -> bool {
    if offense.cop_name == SYNTAX_COP_NAME {
        return false;
    }
    let start = offense.range.start_offset as usize;
    for state in states {
        if start >= state.line_start && start < state.line_end {
            return state.disable_all
                || state.disabled_cops.contains(&offense.cop_name)
                || state.todo_all
                || state.todo_cops.contains(&offense.cop_name);
        }
    }
    false
}

fn apply_inline_directive_filter(mut offenses: Vec<Offense>, source: &str) -> Vec<Offense> {
    if offenses.is_empty() {
        return Vec::new();
    }
    let states = directive_states_by_line(source);
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
fn lint_files_memoized(
    sources: &[(String, String)],
    cops: &[&PluginCopV1],
    mruby_cops: &[MrubyCopSource],
    config: &MurphyConfig,
    cache: Option<&Cache>,
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
            // Lint once against the representative path. Then for every
            // additional path sharing the same content, clone the offense
            // list with `file` rewritten.
            let representative = paths[0];
            let base = lint_source(content, representative, cops, mruby_cops, config, cache);
            let mut all: Vec<Offense> = Vec::with_capacity(base.len() * paths.len());
            all.extend(base.iter().cloned());
            for &other in &paths[1..] {
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
        CliCommand::Migrate(migrate_args) => run_migrate(&migrate_args),
        CliCommand::Ast(ast_args) => run_ast(&ast_args),
        CliCommand::Cops(cops_args) => run_cops(&cops_args),
        CliCommand::Lsp(lsp_args) => lsp::run(&lsp_args.args),
        CliCommand::NewCop(new_cop_args) => new_cop_command(&new_cop_args.cop),
        CliCommand::TestCop(test_cop_args) => test_cop_command(&test_cop_args.spec_files),
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
        CopsCommand::List(list_args) => cops::list_with_format(list_args.format.into()),
    }
}

fn run_lint(args: &LintArgs) -> Result<u8, AppError> {
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
    let config =
        MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
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
    let registry = CopRegistry::discover_with_config(Path::new("."), &config, builtin_pack())
        .map_err(|e| AppError::setup(e.to_string()))?;
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
            let local_config =
                MurphyConfig::load_with_defaults(root, murphy_std::BUNDLED_DEFAULTS_YAML)
                    .map_err(|e| AppError::setup(e.to_string()))?;
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

    // ── arena binary cache (murphy-9cr.26) ─────────────────────────────────
    // `Cache::open` consults `MURPHY_NO_CACHE` itself; `--no-cache` is the
    // CLI-side opt-out. Either path collapses to `Option::None`, which
    // `parse_with_cache` understands as "no caching".
    let cache: Option<Cache> = if no_cache {
        None
    } else {
        Cache::open(murphy_translate::LAYER_VERSION)
    };
    let cache_ref = cache.as_ref();
    if debug {
        eprintln!(
            "murphy: debug: cache active={} (--no-cache={} MURPHY_NO_CACHE={})",
            cache_ref.is_some(),
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
    let flat_offenses: Vec<Offense> = if debug {
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
        lint_files_memoized(&sources_for_lint, cops, mruby_cops, &config, cache_ref)
    };
    let offenses = aggregate_with_config(flat_offenses, &config);
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
