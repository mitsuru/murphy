//! `Lint/FormatParameterMismatch` — flag obvious format argument count mismatches.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FormatParameterMismatch
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop 1.87 including full FormatString parser parity
//!   (flags, width, precision, named interpolation, dynamic width/precision),
//!   heredoc/splat handling, and mixed-sequence validation.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct FormatParameterMismatch;

#[cop(
    name = "Lint/FormatParameterMismatch",
    description = "Check format field and argument counts.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FormatParameterMismatch {
    #[on_node(kind = "send", methods = ["format", "sprintf", "%"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else { return; };
        let method_name = cx.symbol_str(method);
        let args = cx.list(args);
        match method_name {
            "format" | "sprintf" => {
                // called_on_string? + format_method? parity:
                // bare or Kernel receiver, >1 args, first arg str/dstr.
                let receiver_ok = receiver.get().is_none_or(|recv| matches!(*cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Kernel"));
                if !receiver_ok {
                    return;
                }
                if args.len() <= 1 {
                    return;
                }
                if !is_string_type(args[0], cx) {
                    return;
                }
                let format_node = args[0];
                let passed_args = args.len() - 1;
                let src = cx.raw_source(cx.range(format_node));
                if is_mixed_format(src) {
                    cx.emit_offense(cx.node(node).loc.name, "Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.", None);
                    return;
                }
                // splat_args? parity: any splat after format string skips.
                if args.iter().skip(1).any(|&arg| matches!(cx.kind(arg), NodeKind::Splat(_))) {
                    return;
                }
                // countable_format? parity: heredoc format string is uncountable.
                if cx.raw_source(cx.range(format_node)).starts_with("<<") {
                    return;
                }
                let Some(field_count) = expected_fields_count(src) else { return; };
                // offending_node? zero-fields guard: dstr format with 0 fields never offenses.
                if field_count == 0 && matches!(*cx.kind(format_node), NodeKind::Dstr(_)) {
                    return;
                }
                if field_count != passed_args {
                    let display_method = method_name;
                    cx.emit_offense(
                        cx.node(node).loc.name,
                        &format!("Number of arguments ({passed_args}) to `{display_method}` doesn't match the number of fields ({field_count})."),
                        None,
                    );
                }
            }
            "%" => {
                let Some(recv) = receiver.get() else { return; };
                if !is_string_type(recv, cx) {
                    return;
                }
                if args.len() != 1 {
                    return;
                }
                let rhs = args[0];
                // percent? heredoc guard parity: string receiver + heredoc RHS is not a format call.
                // RuboCop checks first_argument source for `<<`.
                if cx.raw_source(cx.range(rhs)).starts_with("<<") {
                    return;
                }
                let recv_src = cx.raw_source(cx.range(recv));
                if is_mixed_format(recv_src) {
                    cx.emit_offense(cx.node(node).loc.name, "Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.", None);
                    return;
                }
                // countable_percent? parity: only array RHS is countable.
                let NodeKind::Array(items) = *cx.kind(rhs) else { return; };
                let items = cx.list(items);
                let passed = items.len();
                let Some(field_count) = expected_fields_count(recv_src) else { return; };
                // offending_node? zero-fields guard: array RHS with 0 fields never offenses.
                // Covers `"#{foo}" % [1,2]` and `'%' % []`.
                if field_count == 0 {
                    return;
                }
                if field_count != passed {
                    cx.emit_offense(
                        cx.node(node).loc.name,
                        &format!("Number of arguments ({passed}) to `String#%` doesn't match the number of fields ({field_count})."),
                        None,
                    );
                }
            }
            _ => {}
        }
    }
}

fn is_string_type(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_))
}

// --- RuboCop::Cop::Utils::FormatString parity ---

#[derive(Debug, Clone)]
struct FormatSequence {
    source: String,
    name: Option<String>,
    type_char: Option<char>,
}

impl FormatSequence {
    fn is_percent(&self) -> bool {
        self.type_char == Some('%')
    }

    fn arity(&self) -> usize {
        self.source.bytes().filter(|&b| b == b'*').count() + 1
    }

