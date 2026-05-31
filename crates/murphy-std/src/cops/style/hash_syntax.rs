//! `Style/HashSyntax` — checks hash literal key syntax, mirroring
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashSyntax
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-90zo
//!   - murphy-o0gk
//! notes: >
//!   EnforcedShorthandSyntax modes 'always' and 'never' are implemented
//!   for detection. Autocorrect for 'never' (expanding {foo:} to {foo: foo})
//!   is safe and implemented. Autocorrect for modes that omit the value
//!   ('always', 'consistent', 'either_consistent' omit direction) is NOT
//!   implemented: when the hash is a bare method argument, omitting the value
//!   changes parse semantics and requires inserting braces (e.g.
//!   foo(x: x) -> foo({x:})). Detecting whether a hash is a bare argument
//!   requires parent-context inspection, which the murphy-plugin-api Cx ABI
//!   does not currently expose (no is_argument_hash / is_braced helper).
//!   Modes 'consistent' and 'either_consistent' flag mixed-shorthand hashes
//!   but do not autocorrect the omit direction.
//!   TargetRubyVersion gating is not applied per-option: Murphy's
//!   minimum_target_ruby_version cop attribute gates the entire cop, not
//!   individual options, so the shorthand check fires regardless of target
//!   Ruby version (a no-op on Ruby < 3.1 since such files won't use {foo:}).
//!   Value-omitted pairs ({foo:}) are detected via the ABI signal that
//!   the value NodeKind is Unknown with value_range.start < key_range.end —
//!   this is an imprecise signal (Unknown is a generic fallback sentinel),
//!   but is the only available indicator without a dedicated AST node.
//! ```
//!
//! RuboCop's same-named cop for the core hash-rocket / Ruby 1.9 styles.
//!
//! ## Matched shapes
//!
//! Dispatches on `Hash` so the cop can make whole-hash decisions for
//! `no_mixed_keys`, `ruby19_no_mixed_keys`, and
//! `UseHashRocketsWithSymbolValues`.
//!
//! ## Known v1 limitations
//!
//! `EnforcedShorthandSyntax` modes `consistent` and `either_consistent`
//! autocorrect is not implemented for the omit direction (call-context ABI gap).

use murphy_plugin_api::NodeList;
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct HashSyntax;

#[derive(CopOptions)]
pub struct HashSyntaxOptions {
    #[option(
        name = "EnforcedStyle",
        default = "ruby19",
        description = "Hash key syntax style."
    )]
    pub enforced_style: HashSyntaxStyle,

    #[option(
        name = "UseHashRocketsWithSymbolValues",
        default = false,
        description = "Prefer hash rockets when any value in the hash is a symbol."
    )]
    pub use_hash_rockets_with_symbol_values: bool,

    #[option(
        name = "PreferHashRocketsForNonAlnumEndingSymbols",
        default = false,
        description = "Keep hash rockets for symbols ending in non-alphanumeric punctuation."
    )]
    pub prefer_hash_rockets_for_non_alnum_ending_symbols: bool,

    #[option(
        name = "EnforcedShorthandSyntax",
        default = "either",
        description = "Ruby 3.1 hash value omission enforcement style."
    )]
    pub enforced_shorthand_syntax: ShorthandSyntax,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashSyntaxStyle {
    #[option(value = "ruby19")]
    Ruby19,
    #[option(value = "hash_rockets")]
    HashRockets,
    #[option(value = "no_mixed_keys")]
    NoMixedKeys,
    #[option(value = "ruby19_no_mixed_keys")]
    Ruby19NoMixedKeys,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShorthandSyntax {
    /// Accept both shorthand and explicit syntax (no offenses). Default.
    #[option(value = "either")]
    Either,
    /// Flag Ruby 3.1 shorthand `{foo:}`, require explicit `{foo: foo}`.
    #[option(value = "never")]
    Never,
    /// Flag explicit `{foo: foo}` when value can be omitted, require `{foo:}`.
    #[option(value = "always")]
    Always,
    /// Require consistent use of shorthand: all-or-none when all omittable.
    #[option(value = "consistent")]
    Consistent,
    /// Accept either form, but require consistency within each hash.
    #[option(value = "either_consistent")]
    EitherConsistent,
}

const MSG_INCLUDE_HASH_VALUE: &str = "Include the hash value.";
const MSG_OMIT_HASH_VALUE: &str = "Omit the hash value.";
const MSG_DO_NOT_MIX_OMIT: &str =
    "Do not mix explicit and implicit hash values. Omit the hash value.";
const MSG_DO_NOT_MIX_EXPLICIT: &str =
    "Do not mix explicit and implicit hash values. Include the hash value.";

#[cop(
    name = "Style/HashSyntax",
    description = "Check hash literal key syntax.",
    default_severity = "warning",
    default_enabled = true,
    options = HashSyntaxOptions,
)]
impl HashSyntax {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Hash(list) = *cx.kind(node) else {
            return;
        };
        let opts = cx.options_or_default::<HashSyntaxOptions>();

