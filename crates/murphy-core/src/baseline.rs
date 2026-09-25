//! Baseline / suppress file (`.murphy-baseline.toml`) — Phase 9 B3.
//!
//! A baseline freezes *existing* violations so `murphy lint` reports only
//! *new* ones. It is the RuboCop `.rubocop_todo.yml` equivalent, in TOML form
//! consistent with the historical `murphy.toml` (ADR 0015) rather than YAML.
//!
//! ## Granularity decision (roadmap §4.2 debate, decided here)
//!
//! Entries are keyed by **(file, cop ID)** with a per-entry **count** — NOT
//! line-level. Line/range/message fingerprints are brittle: any edit shifts
//! lines, and message wording changes across cop versions. `(file, cop)`
//! matches RuboCop's todo granularity (`Exclude:` file lists per cop) and is
//! stable under refactoring inside the file.
//!
//! The count refines RuboCop's "suppress the whole file" behaviour: a
//! generated entry records *how many* offenses of that cop the file had, so a
//! *new* offense of the same cop in the same file still surfaces once the
//! count is exceeded. A hand-written entry may omit `count` to suppress *all*
//! current and future offenses of that cop in that file (RuboCop parity).
//!
//! Filtering is deterministic: the first `count` offenses per `(file, cop)`
//! in aggregate order are treated as baselined, the rest are reported.
//!
//! ## Responsibility split with `.murphyignore`
//!
//! - `.murphyignore` (gitignore syntax): **file-level discovery exclusion**.
//!   Ignored files are never parsed or linted — they never appear in output.
//! - `.murphy-baseline.toml`: **cop-level freeze**. Listed files are still
//!   linted; only the *known* `(file, cop)` offenses are filtered from
//!   output. New cops, new files, and over-count offenses still surface.
//!
//! Use `.murphyignore` for files that should never be linted (vendored code,
//! generated output). Use the baseline for legacy adoption: freeze today's
//! violations, keep the lint green, and fix or triage incrementally.
//!
//! ## ADR 0006 contract
//!
//! The baseline **filters output only**. The default JSON offense shape is
//! unchanged; every `--format` (human/json/progress) sees the filtered list.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::Offense;

/// Baseline file format version. Version `1` is the only version accepted.
pub const BASELINE_VERSION: u32 = 1;

/// Conventional baseline filename in the project root.
pub const DEFAULT_BASELINE_FILENAME: &str = ".murphy-baseline.toml";

/// Suppress-all allowance: an entry with no `count` suppresses every offense
/// of its `(file, cop)` key, present or future.
const SUPPRESS_ALL: u64 = u64::MAX;

/// A parsed baseline: `(normalized file, cop ID)` → suppressed offense count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Baseline {
    entries: BTreeMap<(String, String), u64>,
}

/// Failure to load or parse a baseline file. The CLI maps every variant to
/// exit `2` (config/setup error) via its `AppError::setup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineError {
    /// The baseline file could not be read or written.
    Io(String),
    /// The file is not valid baseline TOML, or an entry is malformed.
    Parse(String),
    /// The file declares an unsupported `version`.
    UnsupportedVersion(u32),
}

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BaselineError::Io(m) => write!(f, "{m}"),
            BaselineError::Parse(m) => write!(f, "invalid baseline: {m}"),
            BaselineError::UnsupportedVersion(v) => write!(
                f,
                "invalid baseline: unsupported version {v} (expected {BASELINE_VERSION})"
            ),
        }
    }
}

impl std::error::Error for BaselineError {}

/// Serde shape of the TOML document. `count` is optional: absent means
/// suppress-all (RuboCop todo parity for hand-written entries).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct BaselineDocument {
    version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<BaselineEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct BaselineEntry {
    file: String,
    cop: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count: Option<u64>,
}

/// Normalize an offense path for baseline keys: strip a leading `./` so
/// discovery-shaped paths (`./a.rb`) and explicit-arg paths (`a.rb`) share
/// one key. Matching is otherwise verbatim — generate and lint from the same
/// working directory.
fn normalize_path(file: &str) -> &str {
    let mut rest = file;
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest
}

impl Baseline {
    /// An empty baseline: [`Self::filter_offenses`] suppresses nothing.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of distinct `(file, cop)` entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// True when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Suppressed offense count for one `(file, cop)` key, or `None` when the
    /// key is not baselined. `u64::MAX` means suppress-all.
    pub fn allowance(&self, file: &str, cop: &str) -> Option<u64> {
        self.entries
            .get(&(normalize_path(file).to_string(), cop.to_string()))
            .copied()
    }

