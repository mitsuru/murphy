//! `Naming/InclusiveLanguage` — recommend inclusive alternatives for flagged terms.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/InclusiveLanguage
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-e7bz.41.1, murphy-e7bz.41.2, murphy-e7bz.41.3]
//! notes: >
//!   Ports RuboCop's configurable FlaggedTerms, Check* switches, comments,
//!   strings, symbols, identifiers, constants, variables, filepath scan,
//!   suggestion messages, and single-suggestion autocorrection. It merges
//!   partial term config with RuboCop defaults and distinguishes labels,
//!   quoted/percent-array symbols, and alias/undef names. Nested `inherit_mode`
//!   array merges apply recursively and preserve custom-term insertion order.
//!   The cop is disabled by default. Remaining limits: Rust `regex` lacks Ruby
//!   look-around/backreferences, and config JSON loses YAML Regexp tags, making
//!   tagged regexes and slash-delimited plain strings ambiguous. Filepath
//!   offenses use `Range::NO_LOCATION` via `Cx::emit_file_offense` so no
//!   fabricated range is serialized or rendered. Heredoc
//!   strings use label-paired body ranges; ambiguous or incomplete delimiters
//!   are skipped so corrections cannot touch the opener or terminator. AST
//!   ranges are collected once per file because the cop macro cannot combine
//!   file and node handlers.
//! ```
//!
//! The parser's `Str` leaves represent string content in plain, interpolated,
//! regexp, xstring, and interpolated-symbol literals. Heredoc opener ranges are
//! mapped to their bodies, excluding delimiters. We inspect string leaves only,
//! so code inside `#{...}` remains governed by identifier/constant rules.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use murphy_plugin_api::regex::{Regex, RegexBuilder};
use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct InclusiveLanguage;

/// A single entry under RuboCop's `FlaggedTerms` option.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlaggedTermOptions {
    /// Optional Ruby-regexp source. When absent, the term key is itself a
    /// regexp, matching RuboCop's `Regexp.new(term)` behavior.
    pub regex: Option<String>,
    /// Match whole words only. Ignored when `regex` is provided, as upstream.
    pub whole_word: bool,
    /// A string or list of strings shown in the offense message.
    pub suggestions: Option<serde_json::Value>,
    /// Allowed usages masked before flagged terms are scanned.
    pub allowed_regex: Vec<String>,
}

/// Manually decoded because `FlaggedTerms` is a nested map and the derive
/// supports only flat option fields. The empty schema is intentional: the host
/// passes the complete rule map to `from_config_json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub check_identifiers: bool,
    pub check_constants: bool,
    pub check_variables: bool,
    pub check_strings: bool,
    pub check_symbols: bool,
    pub check_comments: bool,
    pub check_filepaths: bool,
    pub flagged_terms: Vec<(String, FlaggedTermOptions)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            check_identifiers: true,
            check_constants: true,
            check_variables: true,
            check_strings: false,
            check_symbols: true,
            check_comments: true,
            check_filepaths: true,
            flagged_terms: vec![
                (
                    "whitelist".to_string(),
                    FlaggedTermOptions {
                        regex: Some(r"white[-_\s]?list".to_string()),
                        suggestions: Some(serde_json::json!(["allowlist", "permit"])),
                        ..FlaggedTermOptions::default()
                    },
                ),
                (
                    "blacklist".to_string(),
                    FlaggedTermOptions {
                        regex: Some(r"black[-_\s]?list".to_string()),
                        suggestions: Some(serde_json::json!(["denylist", "block"])),
                        ..FlaggedTermOptions::default()
                    },
                ),
                (
                    "slave".to_string(),
                    FlaggedTermOptions {
                        whole_word: true,
                        suggestions: Some(serde_json::json!(
                            ["replica", "secondary", "follower"]
                        )),
                        ..FlaggedTermOptions::default()
                    },
                ),
            ],
        }
    }
}

impl CopOptions for Options {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let object = value.as_object().ok_or_else(ConfigError::not_an_object)?;
        let mut options = Self::default();

        parse_bool_option(object, "CheckIdentifiers", &mut options.check_identifiers)?;
        parse_bool_option(object, "CheckConstants", &mut options.check_constants)?;
        parse_bool_option(object, "CheckVariables", &mut options.check_variables)?;
        parse_bool_option(object, "CheckStrings", &mut options.check_strings)?;
        parse_bool_option(object, "CheckSymbols", &mut options.check_symbols)?;
        parse_bool_option(object, "CheckComments", &mut options.check_comments)?;
        parse_bool_option(object, "CheckFilepaths", &mut options.check_filepaths)?;

        if let Some(flagged_terms) = object.get("FlaggedTerms") {
            let flagged_terms = flagged_terms.as_object().ok_or_else(|| {
                ConfigError::type_mismatch("FlaggedTerms", "object of term definitions")
            })?;
            for (term, definition) in flagged_terms {
                if definition.is_null() {
                    options.flagged_terms.retain(|(existing, _)| existing != term);
                    continue;
                }
                let definition = definition.as_object().ok_or_else(|| {
                    ConfigError::type_mismatch(
                        format!("FlaggedTerms.{term}"),
                        "object or null",
                    )
                })?;
                let mut term_options = options
                    .flagged_terms
                    .iter()
                    .find(|(existing, _)| existing == term)
                    .map_or_else(FlaggedTermOptions::default, |(_, value)| value.clone());
                if let Some(value) = definition.get("Regex") {
                    term_options.regex = match value {
                        serde_json::Value::Null => None,
                        serde_json::Value::String(source) => {
                            Some(strip_ruby_regex_delimiters(source))
                        }
                        _ => {
                            return Err(ConfigError::type_mismatch(
                                format!("FlaggedTerms.{term}.Regex"),
                                "string regexp or null",
                            ));
                        }
                    };
                }
                if let Some(value) = definition.get("WholeWord") {
                    term_options.whole_word = match value {
                        serde_json::Value::Bool(value) => *value,
                        serde_json::Value::Null => false,
                        _ => {
                            return Err(ConfigError::type_mismatch(
                                format!("FlaggedTerms.{term}.WholeWord"),
                                "boolean",
                            ));
                        }
                    };
                }
                if let Some(value) = definition.get("Suggestions") {
                    term_options.suggestions = (!value.is_null()).then(|| value.clone());
                }
                if definition.contains_key("AllowedRegex") {
                    term_options.allowed_regex = parse_allowed_regex(
                        definition.get("AllowedRegex"),
                        &format!("FlaggedTerms.{term}.AllowedRegex"),
                    )?;
                }
                validate_term_regexes(term, &term_options)?;
                if let Some((_, existing)) = options
                    .flagged_terms
                    .iter_mut()
                    .find(|(existing, _)| existing == term)
                {
                    *existing = term_options;
                } else {
                    options.flagged_terms.push((term.clone(), term_options));
                }
            }
        }