        // Shorthand check is orthogonal to the key-syntax check; run it first
        // over all raw pairs (including value-omitted ones that PairInfo skips).
        check_shorthand(cx, list, &opts);

        let pairs: Vec<PairInfo<'_>> = cx
            .list(list)
            .iter()
            .filter_map(|&pair| PairInfo::new(pair, cx, &opts))
            .collect();
        if pairs.is_empty() {
            return;
        }

        let force_hash_rockets = opts.use_hash_rockets_with_symbol_values
            && pairs.iter().any(|pair| pair.value_is_symbol);

        if opts.enforced_style == HashSyntaxStyle::HashRockets || force_hash_rockets {
            for pair in &pairs {
                if pair.style == PairStyle::Colon {
                    emit_hash_rockets(cx, pair, "Use hash rockets syntax.");
                }
            }
            return;
        }

        match opts.enforced_style {
            HashSyntaxStyle::Ruby19 => {
                if pairs.iter().all(|pair| pair.can_use_ruby19) {
                    for pair in &pairs {
                        if pair.style == PairStyle::Rocket {
                            emit_ruby19(cx, pair, "Use the new Ruby 1.9 hash syntax.");
                        }
                    }
                }
            }
            HashSyntaxStyle::HashRockets => unreachable!("handled above"),
            HashSyntaxStyle::NoMixedKeys => check_no_mixed_keys(cx, &pairs),
            HashSyntaxStyle::Ruby19NoMixedKeys => {
                if pairs.iter().all(|pair| pair.can_use_ruby19) {
                    for pair in &pairs {
                        if pair.style == PairStyle::Rocket {
                            emit_ruby19(cx, pair, "Use the new Ruby 1.9 hash syntax.");
                        }
                    }
                } else {
                    check_no_mixed_keys(cx, &pairs);
                }
            }
        }
    }
}

/// Classifies each raw pair for shorthand analysis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShorthandPairKind {
    /// Uses Ruby 3.1 value omission (`{foo:}`).
    Omitted,
    /// Has an explicit value that *could* be omitted (`{foo: foo}`).
    Omittable,
    /// Has an explicit value that *cannot* be omitted (`{foo: bar}`).
    Required,
}

/// Returns true if this pair uses Ruby 3.1 value omission syntax.
///
/// Detected by: value NodeKind is `Unknown` AND value range starts before key
/// range ends (the ABI signal for an implicit/omitted value node from prism).
fn is_value_omitted(pair: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    if !matches!(*cx.kind(value), NodeKind::Unknown) {
        return false;
    }
    let key_range = cx.range(key);
    let value_range = cx.range(value);
    value_range.start < key_range.end
}