    /// Build a baseline freezing exactly the given offenses: one entry per
    /// distinct `(file, cop)` with `count` = observed occurrences. Offense
    /// paths are normalized (see [`normalize_path`]).
    pub fn generate(offenses: &[Offense]) -> Self {
        let mut entries: BTreeMap<(String, String), u64> = BTreeMap::new();
        for offense in offenses {
            let key = (
                normalize_path(&offense.file).to_string(),
                offense.cop_name.clone(),
            );
            *entries.entry(key).or_insert(0) += 1;
        }
        Baseline { entries }
    }

    /// Parse baseline TOML text. Duplicate entries for one key merge: any
    /// suppress-all entry wins, otherwise counts add (saturating).
    pub fn from_toml_str(text: &str) -> Result<Self, BaselineError> {
        let doc: BaselineDocument =
            toml::from_str(text).map_err(|e| BaselineError::Parse(e.to_string()))?;
        if doc.version != BASELINE_VERSION {
            return Err(BaselineError::UnsupportedVersion(doc.version));
        }
        let mut entries: BTreeMap<(String, String), u64> = BTreeMap::new();
        for entry in doc.entries {
            if entry.file.is_empty() {
                return Err(BaselineError::Parse(
                    "entry has an empty `file`".to_string(),
                ));
            }
            if entry.cop.is_empty() {
                return Err(BaselineError::Parse("entry has an empty `cop`".to_string()));
            }
            let key = (normalize_path(&entry.file).to_string(), entry.cop);
            let allowance = entry.count.unwrap_or(SUPPRESS_ALL);
            entries
                .entry(key)
                .and_modify(|slot| {
                    *slot = if *slot == SUPPRESS_ALL || allowance == SUPPRESS_ALL {
                        SUPPRESS_ALL
                    } else {
                        slot.saturating_add(allowance)
                    };
                })
                .or_insert(allowance);
        }
        Ok(Baseline { entries })
    }