        Ok(options)
    }

    fn to_config_json(&self) -> String {
        let mut object = serde_json::Map::new();
        object.insert(
            "CheckIdentifiers".to_string(),
            serde_json::Value::Bool(self.check_identifiers),
        );
        object.insert(
            "CheckConstants".to_string(),
            serde_json::Value::Bool(self.check_constants),
        );
        object.insert(
            "CheckVariables".to_string(),
            serde_json::Value::Bool(self.check_variables),
        );
        object.insert(
            "CheckStrings".to_string(),
            serde_json::Value::Bool(self.check_strings),
        );
        object.insert(
            "CheckSymbols".to_string(),
            serde_json::Value::Bool(self.check_symbols),
        );
        object.insert(
            "CheckComments".to_string(),
            serde_json::Value::Bool(self.check_comments),
        );
        object.insert(
            "CheckFilepaths".to_string(),
            serde_json::Value::Bool(self.check_filepaths),
        );

        let mut terms = serde_json::Map::new();
        for default_term in ["whitelist", "blacklist", "slave"] {
            let value = self
                .flagged_terms
                .iter()
                .find(|(term, _)| term == default_term)
                .map_or(serde_json::Value::Null, |(_, options)| {
                    term_options_value(default_term, options)
                });
            terms.insert(default_term.to_string(), value);
        }
        for (term, options) in &self.flagged_terms {
            if !["whitelist", "blacklist", "slave"].contains(&term.as_str()) {
                terms.insert(term.clone(), term_options_value(term, options));
            }
        }
        object.insert("FlaggedTerms".to_string(), serde_json::Value::Object(terms));
        serde_json::Value::Object(object).to_string()
    }
}

fn parse_bool_option(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    target: &mut bool,
) -> Result<(), ConfigError> {
    if let Some(value) = object.get(name) {
        *target = value
            .as_bool()
            .ok_or_else(|| ConfigError::type_mismatch(name, "boolean"))?;
    }
    Ok(())
}

fn parse_allowed_regex(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Vec<String>, ConfigError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(pattern) => {
            if pattern.trim().is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![strip_ruby_regex_delimiters(pattern)])
            }
        }
        serde_json::Value::Array(patterns) => patterns
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                if value.is_null() {
                    return None;
                }
                Some(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            ConfigError::type_mismatch(
                                format!("{field}[{index}]"),
                                "string regexp",
                            )
                        })
                        .map(strip_ruby_regex_delimiters),
                )
            })
            .collect(),
        _ => Err(ConfigError::type_mismatch(field, "string or array of strings")),
    }
}

fn strip_ruby_regex_delimiters(source: &str) -> String {
    let Some((closing, flags)) = source
        .strip_prefix('/')
        .and_then(|body| {
            body.char_indices()
                .rev()
                .find(|(index, ch)| *ch == '/' && !is_escaped(body, *index))
                .map(|(index, _)| (index + 1, &body[index + 1..]))
        })
        .filter(|(_, flags)| flags.chars().all(|flag| matches!(flag, 'i' | 'm' | 'x' | 'o' | 'n' | 'e' | 's' | 'u')))
    else {
        return source.to_string();
    };
    let body = &source[1..closing];
    let mut pattern = String::new();
    if flags.contains('m') {
        pattern.push_str("(?s)");
    }
    if flags.contains('x') {
        pattern.push_str("(?x)");
    }
    pattern.push_str(body);
    pattern
}

fn is_escaped(source: &str, index: usize) -> bool {
    source.as_bytes()[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn validate_term_regexes(term: &str, options: &FlaggedTermOptions) -> Result<(), ConfigError> {
    let source = options.regex.as_deref().unwrap_or(term);
    build_regex(source).map_err(|_| {
        ConfigError::type_mismatch(format!("FlaggedTerms.{term}.Regex"), "valid regexp")
    })?;
    for (index, source) in options.allowed_regex.iter().enumerate() {
        build_regex(source).map_err(|_| {
            ConfigError::type_mismatch(
                format!("FlaggedTerms.{term}.AllowedRegex[{index}]"),
                "valid regexp",
            )
        })?;
    }
    Ok(())
}

fn build_regex(source: &str) -> Result<Regex, murphy_plugin_api::regex::Error> {
    RegexBuilder::new(source).case_insensitive(true).build()
}

fn term_options_value(term: &str, options: &FlaggedTermOptions) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    let is_default_term = ["whitelist", "blacklist", "slave"].contains(&term);
    let regex = options
        .regex
        .as_ref()
        .map_or(serde_json::Value::Null, |regex| {
            serde_json::Value::String(regex.clone())
        });
    if is_default_term || options.regex.is_some() {
        object.insert("Regex".to_string(), regex);
    }
    if is_default_term || options.whole_word {
        object.insert(
            "WholeWord".to_string(),
            serde_json::Value::Bool(options.whole_word),
        );
    }
    if let Some(suggestions) = &options.suggestions {
        object.insert("Suggestions".to_string(), suggestions.clone());
    } else if is_default_term {
        object.insert("Suggestions".to_string(), serde_json::Value::Null);
    }
    if !options.allowed_regex.is_empty() || is_default_term {
        let patterns: Vec<_> = options
            .allowed_regex
            .iter()
            .cloned()
            .map(serde_json::Value::String)
            .collect();
        let allowed_regex = match patterns.as_slice() {
            [] => serde_json::Value::Null,
            [one] => one.clone(),
            _ => serde_json::Value::Array(patterns),
        };
        object.insert("AllowedRegex".to_string(), allowed_regex);
    }
    serde_json::Value::Object(object)
}

type CompiledOptionsCache = OnceLock<Mutex<Option<(Options, Arc<CompiledOptions>)>>>;

/// Regex compilation is shared across files with the same config. The cop is
/// file-scoped and may be constructed anew for each investigation, so this
/// process-wide one-entry cache avoids rebuilding the (usually three) regexes
/// for every file. Cop options are uniform for a named rule in normal config.
fn cached_compiled_options(options: &Options) -> Arc<CompiledOptions> {
    static CACHE: CompiledOptionsCache = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_options, compiled)) = cache.as_ref()
        && cached_options == options
    {
        return Arc::clone(compiled);
    }
    let compiled = Arc::new(CompiledOptions::new(options));
    *cache = Some((options.clone(), Arc::clone(&compiled)));
    compiled
}

struct CompiledTerm {
    regex: Regex,
    whole_word: bool,
    suffix: String,
    sole_suggestion: Option<String>,
}

struct CompiledOptions {
    terms: Vec<CompiledTerm>,
    allowed_regex: Option<Regex>,
}