/// Returns true if the pair's value can be omitted in Ruby 3.1 shorthand.
///
/// A value is omittable when:
/// - The key is a symbol whose name does NOT end in `!` or `?`.
/// - The value is a bare local-variable read (`lvar`) or an implicit send
///   (no receiver, no args) whose name equals the key symbol name.
fn is_value_omittable(pair: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    let key_name = match *cx.kind(key) {
        NodeKind::Sym(sym) => cx.symbol_str(sym),
        _ => return false,
    };
    // RuboCop: `hash_key_source.end_with?('!', '?')` → require hash value.
    if key_name.ends_with('!') || key_name.ends_with('?') {
        return false;
    }
    match *cx.kind(value) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == key_name,
        NodeKind::Send {
            receiver,
            method,
            args,
        } => receiver.is_none() && cx.list(args).is_empty() && cx.symbol_str(method) == key_name,
        _ => false,
    }
}

/// Classifies a single raw pair for shorthand analysis.
fn classify_shorthand(pair: NodeId, cx: &Cx<'_>) -> ShorthandPairKind {
    if is_value_omitted(pair, cx) {
        ShorthandPairKind::Omitted
    } else if is_value_omittable(pair, cx) {
        ShorthandPairKind::Omittable
    } else {
        ShorthandPairKind::Required
    }
}

fn check_shorthand(cx: &Cx<'_>, list: NodeList, opts: &HashSyntaxOptions) {
    match opts.enforced_shorthand_syntax {
        ShorthandSyntax::Either => {}
        ShorthandSyntax::Never => check_shorthand_never(cx, list),
        ShorthandSyntax::Always => check_shorthand_always(cx, list),
        ShorthandSyntax::Consistent => check_shorthand_consistent(cx, list),
        ShorthandSyntax::EitherConsistent => check_shorthand_either_consistent(cx, list),
    }
}

/// "never" mode: flag every omitted pair and expand it to explicit form.
fn check_shorthand_never(cx: &Cx<'_>, list: NodeList) {
    for &pair in cx.list(list) {
        if !is_value_omitted(pair, cx) {
            continue;
        }
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            continue;
        };
        let offense_range = cx.range(key);
        cx.emit_offense(offense_range, MSG_INCLUDE_HASH_VALUE, None);
        // Autocorrect: expand `foo:` to `foo: foo`.
        // The inserted value must be the symbol name (an identifier), not the
        // raw key source text. For quoted labels like `{ "foo bar": }` or
        // `{ "t o": }`, raw key source would include the quotes — yielding
        // `{ "t o": "t o" }` (string literal) instead of `{ "t o": t_o }`.
        // However, quoted-label symbols whose names are not valid bare
        // identifiers can't be shorthand anyway (Ruby only allows valid local
        // variable names), so in practice the symbol name is always a plain
        // identifier here. Use `cx.symbol_str` to derive the value safely.
        let sym_name = match *cx.kind(key) {
            NodeKind::Sym(sym) => cx.symbol_str(sym),
            _ => continue,
        };
        let key_src = cx.raw_source(offense_range);
        if key_src.ends_with(':') {
            let key_part = key_src.trim_end_matches(':').trim_end();
            cx.emit_edit(offense_range, &format!("{key_part}: {sym_name}"));
        }
    }
}

/// "always" mode: flag every pair whose value can be omitted.
///
/// Detection only — autocorrect for the omit direction is not implemented.
/// See murphy-parity notes for the call-context ABI limitation.
fn check_shorthand_always(cx: &Cx<'_>, list: NodeList) {
    for &pair in cx.list(list) {
        if !is_value_omittable(pair, cx) {
            continue;
        }
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        let offense_range = Range {
            start: cx.range(key).start,
            end: cx.range(value).end,
        };
        cx.emit_offense(offense_range, MSG_OMIT_HASH_VALUE, None);
    }
}