    /// Read and parse a baseline file.
    pub fn load(path: &Path) -> Result<Self, BaselineError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| BaselineError::Io(format!("cannot read {}: {e}", path.display())))?;
        Self::from_toml_str(&text)
    }

    /// Serialize to baseline TOML. Entries are sorted by `(file, cop)` via
    /// the `BTreeMap`, so generation is deterministic. Suppress-all entries
    /// serialize without `count`.
    pub fn to_toml(&self) -> String {
        let doc = BaselineDocument {
            version: BASELINE_VERSION,
            entries: self
                .entries
                .iter()
                .map(|((file, cop), allowance)| BaselineEntry {
                    file: file.clone(),
                    cop: cop.clone(),
                    count: if *allowance == SUPPRESS_ALL {
                        None
                    } else {
                        Some(*allowance)
                    },
                })
                .collect(),
        };
        let mut out = String::from(
            "# Murphy baseline — freeze existing violations, report only new ones.\n\
             # Generated by `murphy lint --generate-baseline`; safe to check in.\n\
             # Each entry suppresses up to `count` offenses of `cop` in `file`.\n\
             # Omit `count` to suppress all offenses of that cop in that file.\n",
        );
        out.push_str(&toml::to_string(&doc).unwrap_or_else(|_| "version = 1\n".to_string()));
        out
    }

    /// Write baseline TOML to `path`.
    pub fn save(&self, path: &Path) -> Result<(), BaselineError> {
        std::fs::write(path, self.to_toml())
            .map_err(|e| BaselineError::Io(format!("cannot write {}: {e}", path.display())))
    }

    /// Split offenses into baselined (suppressed) vs new (reported): the
    /// first `count` offenses per `(file, cop)` key in input order are
    /// suppressed. Input is expected in aggregate order (deterministic), so
    /// which offenses count as "known" is stable across runs.
    pub fn filter_offenses(&self, offenses: Vec<Offense>) -> Vec<Offense> {
        if self.entries.is_empty() {
            return offenses;
        }
        let mut seen: BTreeMap<(String, String), u64> = BTreeMap::new();
        offenses
            .into_iter()
            .filter(|offense| {
                let key = (
                    normalize_path(&offense.file).to_string(),
                    offense.cop_name.clone(),
                );
                match self.entries.get(&key) {
                    None => true,
                    Some(&allowed) => {
                        let n = seen.entry(key).or_insert(0);
                        *n += 1;
                        *n > allowed
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offense::{Range, Severity};

    fn off(file: &str, cop: &str, start: u32) -> Offense {
        Offense::new(
            file,
            cop,
            Range {
                start_offset: start,
                end_offset: start + 5,
            },
            Severity::Warning,
            "msg",
        )
    }

    #[test]
    fn generate_then_filter_suppresses_everything() {
        let offenses = vec![off("a.rb", "Lint/Debugger", 0), off("b.rb", "Style/Foo", 3)];
        let baseline = Baseline::generate(&offenses);
        assert_eq!(baseline.entry_count(), 2);
        assert_eq!(baseline.filter_offenses(offenses), Vec::new());
    }

    #[test]
    fn count_semantics_report_new_same_cop_offenses() {
        let old = vec![off("a.rb", "Lint/Debugger", 0)];
        let baseline = Baseline::generate(&old);
        // One more offense of the same cop in the same file is NEW.
        let now = vec![
            off("a.rb", "Lint/Debugger", 0),
            off("a.rb", "Lint/Debugger", 99),
        ];
        let kept = baseline.filter_offenses(now);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].range.start_offset, 99);
    }

    #[test]
    fn other_cops_and_files_always_surface() {
        let baseline = Baseline::generate(&[off("a.rb", "Lint/Debugger", 0)]);
        let now = vec![
            off("a.rb", "Style/Foo", 0),     // other cop, same file
            off("b.rb", "Lint/Debugger", 0), // same cop, other file
        ];
        assert_eq!(baseline.filter_offenses(now).len(), 2);
    }

    #[test]
    fn missing_count_suppresses_all() {
        let baseline = Baseline::from_toml_str(
            "version = 1\n[[entries]]\nfile = \"a.rb\"\ncop = \"Lint/Debugger\"\n",
        )
        .expect("hand-written suppress-all must parse");
        assert_eq!(baseline.allowance("a.rb", "Lint/Debugger"), Some(u64::MAX));
        let now = vec![
            off("a.rb", "Lint/Debugger", 0),
            off("a.rb", "Lint/Debugger", 1),
        ];
        assert!(baseline.filter_offenses(now).is_empty());
    }

    #[test]
    fn toml_roundtrip_preserves_counts() {
        let offenses = vec![
            off("a.rb", "Lint/Debugger", 0),
            off("a.rb", "Lint/Debugger", 10),
            off("b.rb", "Style/Foo", 0),
        ];
        let baseline = Baseline::generate(&offenses);
        let text = baseline.to_toml();
        let parsed = Baseline::from_toml_str(&text).expect("generated TOML must parse");
        assert_eq!(parsed, baseline);
        assert_eq!(parsed.allowance("a.rb", "Lint/Debugger"), Some(2));
        assert_eq!(parsed.allowance("b.rb", "Style/Foo"), Some(1));
    }

    #[test]
    fn leading_dot_slash_paths_share_one_key() {
        let baseline = Baseline::generate(&[off("./a.rb", "Lint/Debugger", 0)]);
        assert_eq!(baseline.allowance("a.rb", "Lint/Debugger"), Some(1));
        assert!(
            baseline
                .filter_offenses(vec![off("a.rb", "Lint/Debugger", 0)])
                .is_empty()
        );
    }

    #[test]
    fn duplicate_entries_sum_counts() {
        let baseline = Baseline::from_toml_str(
            "version = 1\n[[entries]]\nfile = \"a.rb\"\ncop = \"C\"\ncount = 1\n\
             [[entries]]\nfile = \"a.rb\"\ncop = \"C\"\ncount = 2\n",
        )
        .expect("duplicates must parse");
        assert_eq!(baseline.allowance("a.rb", "C"), Some(3));
    }

    #[test]
    fn duplicate_with_suppress_all_wins() {
        let baseline = Baseline::from_toml_str(
            "version = 1\n[[entries]]\nfile = \"a.rb\"\ncop = \"C\"\ncount = 1\n\
             [[entries]]\nfile = \"a.rb\"\ncop = \"C\"\n",
        )
        .expect("duplicates must parse");
        assert_eq!(baseline.allowance("a.rb", "C"), Some(u64::MAX));
    }

    #[test]
    fn bad_version_is_rejected() {
        let err = Baseline::from_toml_str("version = 99\n").unwrap_err();
        assert_eq!(err, BaselineError::UnsupportedVersion(99));
    }

    #[test]
    fn malformed_toml_and_empty_fields_are_rejected() {
        assert!(Baseline::from_toml_str("not toml [[[").is_err());
        assert!(
            Baseline::from_toml_str("version = 1\n[[entries]]\nfile = \"\"\ncop = \"C\"\n")
                .is_err()
        );
        assert!(
            Baseline::from_toml_str("version = 1\n[[entries]]\nfile = \"a.rb\"\ncop = \"\"\n")
                .is_err()
        );
    }

    #[test]
    fn empty_baseline_suppresses_nothing() {
        let baseline = Baseline::empty();
        let now = vec![off("a.rb", "C", 0)];
        assert_eq!(baseline.filter_offenses(now.clone()), now);
    }
}