impl CompiledOptions {
    fn new(options: &Options) -> Self {
        let terms = options
            .flagged_terms
            .iter()
            .filter_map(|(term, options)| {
                let source = options.regex.as_deref().unwrap_or(term);
                let regex = build_regex(source).ok()?;
                Some(CompiledTerm {
                    regex,
                    whole_word: options.whole_word && options.regex.is_none(),
                    suffix: suggestions_suffix(options.suggestions.as_ref()),
                    sole_suggestion: sole_suggestion(options.suggestions.as_ref()),
                })
            })
            .collect();
        let allowed_patterns: Vec<_> = options
            .flagged_terms
            .iter()
            .flat_map(|(_, term)| term.allowed_regex.iter().map(String::as_str))
            .collect();
        let allowed_regex = if allowed_patterns.is_empty() {
            None
        } else {
            let source = allowed_patterns
                .iter()
                .map(|pattern| format!("(?:{pattern})"))
                .collect::<Vec<_>>()
                .join("|");
            build_regex(&source).ok()
        };
        Self {
            terms,
            allowed_regex,
        }
    }

    fn scan_for_words(&self, input: &str) -> Vec<WordMatch> {
        if self.terms.is_empty() || input.is_empty() {
            return Vec::new();
        }
        let masked = self.mask_input(input);
        let mut candidates = Vec::new();
        for (term_index, term) in self.terms.iter().enumerate() {
            for matched in term.regex.find_iter(&masked.text) {
                if masked.overlaps_allowed_range(matched.start(), matched.end()) {
                    continue;
                }
                if term.whole_word
                    && !whole_word_match(&masked.text, matched.start(), matched.end())
                {
                    continue;
                }
                let (Some(start), Some(end)) = (
                    masked.source_offset(matched.start()),
                    masked.source_offset(matched.end()),
                ) else {
                    continue;
                };
                candidates.push(WordMatch {
                    start,
                    end,
                    term_index,
                    word: matched.as_str().to_string(),
                });
            }
        }
        select_non_overlapping(candidates)
    }

    fn mask_input(&self, input: &str) -> MaskedInput {
        let Some(regex) = &self.allowed_regex else {
            return MaskedInput {
                text: input.to_string(),
                spans: Vec::new(),
            };
        };
        let mut masked = String::with_capacity(input.len());
        let mut spans = Vec::new();
        let mut cursor = 0;
        for matched in regex.find_iter(input) {
            masked.push_str(&input[cursor..matched.start()]);
            let masked_start = masked.len();
            masked.extend(std::iter::repeat_n('*', matched.as_str().chars().count()));
            let masked_end = masked.len();
            spans.push(MaskedSpan {
                masked_start,
                masked_end,
                source_start: matched.start(),
                source_end: matched.end(),
            });
            cursor = matched.end();
        }
        masked.push_str(&input[cursor..]);
        MaskedInput { text: masked, spans }
    }

    fn find_term(&self, word: &str) -> Option<&CompiledTerm> {
        // Match RuboCop's `find_flagged_term` lookup on the isolated word, while
        // retaining whole-word boundaries for earlier terms that only match a substring.
        self.terms.iter().find(|term| {
            if term.whole_word {
                term.regex.find_iter(word).any(|matched| {
                    whole_word_match(word, matched.start(), matched.end())
                })
            } else {
                term.regex.is_match(word)
            }
        })
    }

    fn message(&self, word: &str, filepath: bool) -> String {
        let suffix = self
            .find_term(word)
            .map_or_else(|| " with another term".to_string(), |term| {
                if term.suffix.is_empty() {
                    " with another term".to_string()
                } else {
                    term.suffix.clone()
                }
            });
        if filepath {
            format!("Consider replacing '{word}' in file path{suffix}.")
        } else {
            format!("Consider replacing '{word}'{suffix}.")
        }
    }

    fn sole_suggestion(&self, word: &str) -> Option<&str> {
        self.find_term(word)
            .and_then(|term| term.sole_suggestion.as_deref())
    }
}

struct MaskedSpan {
    masked_start: usize,
    masked_end: usize,
    source_start: usize,
    source_end: usize,
}

struct MaskedInput {
    text: String,
    spans: Vec<MaskedSpan>,
}

impl MaskedInput {
    fn overlaps_allowed_range(&self, start: usize, end: usize) -> bool {
        self.spans.iter().any(|span| {
            span.masked_start < span.masked_end
                && start < span.masked_end
                && span.masked_start < end
        })
    }

    fn source_offset(&self, masked_offset: usize) -> Option<usize> {
        let mut byte_shrinkage = 0;
        for span in &self.spans {
            if masked_offset <= span.masked_start {
                return Some(masked_offset + byte_shrinkage);
            }
            if masked_offset < span.masked_end {
                return None;
            }
            byte_shrinkage += (span.source_end - span.source_start)
                - (span.masked_end - span.masked_start);
        }
        Some(masked_offset + byte_shrinkage)
    }
}

struct WordMatch {
    start: usize,
    end: usize,
    term_index: usize,
    word: String,
}

fn select_non_overlapping(mut candidates: Vec<WordMatch>) -> Vec<WordMatch> {
    candidates.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.term_index.cmp(&right.term_index))
            .then_with(|| right.end.cmp(&left.end))
    });
    let mut selected = Vec::new();
    let mut cursor = 0;
    let mut index = 0;
    while index < candidates.len() {
        while index < candidates.len() && candidates[index].start < cursor {
            index += 1;
        }
        if index == candidates.len() {
            break;
        }
        let start = candidates[index].start;
        let mut next = index + 1;
        while next < candidates.len() && candidates[next].start == start {
            next += 1;
        }
        let chosen = index;
        if candidates[chosen].end > start {
            cursor = candidates[chosen].end;
            selected.push(WordMatch {
                start,
                end: candidates[chosen].end,
                term_index: candidates[chosen].term_index,
                word: candidates[chosen].word.clone(),
            });
        }
        index = next;
    }
    selected
}

fn whole_word_match(input: &str, start: usize, end: usize) -> bool {
    let before_is_word = input[..start]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    let after_is_word = input[end..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    !before_is_word && !after_is_word
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckKind {
    Identifier,
    Constant,
    Variable,
    Symbol,
    String,
}

impl Options {
    fn enabled(&self, kind: CheckKind) -> bool {
        match kind {
            CheckKind::Identifier => self.check_identifiers,
            CheckKind::Constant => self.check_constants,
            CheckKind::Variable => self.check_variables,
            CheckKind::Symbol => self.check_symbols,
            CheckKind::String => self.check_strings,
        }
    }
}

#[cop(
    name = "Naming/InclusiveLanguage",
    description = "Recommend the use of inclusive language instead of problematic terms.",
    default_severity = "warning",
    default_enabled = false,
    options = Options
)]
impl InclusiveLanguage {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let options = cx.options_or_default::<Options>();
        let compiled = cached_compiled_options(&options);
        if compiled.terms.is_empty() {
            return;
        }