/// "consistent" mode: all-or-none shorthand when all pairs are omittable.
///
/// Autocorrect for the omit direction is not implemented (call-context ABI gap).
fn check_shorthand_consistent(cx: &Cx<'_>, list: NodeList) {
    let pairs = cx.list(list);

    let mut has_omitted = false;
    let mut has_omittable = false;
    let mut has_required = false;
    for &pair in pairs {
        match classify_shorthand(pair, cx) {
            ShorthandPairKind::Omitted => has_omitted = true,
            ShorthandPairKind::Omittable => has_omittable = true,
            ShorthandPairKind::Required => has_required = true,
        }
    }

    if has_omitted && (has_omittable || has_required) {
        // Mixed: some omitted, some not.
        if has_required {
            // Can't omit all → expand the omitted ones.
            for &pair in pairs {
                if classify_shorthand(pair, cx) == ShorthandPairKind::Omitted {
                    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
                        continue;
                    };
                    cx.emit_offense(cx.range(key), MSG_DO_NOT_MIX_EXPLICIT, None);
                }
            }
        } else {
            // All non-omitted are omittable → flag them to omit.
            for &pair in pairs {
                if classify_shorthand(pair, cx) == ShorthandPairKind::Omittable {
                    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                        continue;
                    };
                    cx.emit_offense(
                        Range {
                            start: cx.range(key).start,
                            end: cx.range(value).end,
                        },
                        MSG_DO_NOT_MIX_OMIT,
                        None,
                    );
                }
            }
        }
    } else if !has_omitted && !has_required && has_omittable {
        // All pairs are omittable but none use shorthand: flag them.
        for &pair in pairs {
            let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                continue;
            };
            cx.emit_offense(
                Range {
                    start: cx.range(key).start,
                    end: cx.range(value).end,
                },
                MSG_OMIT_HASH_VALUE,
                None,
            );
        }
    }
}

/// "either_consistent" mode: accept both forms, but flag mixed hashes.
fn check_shorthand_either_consistent(cx: &Cx<'_>, list: NodeList) {
    let pairs = cx.list(list);

    let mut has_omitted = false;
    let mut has_omittable = false;
    let mut has_required = false;
    for &pair in pairs {
        match classify_shorthand(pair, cx) {
            ShorthandPairKind::Omitted => has_omitted = true,
            ShorthandPairKind::Omittable => has_omittable = true,
            ShorthandPairKind::Required => has_required = true,
        }
    }

    if !has_omitted || (!has_omittable && !has_required) {
        return;
    }

    // Mixed explicit and shorthand.
    if has_required {
        // Can't omit all → expand the omitted ones.
        for &pair in pairs {
            if classify_shorthand(pair, cx) == ShorthandPairKind::Omitted {
                let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
                    continue;
                };
                cx.emit_offense(cx.range(key), MSG_DO_NOT_MIX_EXPLICIT, None);
            }
        }
    } else {
        // All non-omitted are omittable → flag them to omit.
        for &pair in pairs {
            if classify_shorthand(pair, cx) == ShorthandPairKind::Omittable {
                let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                    continue;
                };
                cx.emit_offense(
                    Range {
                        start: cx.range(key).start,
                        end: cx.range(value).end,
                    },
                    MSG_DO_NOT_MIX_OMIT,
                    None,
                );
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PairStyle {
    Rocket,
    Colon,
}

struct PairInfo<'a> {
    key_range: Range,
    style: PairStyle,
    key_text: &'a str,
    key_symbol: Option<&'a str>,
    can_use_ruby19: bool,
    value_is_symbol: bool,
    operator_range: Range,
    edit_end: u32,
}

impl<'a> PairInfo<'a> {
    fn new(pair: NodeId, cx: &Cx<'a>, opts: &HashSyntaxOptions) -> Option<Self> {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            return None;
        };
        let key_range = cx.range(key);
        let value_range = cx.range(value);
        if value_range.start < key_range.end {
            // Ruby value omission (`{ foo: }`) translates as a Pair whose key
            // covers `foo:` and value covers the identifier `foo`.
            return None;
        }
        let gap = Range {
            start: key_range.end,
            end: value_range.start,
        };
        let gap_src = cx.raw_source(gap);
        let key_text = cx.raw_source(key_range);
        let (style, operator_range, edit_end) = if let Some(offset) = gap_src.find("=>") {
            let op_start = gap.start + offset as u32;
            let op_end = op_start + 2;
            (
                PairStyle::Rocket,
                Range {
                    start: op_start,
                    end: op_end,
                },
                skip_inline_space(cx, op_end),
            )
        } else if key_text.ends_with(':') && key_range.end > key_range.start {
            let op_start = key_range.end - 1;
            let op_end = key_range.end;
            (
                PairStyle::Colon,
                Range {
                    start: op_start,
                    end: op_end,
                },
                skip_inline_space(cx, op_end),
            )
        } else {
            return None;
        };
        let key_symbol = match *cx.kind(key) {
            NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
            _ => None,
        };
        Some(Self {
            key_range,
            style,
            key_text,
            key_symbol,
            can_use_ruby19: key_symbol
                .map(|sym| acceptable_ruby19_symbol(sym, key_text, opts))
                .unwrap_or(false),
            value_is_symbol: matches!(*cx.kind(value), NodeKind::Sym(_)),
            operator_range,
            edit_end,
        })
    }
}

