//! `Style/HashSyntax` — checks hash literal key syntax, mirroring
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
//! Ruby 3.1 hash value omission (`{foo:}` / `{foo: foo}`) is not enforced
//! yet, so this port intentionally does not expose RuboCop's
//! `EnforcedShorthandSyntax` option. That mode needs additional call-context
//! autocorrection to avoid changing parse semantics for argument hashes.

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
    use super::{HashSyntax, HashSyntaxOptions, HashSyntaxStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn option_defaults_match_rubocop() {
        let opts = HashSyntaxOptions::default();
        assert_eq!(opts.enforced_style, HashSyntaxStyle::Ruby19);
        assert!(!opts.use_hash_rockets_with_symbol_values);
        assert!(!opts.prefer_hash_rockets_for_non_alnum_ending_symbols);
    }

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