        if options.check_filepaths {
            check_filepath(cx, &compiled);
        }

        let mut ranges = Vec::new();
        let heredoc_ranges = if options.check_strings {
            super::heredoc_delimiter_naming::body_ranges(cx)
        } else {
            Vec::new()
        };
        if options.check_comments {
            ranges.extend(cx.comments().iter().map(|comment| comment.range));
        }
        if options.check_identifiers
            || options.check_constants
            || options.check_variables
            || options.check_symbols
            || options.check_strings
        {
            let root = cx.root();
            for id in cx
                .descendants(root)
                .into_iter()
                .chain(std::iter::once(root))
            {
                collect_node_ranges(id, &options, cx, &heredoc_ranges, &mut ranges);
            }
        }
        ranges.sort_by_key(|range| (range.start, range.end));
        ranges.dedup_by_key(|range| (range.start, range.end));

        for range in ranges {
            check_source_range(range, cx, &compiled);
        }
    }
}

murphy_plugin_api::submit_cop!(InclusiveLanguage);

fn check_filepath(cx: &Cx<'_>, compiled: &CompiledOptions) {
    let words = compiled.scan_for_words(cx.file_path());
    if words.is_empty() {
        return;
    }
    let message = if words.len() == 1 {
        compiled.message(&words[0].word, true)
    } else {
        let joined = words
            .iter()
            .map(|matched| matched.word.as_str())
            .collect::<Vec<_>>()
            .join("', '");
        format!("Consider replacing '{joined}' in file path with other terms.")
    };
    // Filepath-only offense: no source location (murphy-e7bz.41.2). Uses
    // `Range::NO_LOCATION` on the wire so the ABI stays frozen; the host
    // serializes/renders it without a fabricated range or line/column.
    cx.emit_file_offense(&message, None);
}

fn check_source_range(range: Range, cx: &Cx<'_>, compiled: &CompiledOptions) {
    let text = cx.raw_source(range);
    let mut emitted_words = HashSet::new();
    for matched in compiled.scan_for_words(text) {
        // Preserve the match offset from the masked scan. Searching the original
        // source for the word can select an earlier, AllowedRegex-masked match.
        // RuboCop currently has this range bug; Murphy keeps corrections on the
        // actual flagged occurrence instead.
        if !emitted_words.insert(matched.word.clone()) {
            continue;
        }
        let offense_range = Range {
            start: range.start + matched.start as u32,
            end: range.start + matched.end as u32,
        };
        let message = compiled.message(&matched.word, false);
        cx.emit_offense(offense_range, &message, None);
        if let Some(replacement) = compiled.sole_suggestion(&matched.word) {
            cx.emit_edit(offense_range, replacement);
        }
    }
}

fn collect_node_ranges(
    id: NodeId,
    options: &Options,
    cx: &Cx<'_>,
    heredoc_ranges: &[(Range, Option<Range>)],
    out: &mut Vec<Range>,
) {
    match *cx.kind(id) {
        NodeKind::Lvar(_) | NodeKind::Lvasgn { .. } => {
            push_name_range(id, CheckKind::Identifier, options, cx, out);
        }
        NodeKind::Ivar(_) | NodeKind::Ivasgn { .. } | NodeKind::Cvar(_) | NodeKind::Cvasgn { .. }
        | NodeKind::Gvar(_) | NodeKind::Gvasgn { .. } => {
            push_name_range(id, CheckKind::Variable, options, cx, out);
        }
        NodeKind::Const { scope, name } | NodeKind::Casgn { scope, name, .. }
            if options.enabled(CheckKind::Constant) =>
        {
            out.push(scoped_name_range(id, cx.symbol_str(name), scope.get(), cx));
        }
        NodeKind::Send { receiver, method, .. } if options.enabled(CheckKind::Identifier) => {
            out.push(method_name_range(id, cx.symbol_str(method), receiver.get(), cx));
        }
        NodeKind::Csend { receiver, method, .. } if options.enabled(CheckKind::Identifier) => {
            out.push(method_name_range(id, cx.symbol_str(method), Some(receiver), cx));
        }
        NodeKind::Def { receiver, name, .. } if options.enabled(CheckKind::Identifier) => {
            out.push(method_name_range(id, cx.symbol_str(name), receiver.get(), cx));
        }
        NodeKind::Defs { receiver, name, .. } if options.enabled(CheckKind::Identifier) => {
            out.push(method_name_range(id, cx.symbol_str(name), Some(receiver), cx));
        }
        NodeKind::Arg(name) | NodeKind::Restarg(name) | NodeKind::Kwrestarg(name)
        | NodeKind::Blockarg(name)
        | NodeKind::Shadowarg(name) => {
            push_named_range(id, cx.symbol_str(name), CheckKind::Identifier, options, cx, out);
        }
        NodeKind::MatchVar(name) if !is_shorthand_hash_pattern_binding(id, cx) => {
            push_named_range(id, cx.symbol_str(name), CheckKind::Identifier, options, cx, out);
        }
        NodeKind::Optarg { name, .. } => {
            push_named_range(id, cx.symbol_str(name), CheckKind::Identifier, options, cx, out);
        }
        NodeKind::Str(_) if options.enabled(CheckKind::String) => {
            push_string_range(id, cx, heredoc_ranges, out);
        }
        NodeKind::Sym(_) => push_symbol_range(id, options, cx, out),
        _ => {}
    }
}

fn push_string_range(
    id: NodeId,
    cx: &Cx<'_>,
    heredoc_ranges: &[(Range, Option<Range>)],
    out: &mut Vec<Range>,
) {
    let range = cx.range(id);
    let key = (range.start, range.end);
    match heredoc_ranges.binary_search_by_key(&key, |(opener, _)| (opener.start, opener.end)) {
        Ok(index) => {
            if let Some(body) = heredoc_ranges[index].1 {
                out.push(body);
            }
        }
        Err(_) => out.push(range),
    }
}

fn push_symbol_range(id: NodeId, options: &Options, cx: &Cx<'_>, out: &mut Vec<Range>) {
    let Some(kind) = symbol_check_kind(id, cx).filter(|kind| options.enabled(*kind)) else {
        return;
    };
    let range = if kind == CheckKind::String {
        quoted_symbol_content_range(id, cx).unwrap_or_else(|| cx.range(id))
    } else {
        cx.range(id)
    };
    out.push(range);
}

fn symbol_check_kind(id: NodeId, cx: &Cx<'_>) -> Option<CheckKind> {
    let range = cx.range(id);
    let source = cx.raw_source(range);
    if quoted_symbol_content_range(id, cx).is_some() || is_percent_i_symbol(id, cx) {
        return Some(CheckKind::String);
    }
    if is_hash_label_key(id, cx) {
        return None;
    }
    if is_bare_alias_name(id, cx, source) {
        Some(CheckKind::Identifier)
    } else {
        Some(CheckKind::Symbol)
    }
}