fn check_no_mixed_keys(cx: &Cx<'_>, pairs: &[PairInfo<'_>]) {
    if !pairs.iter().all(|pair| pair.can_use_ruby19) {
        for pair in pairs {
            if pair.style == PairStyle::Colon {
                emit_hash_rockets(cx, pair, "Don't mix styles in the same hash.");
            }
        }
        return;
    }

    let Some(first_style) = pairs.first().map(|pair| pair.style) else {
        return;
    };
    for pair in pairs {
        if pair.style == first_style {
            continue;
        }
        match first_style {
            PairStyle::Rocket => emit_hash_rockets(cx, pair, "Don't mix styles in the same hash."),
            PairStyle::Colon => emit_ruby19(cx, pair, "Don't mix styles in the same hash."),
        }
    }
}

fn emit_ruby19(cx: &Cx<'_>, pair: &PairInfo<'_>, message: &str) {
    let Some(replacement_key) = ruby19_key(pair) else {
        return;
    };
    cx.emit_offense(
        Range {
            start: pair.key_range.start,
            end: pair.operator_range.end,
        },
        message,
        None,
    );
    cx.emit_edit(
        Range {
            start: pair.key_range.start,
            end: pair.edit_end,
        },
        &format!("{replacement_key}: "),
    );
}

fn emit_hash_rockets(cx: &Cx<'_>, pair: &PairInfo<'_>, message: &str) {
    let Some(replacement_key) = hash_rocket_key(pair) else {
        return;
    };
    cx.emit_offense(
        Range {
            start: pair.key_range.start,
            end: pair.operator_range.end,
        },
        message,
        None,
    );
    cx.emit_edit(
        Range {
            start: pair.key_range.start,
            end: pair.edit_end,
        },
        &format!("{replacement_key} => "),
    );
}

fn hash_rocket_key(pair: &PairInfo<'_>) -> Option<String> {
    if let Some(stripped) = pair.key_text.strip_suffix(':') {
        return Some(format!(":{stripped}"));
    }
    pair.key_symbol.map(|sym| format!(":{sym}"))
}

fn ruby19_key(pair: &PairInfo<'_>) -> Option<String> {
    let key = pair.key_text;
    if let Some(stripped) = key.strip_prefix(':') {
        return Some(stripped.to_string());
    }
    pair.key_symbol.map(str::to_string)
}