    fn max_digit_dollar_num(&self) -> Option<usize> {
        max_digit_dollar_in(&self.source)
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn parse_digit_dollar_end(s: &[u8], pos: usize) -> Option<usize> {
    let mut i = pos;
    let start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i > start && i < s.len() && s[i] == b'$' {
        Some(i + 1)
    } else {
        None
    }
}

fn collect_flags_splits(s: &[u8], pos: usize, out: &mut Vec<usize>) {
    if out.contains(&pos) {
        return;
    }
    out.push(pos);
    if pos < s.len() && matches!(s[pos], b' ' | b'#' | b'0' | b'+' | b'-') {
        collect_flags_splits(s, pos + 1, out);
    }
    if let Some(np) = parse_digit_dollar_end(s, pos) {
        collect_flags_splits(s, np, out);
    }
}

fn flags_splits(s: &[u8], start: usize) -> Vec<usize> {
    let mut out = Vec::new();
    collect_flags_splits(s, start, &mut out);
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

fn parse_number_end(s: &[u8], pos: usize) -> Option<usize> {
    if pos >= s.len() {
        return None;
    }
    if s[pos] == b'*' {
        let mut p = pos + 1;
        if let Some(np) = parse_digit_dollar_end(s, p) {
            p = np;
        }
        Some(p)
    } else if s[pos] == b'#' && pos + 1 < s.len() && s[pos + 1] == b'{' {
        // INTERPOLATION `#{...}` — minimal to next `}` like Ruby `.*?`.
        let mut p = pos + 2;
        while p < s.len() {
            if s[p] == b'}' {
                return Some(p + 1);
            }
            p += 1;
        }
        None
    } else if s[pos].is_ascii_digit() {
        let mut p = pos;
        while p < s.len() && s[p].is_ascii_digit() {
            p += 1;
        }
        Some(p)
    } else {
        None
    }
}

fn parse_width_opt(s: &[u8], pos: usize) -> (Option<String>, usize) {
    if let Some(end) = parse_number_end(s, pos) {
        let w = std::str::from_utf8(&s[pos..end]).unwrap_or("").to_string();
        (Some(w), end)
    } else {
        (None, pos)
    }
}

fn parse_precision_opt(s: &[u8], pos: usize) -> (Option<String>, usize) {
    if pos >= s.len() || s[pos] != b'.' {
        return (None, pos);
    }
    let mut p = pos + 1;
    if let Some(np) = parse_number_end(s, p) {
        let prec = std::str::from_utf8(&s[p..np]).unwrap_or("").to_string();
        p = np;
        (Some(prec), p)
    } else {
        (Some(String::new()), p)
    }
}

fn parse_name(s: &[u8], pos: usize) -> Option<(String, usize)> {
    if pos >= s.len() || s[pos] != b'<' {
        return None;
    }
    let mut p = pos + 1;
    let start = p;
    while p < s.len() && is_word_byte(s[p]) {
        p += 1;
    }
    if p == start {
        return None;
    }
    if p >= s.len() || s[p] != b'>' {
        return None;
    }
    let name = std::str::from_utf8(&s[start..p]).unwrap_or("").to_string();
    Some((name, p + 1))
}

fn parse_name_opt(s: &[u8], pos: usize) -> (Option<String>, usize) {
    if let Some((n, np)) = parse_name(s, pos) {
        (Some(n), np)
    } else {
        (None, pos)
    }
}

fn parse_template(s: &[u8], pos: usize) -> Option<(String, usize)> {
    if pos >= s.len() || s[pos] != b'{' {
        return None;
    }
    // TEMPLATE_NAME `(?<!\\#)\\{(?<name>\\w+)\\}` — `{` not preceded by `#`.
    if pos > 0 && s[pos - 1] == b'#' {
        return None;
    }
    let mut p = pos + 1;
    let start = p;
    while p < s.len() && is_word_byte(s[p]) {
        p += 1;
    }
    if p == start {
        return None;
    }
    if p >= s.len() || s[p] != b'}' {
        return None;
    }
    let name = std::str::from_utf8(&s[start..p]).unwrap_or("").to_string();
    Some((name, p + 1))
}

fn parse_type(s: &[u8], pos: usize) -> Option<(char, usize)> {
    if pos >= s.len() {
        return None;
    }
    let b = s[pos];
    if matches!(
        b,
        b'b' | b'B'
            | b'd'
            | b'i'
            | b'o'
            | b'u'
            | b'x'
            | b'X'
            | b'e'
            | b'E'
            | b'f'
            | b'g'
            | b'G'
            | b'a'
            | b'A'
            | b'c'
            | b'p'
            | b's'
    ) {
        Some((b as char, pos + 1))
    } else {
        None
    }
}

type ParsedTail = (Option<String>, Option<String>, Option<String>, char, usize);

// WIDTH? PRECISION? NAME? TYPE
fn parse_option_a(
    s: &[u8],
    pos: usize,
) -> Option<ParsedTail> {
    let (w, p1) = parse_width_opt(s, pos);
    let (prec, p2) = parse_precision_opt(s, p1);
    let (n, p3) = parse_name_opt(s, p2);
    let (t, p4) = parse_type(s, p3)?;
    Some((w, prec, n, t, p4))
}

// WIDTH? NAME PRECISION? TYPE (NAME required)
fn parse_option_b(
    s: &[u8],
    pos: usize,
) -> Option<ParsedTail> {
    let (w, p1) = parse_width_opt(s, pos);
    let (n, p2) = parse_name(s, p1).map(|(name, np)| (Some(name), np))?;
    let (prec, p3) = parse_precision_opt(s, p2);
    let (t, p4) = parse_type(s, p3)?;
    Some((w, prec, n, t, p4))
}

// NAME MORE_FLAGS WIDTH? PRECISION? TYPE (NAME required)
// MORE_FLAGS needs backtracking like initial FLAGS (e.g. `#` vs `#{...}` width).
fn parse_option_c(
    s: &[u8],
    pos: usize,
) -> Option<ParsedTail> {
    let (n, p1) = parse_name(s, pos).map(|(name, np)| (Some(name), np))?;
    for p2 in flags_splits(s, p1) {
        let (w, p3) = parse_width_opt(s, p2);
        let (prec, p4) = parse_precision_opt(s, p3);
        if let Some((t, p5)) = parse_type(s, p4) {
            return Some((w, prec, n, t, p5));
        }
    }
    None
}

// WIDTH? PRECISION? TEMPLATE_NAME
fn parse_option_d(
    s: &[u8],
    pos: usize,
) -> Option<(Option<String>, Option<String>, String, usize)> {
    let (w, p1) = parse_width_opt(s, pos);
    let (prec, p2) = parse_precision_opt(s, p1);
    let (n, p3) = parse_template(s, p2)?;
    Some((w, prec, n, p3))
}

fn try_parse_at(s: &[u8], start: usize) -> Option<(FormatSequence, usize)> {
    if s[start] != b'%' {
        return None;
    }
    if start + 1 >= s.len() {
        return None;
    }
    if s[start + 1] == b'%' {
        return Some((
            FormatSequence {
                source: "%%".to_string(),
                name: None,
                type_char: Some('%'),
            },
            start + 2,
        ));
    }
    // FLAGS* needs backtracking: greedy `#` would swallow `#{...}` interpolation.
    // Try longest flags first (Ruby greedy + backtrack parity).
    for flags_end in flags_splits(s, start + 1) {
        if let Some((_, _prec, n, t, end)) = parse_option_a(s, flags_end) {
            let src = std::str::from_utf8(&s[start..end]).unwrap_or("").to_string();
            return Some((
                FormatSequence {
                    source: src,
                    name: n,
                    type_char: Some(t),
                },
                end,
            ));
        }
        if let Some((_, _prec, n, t, end)) = parse_option_b(s, flags_end) {
            let src = std::str::from_utf8(&s[start..end]).unwrap_or("").to_string();
            return Some((
                FormatSequence {
                    source: src,
                    name: n,
                    type_char: Some(t),
                },
                end,
            ));
        }
        if let Some((_, _prec, n, t, end)) = parse_option_c(s, flags_end) {
            let src = std::str::from_utf8(&s[start..end]).unwrap_or("").to_string();
            return Some((
                FormatSequence {
                    source: src,
                    name: n,
                    type_char: Some(t),
                },
                end,
            ));
        }
        if let Some((_, _prec, n, end)) = parse_option_d(s, flags_end) {
            let src = std::str::from_utf8(&s[start..end]).unwrap_or("").to_string();
            return Some((
                FormatSequence {
                    source: src,
                    name: Some(n),
                    type_char: None,
                },
                end,
            ));
        }
    }
    None
}

fn parse_format_sequences(src: &str) -> Vec<FormatSequence> {
    let s = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            if let Some((seq, end)) = try_parse_at(s, i) {
                out.push(seq);
                i = end;
                continue;
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

fn max_digit_dollar_in(source: &str) -> Option<usize> {
    let s = source.as_bytes();
    let mut i = 0;
    let mut max: Option<usize> = None;
    while i < s.len() {
        if s[i].is_ascii_digit() {
            let mut j = i;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            if j < s.len() && s[j] == b'$' {
                if let Ok(n) = source[i..j].parse::<usize>() {
                    max = Some(max.map_or(n, |m: usize| m.max(n)));
                }
                i = j + 1;
                continue;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    max
}

fn is_mixed_format(src: &str) -> bool {
    let seqs = parse_format_sequences(src);
    let mut kinds = std::collections::HashSet::new();
    for seq in seqs.iter().filter(|s| !s.is_percent()) {
        if seq.name.is_some() {
            kinds.insert(0);
        } else if seq.max_digit_dollar_num().is_some() {
            kinds.insert(1);
        } else {
            kinds.insert(2);
        }
        if kinds.len() > 1 {
            return true;
        }
    }
    false
}

fn expected_fields_count(src: &str) -> Option<usize> {
    let seqs = parse_format_sequences(src);
    if seqs.iter().any(|s| s.name.is_some()) {
        return Some(1);
    }
    let max = seqs.iter().filter_map(|s| s.max_digit_dollar_num()).max();
    if let Some(m) = max
        && m != 0
    {
        return Some(m);
    }
    let sum: usize = seqs
        .iter()
        .filter(|s| !s.is_percent())
        .map(|s| s.arity())
        .sum();
    Some(sum)
}

murphy_plugin_api::submit_cop!(FormatParameterMismatch);

#[cfg(test)]
mod tests {
    use super::FormatParameterMismatch;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_format_argument_count_mismatch() {
        test::<FormatParameterMismatch>()
            .expect_offense(indoc! {r##"
                format('A value: %s and another: %i', a_value)
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                format("something", 1)
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (0).
            "##})
            .expect_offense(indoc! {r##"
                format("%s %s", 1, 2, 3)
                ^^^^^^ Number of arguments (3) to `format` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                sprintf("%s %s", 1, 2, 3)
                ^^^^^^^ Number of arguments (3) to `sprintf` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                Kernel.format("%s %s", 1)
                       ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                Kernel.sprintf("%s %s", 1)
                       ^^^^^^^ Number of arguments (1) to `sprintf` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                "%s %s" % [1, 2, 3]
                        ^ Number of arguments (3) to `String#%` doesn't match the number of fields (2).
            "##});
    }

    #[test]
    fn flags_mixed_format_sequence_types() {
        test::<FormatParameterMismatch>()
            .expect_offense(indoc! {r##"
                format('Unnumbered: %s and numbered: %2$s', a, b)
                ^^^^^^ Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.
            "##})
            .expect_offense(indoc! {r##"
                format('%s %2$s', 'foo', 'bar')
                ^^^^^^ Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.
            "##});
    }

    #[test]
    fn accepts_matching_format_and_percent_calls() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("format('A value: %s and another: %i', a, b)\n")
            .expect_no_offenses("'%s %s' % [a, b]\n")
            .expect_no_offenses("'%s %s' % [a, *rest]\n")
            .expect_no_offenses("format(\"#{fmt}\", a)\n")
            .expect_no_offenses("format('%% done')\n")
            .expect_no_offenses("format(\"%s %d %i\", 1, 2, 3)\n")
            .expect_no_offenses("format('%s %s %% %s %%%% %%%%%% %%5B', 1, 2, 3)\n")
            .expect_no_offenses("format(A_CONST, 1, 2, 3)\n")
            .expect_no_offenses("sprintf(\"%020x%+g:% g %%%#20.8x %#.0e\", 1, 2, 3, 4, 5)\n")
            .expect_no_offenses("format(\"%s\")\n")
            .expect_no_offenses("Foo.format(\"%s\", 1)\n");
    }

    #[test]
    fn handles_splat_arguments() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("sprintf(\"%s, %s, %s\", 1, *arr)\n")
            .expect_no_offenses("puts format(\"%s, %s, %s\", 1, 2, 3, 4, *arr)\n")
            .expect_no_offenses("puts sprintf(\"%s, %s, %s\", 1, 2, 3, 4, *arr)\n")
            .expect_no_offenses("sprintf(\"%d%d\", *test)\n")
            .expect_no_offenses("format(\"%d%d\", *test)\n")
            .expect_offense(indoc! {r##"
                puts "%s, %s, %s" % [1, 2, 3, 4, *arr]
                                  ^ Number of arguments (5) to `String#%` doesn't match the number of fields (3).
            "##});
    }

    #[test]
    fn handles_digit_dollar_numbered_formats() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("format('%1$s %2$s', 'foo', 'bar')\n")
            .expect_no_offenses("format('%1$s %1$s', 'foo')\n")
            .expect_offense(indoc! {r##"
                format('%1$s %2$s', 'foo', 'bar', 'baz')
                ^^^^^^ Number of arguments (3) to `format` doesn't match the number of fields (2).
            "##});
    }

    #[test]
    fn handles_dynamic_width_and_precision() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("format(\"%*d\", max_width, id)\n")
            .expect_no_offenses("format(\"%0*x\", max_width, id)\n")
            .expect_no_offenses("format(\"%*d\", 10, 3)\n")
            .expect_no_offenses("format(\"%.*f\", 2, 20.19)\n")
            .expect_no_offenses("format(\"%*.*f\", 10, 3, 20.19)\n")
            .expect_no_offenses("format(\"%*.*f %*.*f\", 10, 2, 20.19, 5, 1, 11.22)\n")
            .expect_no_offenses("format(\"%.d\", 0)\n")
            .expect_no_offenses("format(\"%0.1f%% percent\", 22.5)\n")
            .expect_offense(indoc! {r##"
                format("%*d", id)
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "##});
    }

    #[test]
    fn handles_named_interpolations() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("\"foo %{bar} baz\" % { bar: 42 }\n")
            .expect_no_offenses("format(\"%%%<hex>02X\", hex: 10)\n")
            .expect_no_offenses("format('%<t>s', t: '%d')\n")
            .expect_no_offenses("params = { y: '2015', m: '01', d: '01' }\nputs format('%{y}-%{m}-%{d}', params)\n")
            .expect_no_offenses("params = { y: '2015', m: '01', d: '01' }\nputs format('%<y>d-%<m>d-%<d>d', params)\n")
            .expect_offense(indoc! {r##"
                params = { y: '2015', m: '01', d: '01' }
                puts format('%{y}-%{m}-%{d}', 2015, 1, 1)
                     ^^^^^^ Number of arguments (3) to `format` doesn't match the number of fields (1).
            "##})
            .expect_offense(indoc! {r##"
                params = { y: '2015', m: '01', d: '01' }
                puts format('%<y>d-%<m>d-%<d>d', 2015, 1, 1)
                     ^^^^^^ Number of arguments (3) to `format` doesn't match the number of fields (1).
            "##});
    }

    #[test]
    fn handles_interpolated_format_strings() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("format(\"#{foo} %s\", \"bar\")\n")
            .expect_no_offenses("format(\"#{foo}\", \"bar\", \"baz\")\n")
            .expect_no_offenses("Kernel.format(\"%.#{number_of_decimal_places}f\", num)\n")
            .expect_no_offenses("\"#{foo} %s %s\" % [1, 2]\n")
            .expect_no_offenses("\"#{foo}\" % [1, 2]\n")
            .expect_no_offenses("format(\"%#{padding}s: %s\", prefix, message)\n")
            .expect_no_offenses("sprintf(\"| %-#{key_offset}s | %-#{val_offset}s |\", key, value)\n")
            .expect_no_offenses("format(\"%s\", \"a b c\".gsub(\" \", \"_\"))\n")
            .expect_no_offenses("\"duration: %10.fms\" % 42\n")
            .expect_offense(indoc! {r##"
                format("#{foo} %s %s", "bar")
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                "%s %s" % ["#{foo}", 1, 2]
                        ^ Number of arguments (3) to `String#%` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                format("%s %s", "#{foo}")
                ^^^^^^ Number of arguments (1) to `format` doesn't match the number of fields (2).
            "##})
            .expect_offense(indoc! {r##"
                "#{foo} %s %s" % [1, 2, 3]
                               ^ Number of arguments (3) to `String#%` doesn't match the number of fields (2).
            "##});
    }

    #[test]
    fn ignores_non_literal_and_edge_cases() {
        test::<FormatParameterMismatch>()
            .expect_no_offenses("puts \"%s\" % {\"a\" => 1}\n")
            .expect_no_offenses("puts \"%s\" % CONST\n")
            .expect_no_offenses("puts \"%s %s\" % var\n")
            .expect_no_offenses("puts \"%s %s\" % (\"ab\".chars)\n")
            .expect_no_offenses("'%' % []\n")
            .expect_no_offenses("puts str % [1, 2]\n")
            .expect_no_offenses("var = '%s'\nvar % [foo]\n")
            .expect_no_offenses("CONST = '%s'\nCONST % [foo]\n")
            .expect_no_offenses("format(\"#{foo}\", \"bar\", \"baz\")\n")
            // `<<`-looking string is still countable (not a real heredoc).
            .expect_offense(indoc! {r##"
                format("<< %s bleh", 1, 2)
                ^^^^^^ Number of arguments (2) to `format` doesn't match the number of fields (1).
            "##});
    }
}