fn quoted_symbol_content_range(id: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let range = cx.range(id);
    let source = cx.raw_source(range);
    let trailing_label_colon = if source.ends_with(':') { 1 } else { 0 };
    let source = if trailing_label_colon == 1 {
        source.strip_suffix(':')?
    } else {
        source
    };
    let bytes = source.as_bytes();
    if source.starts_with("%s") {
        let open = *bytes.get(2)?;
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            other => other,
        };
        if bytes.len() < 4 || bytes.last().copied()? != close {
            return None;
        }
        return Some(Range {
            start: range.start + 3,
            end: range.end - trailing_label_colon - 1,
        });
    }
    let (prefix_len, quote) = if let Some(rest) = source.strip_prefix(':') {
        match rest.as_bytes().first().copied()? {
            b'\'' | b'"' => (2, rest.as_bytes()[0]),
            _ => return None,
        }
    } else {
        match bytes.first().copied()? {
            b'\'' | b'"' => (1, bytes[0]),
            _ => return None,
        }
    };
    if bytes.last().copied()? != quote || bytes.len() < prefix_len + 1 {
        return None;
    }
    Some(Range {
        start: range.start + prefix_len as u32,
        end: range.end - trailing_label_colon - 1,
    })
}
fn is_shorthand_hash_pattern_binding(id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(id).get() else {
        return false;
    };
    let NodeKind::Pair { key, value } = *cx.kind(parent) else {
        return false;
    };
    if value != id {
        return false;
    }
    let NodeKind::Sym(key_name) = *cx.kind(key) else {
        return false;
    };
    let NodeKind::MatchVar(value_name) = *cx.kind(id) else {
        return false;
    };
    if key_name != value_name || cx.range(key).start != cx.range(id).start {
        return false;
    }
    let operator = cx.pair_operator_loc(parent);
    operator != Range::ZERO && cx.raw_source(operator) == ":"
}

fn is_percent_i_symbol(id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(id).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Array(_)) {
        return false;
    }
    let source = cx.raw_source(cx.range(parent)).trim_start();
    source.starts_with("%i") || source.starts_with("%I")
}

fn is_hash_label_key(id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(id).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Pair { key, .. } if key == id => {
            let operator = cx.pair_operator_loc(parent);
            operator != Range::ZERO && cx.raw_source(operator) == ":"
        }
        _ => false,
    }
}

fn is_bare_alias_name(id: NodeId, cx: &Cx<'_>, source: &str) -> bool {
    if source.starts_with(':') || quoted_symbol_content_range(id, cx).is_some() {
        return false;
    }
    let Some(parent) = cx.parent(id).get() else {
        return false;
    };
    matches!(*cx.kind(parent), NodeKind::Alias { .. } | NodeKind::Undef(_))
}

fn push_name_range(
    id: NodeId,
    kind: CheckKind,
    options: &Options,
    cx: &Cx<'_>,
    out: &mut Vec<Range>,
) {
    if !options.enabled(kind) {
        return;
    }
    let name = match *cx.kind(id) {
        NodeKind::Lvar(name)
        | NodeKind::Ivar(name)
        | NodeKind::Cvar(name)
        | NodeKind::Gvar(name)
        | NodeKind::Arg(name)
        | NodeKind::Restarg(name)
        | NodeKind::Kwrestarg(name)
        | NodeKind::Blockarg(name)
        | NodeKind::Shadowarg(name) => cx.symbol_str(name),
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. }
        | NodeKind::Optarg { name, .. } => cx.symbol_str(name),
        _ => return,
    };
    push_named_range(id, name, kind, options, cx, out);
}

fn push_named_range(
    id: NodeId,
    name: &str,
    kind: CheckKind,
    options: &Options,
    cx: &Cx<'_>,
    out: &mut Vec<Range>,
) {
    if !options.enabled(kind) || name.is_empty() {
        return;
    }
    let name_loc = cx.node(id).loc.name;
    if name_loc != Range::ZERO && name_loc.end.saturating_sub(name_loc.start) >= name.len() as u32 {
        out.push(Range {
            start: name_loc.start,
            end: name_loc.start + name.len() as u32,
        });
    } else {
        out.push(search_name_range(id, name, None, cx));
    }
}

fn scoped_name_range(id: NodeId, name: &str, scope: Option<NodeId>, cx: &Cx<'_>) -> Range {
    search_name_range(id, name, scope.map(|scope| cx.range(scope).end), cx)
}

fn method_name_range(id: NodeId, name: &str, receiver: Option<NodeId>, cx: &Cx<'_>) -> Range {
    let name_loc = cx.node(id).loc.name;
    if name_loc != Range::ZERO {
        let name_len = name_loc
            .end
            .saturating_sub(name_loc.start)
            .min(name.len() as u32);
        Range {
            start: name_loc.start,
            end: name_loc.start + name_len,
        }
    } else {
        let from = receiver.map(|receiver| cx.range(receiver).end).or_else(|| {
            if matches!(*cx.kind(id), NodeKind::Def { .. }) {
                let keyword = cx.loc(id).keyword();
                (keyword != Range::ZERO).then_some(keyword.end)
            } else {
                None
            }
        });
        search_name_range(id, name, from, cx)
    }
}

fn search_name_range(id: NodeId, name: &str, from: Option<u32>, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let start = from.unwrap_or(expr.start).max(expr.start).min(expr.end);
    let source_range = Range {
        start,
        end: expr.end,
    };
    let source = cx.raw_source(source_range);
    let offset = source.find(name).unwrap_or(0) as u32;
    let name_start = start + offset;
    Range {
        start: name_start,
        end: (name_start + name.len() as u32).min(expr.end),
    }
}

fn suggestions_suffix(suggestions: Option<&serde_json::Value>) -> String {
    let Some(suggestions) = suggestions else {
        return String::new();
    };
    if suggestions.is_null()
        || suggestions.as_str().is_some_and(|value| value.trim().is_empty())
        || suggestions.as_array().is_some_and(Vec::is_empty)
    {
        return String::new();
    }
    let values: Vec<String> = match suggestions {
        serde_json::Value::String(value) => vec![value.clone()],
        serde_json::Value::Array(values) => values.iter().map(suggestion_to_string).collect(),
        _ => vec![suggestion_to_string(suggestions)],
    };
    let quoted: Vec<String> = values.iter().map(|value| format!("'{value}'")).collect();
    let formatted = match quoted.as_slice() {
        [] => return String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} or {second}"),
        _ => {
            let mut prefix = quoted[..quoted.len() - 1].join(", ");
            prefix.push_str(", or ");
            prefix.push_str(quoted.last().expect("nonempty suggestion list"));
            prefix
        }
    };
    format!(" with {formatted}")
}