fn acceptable_ruby19_symbol(sym: &str, key_text: &str, opts: &HashSyntaxOptions) -> bool {
    if opts.prefer_hash_rockets_for_non_alnum_ending_symbols
        && !sym
            .as_bytes()
            .last()
            .map(|b| b.is_ascii_alphanumeric() || *b == b'\'' || *b == b'"')
            .unwrap_or(false)
    {
        return false;
    }
    if is_bare_ruby19_symbol(sym) {
        return true;
    }
    let quoted = key_text.strip_prefix(':').filter(|s| {
        (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"'))
    });
    quoted.is_some()
}

fn is_bare_ruby19_symbol(sym: &str) -> bool {
    let bytes = sym.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    if !(first == b'_' || first.is_ascii_alphabetic()) {
        return false;
    }
    let body = match rest.last() {
        Some(b'?' | b'!') => &rest[..rest.len() - 1],
        _ => rest,
    };
    body.iter().all(|b| *b == b'_' || b.is_ascii_alphanumeric())
}

fn skip_inline_space(cx: &Cx<'_>, start: u32) -> u32 {
    let src = cx.source().as_bytes();
    let mut idx = start as usize;
    while idx < src.len() && matches!(src[idx], b' ' | b'\t') {
        idx += 1;
    }
    idx as u32
}

#[cfg(test)]
mod tests {
    use super::{HashSyntax, HashSyntaxOptions, HashSyntaxStyle, ShorthandSyntax};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn option_defaults_match_rubocop() {
        let opts = HashSyntaxOptions::default();
        assert_eq!(opts.enforced_style, HashSyntaxStyle::Ruby19);
        assert!(!opts.use_hash_rockets_with_symbol_values);
        assert!(!opts.prefer_hash_rockets_for_non_alnum_ending_symbols);
        assert_eq!(opts.enforced_shorthand_syntax, ShorthandSyntax::Either);
    }

    // --- EnforcedShorthandSyntax tests ---

    #[test]
    fn shorthand_either_accepts_both_explicit_and_shorthand() {
        // default "either" mode: no offense for shorthand or explicit
        test::<HashSyntax>().expect_no_offenses("foo = 1\nx = { foo: }\n");
        test::<HashSyntax>().expect_no_offenses("foo = 1\nx = { foo: foo }\n");
    }

    #[test]
    fn shorthand_never_flags_value_omission() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Never,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    foo = 1
                    x = { foo: }
                          ^^^^ Include the hash value.
                "#},
                "foo = 1\nx = { foo: foo }\n",
            );
    }

    #[test]
    fn shorthand_never_accepts_explicit_syntax() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Never,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("foo = 1\nx = { foo: foo }\n");
    }

    #[test]
    fn shorthand_always_flags_omittable_explicit_value() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Always,
                ..HashSyntaxOptions::default()
            })
            .expect_offense(indoc! {r#"
                    foo = 1
                    x = { foo: foo }
                          ^^^^^^^^ Omit the hash value.
                "#});
    }

    #[test]
    fn shorthand_always_accepts_shorthand() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Always,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("foo = 1\nx = { foo: }\n");
    }

    #[test]
    fn shorthand_always_accepts_non_omittable_value() {
        // value is a different name: cannot omit
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Always,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("x = { foo: bar }\n");
    }

    #[test]
    fn shorthand_always_accepts_predicate_method_keys() {
        // key ending in ? must keep the explicit value (Ruby syntax restriction)
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Always,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("valid = true\nx = { \"valid?\": valid }\n");
    }

    #[test]
    fn shorthand_consistent_flags_all_omittable_when_none_omitted() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Consistent,
                ..HashSyntaxOptions::default()
            })
            .expect_offense(indoc! {r#"
                    foo = 1
                    bar = 2
                    x = { foo: foo, bar: bar }
                          ^^^^^^^^ Omit the hash value.
                                    ^^^^^^^^ Omit the hash value.
                "#});
    }

    #[test]
    fn shorthand_consistent_accepts_mixed_when_some_required() {
        // When some values can't be omitted, the hash is accepted as-is
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::Consistent,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("foo = 1\nx = { foo: foo, bar: baz }\n");
    }

    #[test]
    fn shorthand_either_consistent_flags_mixed_omitted_and_required() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_shorthand_syntax: ShorthandSyntax::EitherConsistent,
                ..HashSyntaxOptions::default()
            })
            .expect_offense(indoc! {r#"
                    foo = 1
                    x = { foo:, bar: baz }
                          ^^^^ Do not mix explicit and implicit hash values. Include the hash value.
                "#});
    }

    // --- Existing key-style tests (unchanged) ---

    #[test]
    fn ruby19_flags_and_corrects_hash_rockets_when_all_keys_can_use_new_syntax() {
        test::<HashSyntax>().expect_correction(
            indoc! {r#"
                x = { :a => 0, :b   =>  2 }
                      ^^^^^ Use the new Ruby 1.9 hash syntax.
                               ^^^^^^^ Use the new Ruby 1.9 hash syntax.
            "#},
            "x = { a: 0, b: 2 }\n",
        );
    }

    #[test]
    fn ruby19_accepts_hash_rockets_when_hash_has_non_symbol_key() {
        test::<HashSyntax>().expect_no_offenses("x = { :a => 0, 'b' => 1 }\n");
    }

    #[test]
    fn ruby19_accepts_keys_that_cannot_use_new_syntax() {
        test::<HashSyntax>().expect_no_offenses("x = { :[] => 0, :a= => 1 }\n");
    }

    #[test]
    fn ruby19_corrects_quoted_symbol_keys() {
        test::<HashSyntax>().expect_correction(
            indoc! {r#"
                x = { :"t o" => 0, :'&&' => 1 }
                      ^^^^^^^^^ Use the new Ruby 1.9 hash syntax.
                                   ^^^^^^^^ Use the new Ruby 1.9 hash syntax.
            "#},
            "x = { \"t o\": 0, '&&': 1 }\n",
        );
    }

    #[test]
    fn ruby19_honors_prefer_hash_rockets_for_non_alnum_ending_symbols() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                prefer_hash_rockets_for_non_alnum_ending_symbols: true,
                ..HashSyntaxOptions::default()
            })
            .expect_no_offenses("x = { :a? => 0, :b! => 1 }\n");
    }

    #[test]
    fn hash_rockets_flags_and_corrects_ruby19_style() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_style: HashSyntaxStyle::HashRockets,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { a: 0, b: 2 }
                          ^^ Use hash rockets syntax.
                                ^^ Use hash rockets syntax.
                "#},
                "x = { :a => 0, :b => 2 }\n",
            );
    }

    #[test]
    fn hash_rockets_corrects_quoted_symbol_keys() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_style: HashSyntaxStyle::HashRockets,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { "t o": 0, '&&': 1 }
                          ^^^^^^ Use hash rockets syntax.
                                    ^^^^^ Use hash rockets syntax.
                "#},
                "x = { :\"t o\" => 0, :'&&' => 1 }\n",
            );
    }

    #[test]
    fn hash_rockets_ignores_colons_in_comments_between_key_and_value() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_style: HashSyntaxStyle::HashRockets,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { a: # note: keep
                          ^^ Use hash rockets syntax.
                      1 }
                "#},
                "x = { :a => # note: keep\n  1 }\n",
            );
    }

    #[test]
    fn ignores_value_omission_without_slicing_gap_backwards() {
        test::<HashSyntax>().expect_no_offenses("foo = 1\nx = { foo: }\n");
    }

    #[test]
    fn no_mixed_keys_flags_second_style_and_corrects_to_first_style() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_style: HashSyntaxStyle::NoMixedKeys,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { :a => 0, b: 1 }
                                   ^^ Don't mix styles in the same hash.
                "#},
                "x = { :a => 0, :b => 1 }\n",
            );
    }

    #[test]
    fn ruby19_no_mixed_keys_flags_mixed_non_symbol_keys() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                enforced_style: HashSyntaxStyle::Ruby19NoMixedKeys,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { a: 0, 'b' => 1 }
                          ^^ Don't mix styles in the same hash.
                "#},
                "x = { :a => 0, 'b' => 1 }\n",
            );
    }

    #[test]
    fn use_hash_rockets_with_symbol_values_forces_hash_rockets_for_whole_hash() {
        test::<HashSyntax>()
            .with_options(&HashSyntaxOptions {
                use_hash_rockets_with_symbol_values: true,
                ..HashSyntaxOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    x = { a: 1, b: :c }
                          ^^ Use hash rockets syntax.
                                ^^ Use hash rockets syntax.
                "#},
                "x = { :a => 1, :b => :c }\n",
            );
    }
}