fn suggestion_to_string(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string)
}

fn sole_suggestion(suggestions: Option<&serde_json::Value>) -> Option<String> {
    match suggestions? {
        // A blank replacement can delete the flagged source, so treat it as no safe correction.
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Array(values) if values.len() == 1 => {
            sole_suggestion(values.first())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{FlaggedTermOptions, InclusiveLanguage, Options, Range};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_options, test};

    fn category_options(identifiers: bool, strings: bool, symbols: bool) -> Options {
        Options {
            check_identifiers: identifiers,
            check_constants: false,
            check_variables: false,
            check_strings: strings,
            check_symbols: symbols,
            check_comments: false,
            check_filepaths: false,
            flagged_terms: Options::default().flagged_terms,
        }
    }

    #[test]
    fn flags_whitelist_in_identifier() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            whitelist_users = []
            ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
        "#});
    }

    #[test]
    fn flags_blacklist_in_constant() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            BlacklistEntry = 1
            ^^^^^^^^^ Consider replacing 'Blacklist' with 'denylist' or 'block'.
        "#});
    }

    #[test]
    fn flags_constants_in_constant_path_compound_assignments() {
        let source = "Config::Blacklist ||= []\nConfig::Blacklist += []\nConfig::Blacklist &&= []\n";
        let offenses = run_cop::<InclusiveLanguage>(source);
        let expected: Vec<_> = source
            .match_indices("Blacklist")
            .map(|(start, name)| Range {
                start: start as u32,
                end: (start + name.len()) as u32,
            })
            .collect();
        assert_eq!(
            offenses.iter().map(|offense| offense.range).collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn flags_whole_word_slave_but_not_substrings() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            slave = 1
            ^^^^^ Consider replacing 'slave' with 'replica', 'secondary', or 'follower'.
        "#});
        test::<InclusiveLanguage>().expect_no_offenses("TeslaVehicle = 1\nslavery = 2\nslave2 = 3\n");
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            slave_node = 1
            ^^^^^ Consider replacing 'slave' with 'replica', 'secondary', or 'follower'.
        "#});
    }

    #[test]
    fn flags_comment_and_symbol_but_not_strings_by_default() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            # remove whitelist
                     ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
        "#});
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            value = :whitelist
                     ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
        "#});
        test::<InclusiveLanguage>().expect_no_offenses("value = \"whitelist\"\n");
    }

    #[test]
    fn symbol_scanning_excludes_labels_and_routes_quoted_symbols_to_strings() {
        let symbols = category_options(false, false, true);
        assert!(run_cop_with_options::<InclusiveLanguage>(
            "value = { whitelist: 1 }\n",
            &symbols,
        )
        .is_empty());
        assert_eq!(
            run_cop_with_options::<InclusiveLanguage>("value = :whitelist\n", &symbols).len(),
            1
        );
        assert!(run_cop_with_options::<InclusiveLanguage>(
            "value = :\"whitelist\"\n",
            &symbols,
        )
        .is_empty());

        let strings = category_options(false, true, false);
        let offenses = run_cop_with_options::<InclusiveLanguage>(
            "value = :\"whitelist\"\n",
            &strings,
        );
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 10, end: 19 });
    }

    #[test]
    fn shorthand_hash_pattern_labels_are_not_identifiers() {
        let identifiers = category_options(true, false, false);
        let shorthand = "case value\nin { whitelist: }\nend\n";
        assert!(run_cop_with_options::<InclusiveLanguage>(shorthand, &identifiers).is_empty());

        let explicit = "case value\nin { whitelist: whitelist }\nend\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(explicit, &identifiers);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start as usize, explicit.rfind("whitelist").unwrap());
    }

    #[test]
    fn percent_i_symbols_follow_string_toggle() {
        let symbols = category_options(false, false, true);
        assert!(run_cop_with_options::<InclusiveLanguage>(
            "value = %i[whitelist]\n",
            &symbols,
        )
        .is_empty());

        let strings = category_options(false, true, false);
        let offenses = run_cop_with_options::<InclusiveLanguage>(
            "value = %i[whitelist]\n",
            &strings,
        );
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 11, end: 20 });
    }

    #[test]
    fn bare_alias_and_undef_names_follow_identifier_switch() {
        let identifiers = category_options(true, false, false);
        let symbols = category_options(false, false, true);
        assert_eq!(
            run_cop_with_options::<InclusiveLanguage>("alias whitelist old\n", &identifiers)
                .len(),
            1
        );
        assert!(run_cop_with_options::<InclusiveLanguage>(
            "alias whitelist old\n",
            &symbols,
        )
        .is_empty());
        assert_eq!(
            run_cop_with_options::<InclusiveLanguage>("undef whitelist\n", &identifiers).len(),
            1
        );
        assert!(run_cop_with_options::<InclusiveLanguage>("undef whitelist\n", &symbols).is_empty());
        assert_eq!(
            run_cop_with_options::<InclusiveLanguage>("undef :whitelist\n", &symbols).len(),
            1
        );
    }

    #[test]
    fn whole_word_boundary_matches_ruby_ascii_word_semantics() {
        let offenses = run_cop::<InclusiveLanguage>("éslave = 1\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 2, end: 7 });
    }

    #[test]
    fn whole_word_term_does_not_claim_another_terms_match() {
        let config = br#"{"FlaggedTerms":{"foo":{"WholeWord":true,"Suggestions":["whole word"]},"custom":{"Regex":"xfoo","Suggestions":["custom"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        let offenses = run_cop_with_options::<InclusiveLanguage>("xfoo\n", &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 0, end: 4 });
        assert_eq!(offenses[0].message, "Consider replacing 'xfoo' with 'custom'.");
    }

    #[test]
    fn explicit_regex_overrides_whole_word_option() {
        let config = br#"{"FlaggedTerms":{"custom":{"Regex":"foo","WholeWord":true,"Suggestions":["regex"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        let offenses = run_cop_with_options::<InclusiveLanguage>("xfoo\n", &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 1, end: 4 });
        assert_eq!(offenses[0].message, "Consider replacing 'foo' with 'regex'.");
    }

    #[test]
    fn term_lookup_matches_rubocop_on_the_isolated_match() {
        let config = br#"{"FlaggedTerms":{"custom":{"Regex":"slave","Suggestions":["sailor"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        let offenses = run_cop_with_options::<InclusiveLanguage>("enslaved = 1\n", &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 2, end: 7 });
        assert_eq!(
            offenses[0].message,
            "Consider replacing 'slave' with 'replica', 'secondary', or 'follower'."
        );
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_no_corrections("enslaved = 1\n");
    }

    #[test]
    fn checks_strings_when_enabled_and_corrects_one_suggestion() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: true,
            check_symbols: false,
            check_comments: false,
            check_filepaths: false,
            flagged_terms: vec![(
                "whitelist".to_string(),
                FlaggedTermOptions {
                    regex: Some(r"white[-_\s]?list".to_string()),
                    suggestions: Some(serde_json::json!(["allowlist"])),
                    ..FlaggedTermOptions::default()
                },
            )],
        };
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_correction(
                indoc! {r#"
                    text = "whitelist"
                            ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist'.
                "#},
                "text = \"allowlist\"\n",
            );
    }

    #[test]
    fn check_strings_scans_heredoc_body_not_delimiters() {
        let options = category_options(false, true, false);
        let delimiter_only = "text = <<~WHITELIST\nclean body\nWHITELIST\n";
        assert!(run_cop_with_options::<InclusiveLanguage>(delimiter_only, &options).is_empty());

        let body_term = "text = <<~END\nwhitelist\nEND\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(body_term, &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].range.start as usize,
            body_term.find("whitelist").expect("body term"),
        );
    }

    #[test]
    fn check_strings_scans_heredoc_body_with_other_quote_in_label() {
        let options = category_options(false, true, false);
        let source = "text = <<\"can't\"\nwhitelist\ncan't\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].range.start as usize,
            source.find("whitelist").expect("heredoc body term"),
        );
    }

    #[test]
    fn heredoc_string_autocorrection_changes_only_the_body() {
        let mut options = category_options(false, true, false);
        let blacklist = options
            .flagged_terms
            .iter_mut()
            .find(|(term, _)| term == "blacklist")
            .expect("default blacklist term");
        blacklist.1.suggestions = Some(serde_json::json!(["denylist"]));
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_correction(
                indoc! {r#"
                    text = <<~BLACKLIST
                    blacklist
                    ^^^^^^^^^ Consider replacing 'blacklist' with 'denylist'.
                    BLACKLIST
                "#},
                "text = <<~BLACKLIST\ndenylist\nBLACKLIST\n",
            );
    }

    #[test]
    fn check_strings_scans_nested_interpolated_heredoc_bodies() {
        let options = category_options(false, true, false);
        let source = "x = <<~OUTER\n  #{<<~INNER}\n    blacklist\n  INNER\n  whitelist\nOUTER\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].range.start as usize, source.find("blacklist").unwrap());
        assert_eq!(offenses[1].range.start as usize, source.find("whitelist").unwrap());
    }

    #[test]
    fn check_strings_scans_multiple_heredoc_bodies() {
        let options = category_options(false, true, false);
        let source = "first, second = [<<~FIRST, <<~SECOND]\n  whitelist\nFIRST\n  blacklist\nSECOND\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].range.start as usize, source.find("whitelist").unwrap());
        assert_eq!(offenses[1].range.start as usize, source.find("blacklist").unwrap());
    }

    #[test]
    fn allowed_regex_masks_phrase_from_comment() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: true,
            check_filepaths: false,
            flagged_terms: vec![(
                "master".to_string(),
                FlaggedTermOptions {
                    allowed_regex: vec![r"master's degree".to_string()],
                    ..FlaggedTermOptions::default()
                },
            )],
        };
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_no_offenses("# master's degree is an allowed phrase\n");
    }

    #[test]
    fn allowed_regex_masks_other_terms_like_rubocop() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: true,
            check_filepaths: false,
            flagged_terms: vec![
                (
                    "master".to_string(),
                    FlaggedTermOptions {
                        allowed_regex: vec![r"master's degree".to_string()],
                        ..FlaggedTermOptions::default()
                    },
                ),
                (
                    "degree".to_string(),
                    FlaggedTermOptions {
                        suggestions: Some(serde_json::json!(["qualification"])),
                        ..FlaggedTermOptions::default()
                    },
                ),
            ],
        };
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_no_offenses("# master's degree\n");
    }

    #[test]
    fn allowed_match_is_not_used_as_offense_or_autocorrect_range() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: true,
            check_filepaths: false,
            flagged_terms: vec![(
                "foo".to_string(),
                FlaggedTermOptions {
                    regex: Some("foo".to_string()),
                    allowed_regex: vec!["foobar".to_string()],
                    suggestions: Some(serde_json::json!(["bar"])),
                    ..FlaggedTermOptions::default()
                },
            )],
        };
        let source = "# foobar foo\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 9, end: 12 });

        let diagnostic = format!(
            "# foobar foo\n{}^^^ Consider replacing 'foo' with 'bar'.\n",
            " ".repeat(9),
        );
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_correction(&diagnostic, "# foobar bar\n");
    }

    #[test]
    fn allowed_unicode_match_does_not_shift_source_range() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: true,
            check_filepaths: false,
            flagged_terms: vec![(
                "foo".to_string(),
                FlaggedTermOptions {
                    regex: Some("foo".to_string()),
                    allowed_regex: vec!["café".to_string()],
                    ..FlaggedTermOptions::default()
                },
            )],
        };
        let source = "# café foo\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start as usize, source.find("foo").unwrap());
    }

    #[test]
    fn blank_allowed_regex_entries_match_rubocop_zero_width_behavior() {
        let config = br#"{"CheckIdentifiers":false,"CheckConstants":false,"CheckVariables":false,"CheckStrings":false,"CheckSymbols":false,"CheckComments":true,"CheckFilepaths":false,"FlaggedTerms":{"master":{"AllowedRegex":["","master's degree"]},"degree":{"Suggestions":["qualification"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        let master = options
            .flagged_terms
            .iter()
            .find(|(term, _)| term == "master")
            .expect("custom master term");
        assert_eq!(master.1.allowed_regex, ["", "master's degree"]);
        let offenses = run_cop_with_options::<InclusiveLanguage>("# master's degree\n", &options);
        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].range, Range { start: 2, end: 8 });
        assert_eq!(offenses[1].range, Range { start: 11, end: 17 });
    }

    #[test]
    fn custom_regex_parses_slash_delimited_yaml_regexp_and_config_roundtrips() {
        let config = br#"{"FlaggedTerms":{"white":{"Regex":"/white[-_\\s]?list/","Suggestions":["allowlist"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        assert_eq!(options.flagged_terms.len(), 4);
        let white = options
            .flagged_terms
            .iter()
            .find(|(term, _)| term == "white")
            .expect("custom term");
        assert_eq!(white.1.regex.as_deref(), Some(r"white[-_\s]?list"));
        for default_term in ["whitelist", "blacklist", "slave"] {
            assert!(options.flagged_terms.iter().any(|(term, _)| term == default_term));
        }
        let encoded = options.to_config_json();
        let decoded = Options::from_config_json(encoded.as_bytes()).expect("roundtrip");
        assert_eq!(decoded, options);
    }

    #[test]
    fn overlapping_custom_regexes_keep_config_insertion_order() {
        let config = br#"{"FlaggedTerms":{"zulu":{"Regex":"foo","Suggestions":["zulu suggestion"]},"alpha":{"Regex":"foobar","Suggestions":["alpha suggestion"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        let offenses = run_cop_with_options::<InclusiveLanguage>("foobar\n", &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 0, end: 3 });
        assert_eq!(
            offenses[0].message,
            "Consider replacing 'foo' with 'zulu suggestion'."
        );
    }

    #[test]
    fn ruby_regexp_modifiers_are_translated_for_rust_regex() {
        assert_eq!(
            super::strip_ruby_regex_delimiters(r"/foo\/bar/i"),
            r"foo\/bar"
        );
        assert_eq!(
            super::strip_ruby_regex_delimiters("/foo.+/m"),
            "(?s)foo.+"
        );
        assert_eq!(
            super::strip_ruby_regex_delimiters("/foo + bar/x"),
            "(?x)foo + bar"
        );
    }

    #[test]
    fn partial_flagged_terms_merge_defaults_and_allow_nil_removal() {
        let config = br#"{"FlaggedTerms":{"whitelist":{"Suggestions":["permit"]},"slave":null,"custom":{}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        assert_eq!(options.flagged_terms.len(), 3);
        assert!(!options.flagged_terms.iter().any(|(term, _)| term == "slave"));
        assert!(options.flagged_terms.iter().any(|(term, _)| term == "blacklist"));
        let whitelist = options
            .flagged_terms
            .iter()
            .find(|(term, _)| term == "whitelist")
            .expect("default term");
        assert_eq!(whitelist.1.regex.as_deref(), Some(r"white[-_\s]?list"));
        assert_eq!(whitelist.1.suggestions, Some(serde_json::json!(["permit"])));
    }

    #[test]
    fn repeated_flagged_words_emit_one_offense_and_edit() {
        let config = br#"{"FlaggedTerms":{"whitelist":{"Suggestions":["allowlist"]}}}"#;
        let options = Options::from_config_json(config).expect("valid option JSON");
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_correction(
                indoc! {r#"
                    whitelist_whitelist
                    ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist'.
                "#},
                "allowlist_whitelist\n",
            );
    }

    #[test]
    fn setter_method_offense_excludes_assignment_suffix() {
        let source = "def whitelist=(value)\nend\n";
        let offenses = run_cop::<InclusiveLanguage>(source);
        let start = source.find("whitelist=").unwrap() as u32;
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].range,
            Range {
                start,
                end: start + "whitelist".len() as u32,
            }
        );
    }

    #[test]
    fn flags_method_names_and_arguments_as_identifiers() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            def whitelist(whitelist)
                ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
                          ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
              whitelist
              ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
            end
        "#});
    }

    #[test]
    fn short_method_name_range_starts_after_def_keyword() {
        let options = Options {
            check_identifiers: true,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: false,
            check_filepaths: false,
            flagged_terms: vec![(
                "f".to_string(),
                FlaggedTermOptions {
                    regex: Some("f".to_string()),
                    suggestions: Some(serde_json::json!(["method"])),
                    ..FlaggedTermOptions::default()
                },
            )],
        };
        let source = "def f; end\n";
        let offenses = run_cop_with_options::<InclusiveLanguage>(source, &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 4, end: 5 });
        test::<InclusiveLanguage>()
            .with_options(&options)
            .expect_correction(
                "def f; end\n    ^ Consider replacing 'f' with 'method'.\n",
                "def method; end\n",
            );
    }

    #[test]
    fn flags_destructured_method_and_block_arguments() {
        let source = "def f((blacklist, value))\nend\nitems.each { |(whitelist, value)| value }\n";
        let offenses = run_cop::<InclusiveLanguage>(source);
        let expected: Vec<_> = ["blacklist", "whitelist"]
            .into_iter()
            .map(|name| {
                let start = source.find(name).unwrap() as u32;
                Range {
                    start,
                    end: start + name.len() as u32,
                }
            })
            .collect();
        assert_eq!(
            offenses.iter().map(|offense| offense.range).collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn flags_block_local_declarations_as_identifiers() {
        let source = "items.each { |; blacklist| nil }\n";
        let offenses = run_cop::<InclusiveLanguage>(source);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start as usize, source.find("blacklist").unwrap());
    }

    #[test]
    fn flags_constants_wrapped_in_shareable_constant_nodes() {
        let source = "# shareable_constant_value: literal\nBlacklist = []\n";
        let offenses = run_cop::<InclusiveLanguage>(source);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start as usize, source.find("Blacklist").unwrap());
    }

    #[test]
    fn keyword_argument_labels_are_not_identifiers() {
        let source = "def method(whitelist:, blacklist: nil)\nend\n";
        assert!(run_cop::<InclusiveLanguage>(source).is_empty());
    }

    #[test]
    fn flags_instance_class_and_global_variables() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            @slave_node = 1
             ^^^^^ Consider replacing 'slave' with 'replica', 'secondary', or 'follower'.
            @@slave_node = 1
              ^^^^^ Consider replacing 'slave' with 'replica', 'secondary', or 'follower'.
            $slave_node = 1
             ^^^^^ Consider replacing 'slave' with 'replica', 'secondary', or 'follower'.
        "#});
    }

    #[test]
    fn flags_pattern_capture_identifiers() {
        test::<InclusiveLanguage>().expect_offense(indoc! {r#"
            case value
            in Foo => whitelist
                      ^^^^^^^^^ Consider replacing 'whitelist' with 'allowlist' or 'permit'.
              :ok
            end
        "#});
    }

    #[test]
    fn default_multiple_suggestions_do_not_autocorrect() {
        test::<InclusiveLanguage>().expect_no_corrections("whitelist_users = []\n");
    }

    #[test]
    fn blank_sole_suggestions_do_not_autocorrect() {
        for suggestion in [
            serde_json::json!(""),
            serde_json::json!("   "),
            serde_json::json!([""]),
        ] {
            let config = serde_json::json!({
                "FlaggedTerms": { "foo": { "Suggestions": suggestion } }
            });
            let encoded = serde_json::to_vec(&config).expect("encode options");
            let options = Options::from_config_json(&encoded).expect("valid option JSON");
            test::<InclusiveLanguage>()
                .with_options(&options)
                .expect_no_corrections("foo = 1\n");
        }
    }

    #[test]
    fn filepath_offense_uses_no_location_range_and_message() {
        let options = Options {
            check_identifiers: false,
            check_constants: false,
            check_variables: false,
            check_strings: false,
            check_symbols: false,
            check_comments: false,
            check_filepaths: true,
            flagged_terms: vec![("t".to_string(), FlaggedTermOptions::default())],
        };
        let offenses = run_cop_with_options::<InclusiveLanguage>("x = 1\n", &options);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range::NO_LOCATION);
        assert!(!offenses[0].has_location());
        assert_eq!(offenses[0].location(), None);
        assert!(offenses[0].message.contains("in file path"));
    }
}
