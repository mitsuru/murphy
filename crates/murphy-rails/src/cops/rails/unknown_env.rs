//! `Rails/UnknownEnv` — flag unknown `Rails.env` environments.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/UnknownEnv
//! upstream_version_checked: 2.35.0
//! version_added: "0.51"
//! version_changed: "2.18"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_case`. A zero-arg `Rails.env`
//!   (bare or `::`-prefixed, matching upstream `(send {(const nil? :Rails)
//!   (const (cbase) :Rails)} :env)`) followed by a zero-arg predicate ending
//!   in `?` flags when the stripped name is not in `Environments` (plus
//!   `local` when Rails >= 7.1, via `rails_version_at_least(7, 1)` where
//!   unset means newest). `==`/`===`/`!=` with a single `Str` argument flags
//!   the string when it names no known environment — `local` is never known
//!   there, matching upstream. `case Rails.env` flags unknown `Str`
//!   `when` conditions the same way; non-string conditions never flag.
//!   Offense ranges mirror upstream (predicate selector, string literal).
//!   The `Did you mean?` suffix ports `DidYouMean::SpellChecker#correct`
//!   (Jaro-Winkler + Levenshtein) verbatim, always over `Environments`
//!   without `local`, matching upstream `message`. No autocorrect.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnknownEnv;

#[derive(CopOptions)]
pub struct UnknownEnvOptions {
    #[option(
        name = "Environments",
        default = ["development", "test", "production"],
        description = "Known Rails environment names."
    )]
    pub environments: Vec<String>,
}

#[cop(
    name = "Rails/UnknownEnv",
    description = "Use correct environment name.",
    default_severity = "warning",
    default_enabled = true,
    options = UnknownEnvOptions,
)]
impl UnknownEnv {
    // Mirrors upstream `on_send`.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    // Mirrors upstream `on_case`.
    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check_case(node, cx);
    }
}

fn check_send(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let opts = cx.options_or_default::<UnknownEnvOptions>();
    if check_predicate(cx, &opts, node) {
        return;
    }
    check_equality(cx, &opts, node);
}

/// Upstream `unknown_environment_predicate?`: a zero-arg `Rails.env`
/// receiver plus a zero-arg predicate whose stripped name is unknown.
/// Offense on the predicate selector.
fn check_predicate(cx: &Cx<'_>, opts: &UnknownEnvOptions, node: NodeId) -> bool {
    if !cx.call_arguments(node).is_empty() {
        return false;
    }
    let Some(name) = cx.method_name(node) else {
        return false;
    };
    let Some(stripped) = name.strip_suffix('?') else {
        return false;
    };
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    if !is_rails_env(cx, recv) {
        return false;
    }
    if known_predicate(cx, opts, stripped) {
        return false;
    }
    cx.emit_offense(cx.selector(node), &message(opts, stripped), None);
    true
}

/// Upstream `unknown_environment_equal?`: `==`/`===`/`!=` with a single
/// `Str` argument on either side of a zero-arg `Rails.env`. Offense on the
/// string literal.
fn check_equality(cx: &Cx<'_>, opts: &UnknownEnvOptions, node: NodeId) {
    let Some(method) = cx.method_name(node) else {
        return;
    };
    if method != "==" && method != "===" && method != "!=" {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    // `(send #rails_env? {:== :=== :!=} $(str ...))`.
    if is_rails_env(cx, recv) {
        if let NodeKind::Str(id) = *cx.kind(args[0]) {
            let value = cx.string_str(id).to_owned();
            if unknown_name(opts, &value) {
                cx.emit_offense(cx.range(args[0]), &message(opts, &value), None);
            }
        }
        return;
    }
    // `(send $(str ...) {:== :=== :!=} #rails_env?)`.
    let NodeKind::Str(id) = *cx.kind(recv) else {
        return;
    };
    if !is_rails_env(cx, args[0]) {
        return;
    }
    let value = cx.string_str(id).to_owned();
    if unknown_name(opts, &value) {
        cx.emit_offense(cx.range(recv), &message(opts, &value), None);
    }
}

fn check_case(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Case { subject, whens, .. } = *cx.kind(node) else {
        return;
    };
    let Some(subject) = subject.get() else {
        return;
    };
    if !is_rails_env(cx, subject) {
        return;
    }
    let opts = cx.options_or_default::<UnknownEnvOptions>();
    for &when in cx.list(whens) {
        let NodeKind::When { conds, .. } = *cx.kind(when) else {
            continue;
        };
        for &cond in cx.list(conds) {
            if let NodeKind::Str(id) = *cx.kind(cond) {
                let value = cx.string_str(id).to_owned();
                if unknown_name(&opts, &value) {
                    cx.emit_offense(cx.range(cond), &message(&opts, &value), None);
                }
            }
        }
    }
}

/// Upstream `rails_env?`: `(send {(const nil? :Rails) (const (cbase)
/// :Rails)} :env)` — zero-arg `env` on bare or `::`-prefixed `Rails`.
fn is_rails_env(cx: &Cx<'_>, node: NodeId) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(node) != Some("env") {
        return false;
    }
    if !cx.call_arguments(node).is_empty() {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.const_name(recv).as_deref() == Some("Rails")
}

/// Upstream `unknown_env_predicate?` inverted: the name is known when it is
/// a configured environment, or `local` on Rails >= 7.1.
fn known_predicate(cx: &Cx<'_>, opts: &UnknownEnvOptions, stripped: &str) -> bool {
    if opts.environments.iter().any(|e| e == stripped) {
        return true;
    }
    // Upstream `environments(with_local: true)`: `local` counts when
    // `supports_local?` (`target_rails_version >= 7.1`, unset means newest).
    stripped == "local" && cx.rails_version_at_least(7, 1)
}

/// Upstream `unknown_env_name?`: configured environments only (`local`
/// never counts for `==`/`case`).
fn unknown_name(opts: &UnknownEnvOptions, name: &str) -> bool {
    !opts.environments.iter().any(|e| e == name)
}

fn message(opts: &UnknownEnvOptions, name: &str) -> String {
    let similar = spell_correct(name, &opts.environments);
    if similar.is_empty() {
        format!("Unknown environment `{name}`.")
    } else {
        format!("Unknown environment `{name}`. Did you mean `{}`?", similar.join(", "))
    }
}

// --- `DidYouMean::SpellChecker` port (message parity) ---
//
// Verbatim port of Ruby's `did_you_mean/spell_checker.rb` (`correct`),
// `jaro_winkler.rb` and `levenshtein.rb`, over character counts (Ruby
// `String#length` counts characters). Verified against the real
// `DidYouMean::SpellChecker`: `proudction` -> [`production`],
// `developpment` -> [`development`], and no suggestion for `something`,
// `local`, `include`, `empty`, `foo` or `locall` with the default
// dictionary.

fn normalize(word: &str) -> String {
    word.to_lowercase().replace('@', "")
}

fn spell_correct(input: &str, dictionary: &[String]) -> Vec<String> {
    let normalized: String = normalize(input);
    let len = normalized.chars().count();
    let threshold = if len > 3 { 0.834 } else { 0.77 };

    let mut words: Vec<&String> = dictionary
        .iter()
        .filter(|w| jaro_winkler(&normalize(w), &normalized) >= threshold)
        .collect();
    // Upstream compares the RAW input (`input.to_s == word.to_s`).
    words.retain(|w| *w != input);
    // Upstream sorts ascending by raw-word distance, then reverses.
    words.sort_by(|a, b| {
        jaro_winkler(a, &normalized)
            .partial_cmp(&jaro_winkler(b, &normalized))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    words.reverse();

    // Correct mistypes.
    let max_typos = (len as f64 * 0.25).ceil() as usize;
    let mut corrections: Vec<String> = words
        .iter()
        .filter(|w| levenshtein(&normalize(w), &normalized) <= max_typos)
        .map(|w| (*w).clone())
        .collect();

    // Correct misspells.
    if corrections.is_empty() {
        corrections = words
            .iter()
            .filter(|w| {
                let word = normalize(w);
                let bound = len.min(word.chars().count());
                levenshtein(&word, &normalized) < bound
            })
            .take(1)
            .map(|w| (*w).clone())
            .collect();
    }
    corrections
}

fn jaro(s1: &str, s2: &str) -> f64 {
    let (c1, c2): (Vec<char>, Vec<char>) = {
        let a: Vec<char> = s1.chars().collect();
        let b: Vec<char> = s2.chars().collect();
        if a.len() > b.len() { (b, a) } else { (a, b) }
    };
    let (len1, len2) = (c1.len(), c2.len());
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }
    let range = (len2 / 2).saturating_sub(1);
    let mut flags1 = vec![false; len1];
    let mut flags2 = vec![false; len2];
    let mut matches = 0usize;
    for i in 0..len1 {
        let last = i + range;
        let mut j = i.saturating_sub(range);
        while j <= last && j < len2 {
            if !flags2[j] && c1[i] == c2[j] {
                flags2[j] = true;
                flags1[i] = true;
                matches += 1;
                break;
            }
            j += 1;
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut k = 0usize;
    let mut transpositions = 0usize;
    for i in 0..len1 {
        if flags1[i] {
            let mut j = k;
            while j < len2 && !flags2[j] {
                j += 1;
            }
            // A next match always exists: both sides hold `matches` flags.
            let index = j.min(len2 - 1);
            k = index + 1;
            if c1[i] != c2[index] {
                transpositions += 1;
            }
        }
    }
    let transpositions = transpositions / 2;
    let (m, l1, l2, t) = (
        matches as f64,
        len1 as f64,
        len2 as f64,
        transpositions as f64,
    );
    (m / l1 + m / l2 + (m - t) / m) / 3.0
}

fn jaro_winkler(s1: &str, s2: &str) -> f64 {
    let jaro_distance = jaro(s1, s2);
    if jaro_distance > 0.7 {
        let c2: Vec<char> = s2.chars().collect();
        let mut bonus = 0usize;
        for (i, ch) in s1.chars().enumerate() {
            if i < 4 && i < c2.len() && ch == c2[i] {
                bonus += 1;
            } else {
                break;
            }
        }
        jaro_distance + (bonus as f64 * 0.1 * (1.0 - jaro_distance))
    } else {
        jaro_distance
    }
}

fn levenshtein(s1: &str, s2: &str) -> usize {
    let c1: Vec<char> = s1.chars().collect();
    let c2: Vec<char> = s2.chars().collect();
    let (n, m) = (c1.len(), c2.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d: Vec<usize> = (0..=m).collect();
    for (i, &ch1) in c1.iter().enumerate() {
        let mut prev = i;
        d[0] = i + 1;
        for (j, &ch2) in c2.iter().enumerate() {
            let cost = if ch1 == ch2 { 0 } else { 1 };
            let x = (d[j + 1] + 1).min(prev + 1).min(d[j] + cost);
            d[j] = prev;
            prev = x;
        }
        d[m] = prev;
    }
    d[m]
}

#[cfg(test)]
mod tests {
    use super::{UnknownEnv, UnknownEnvOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // `TargetRailsVersion < 7.1` so `local` stays unknown for predicates
    // (upstream spec default); every flag-test pins this explicitly.
    fn rails70<T: murphy_plugin_api::NodeCop + Default>(
        t: murphy_plugin_api::test_support::Tester<T>,
    ) -> murphy_plugin_api::test_support::Tester<T> {
        t.with_target_rails_version(7, 0)
    }

    #[test]
    fn flags_predicate_typo_with_suggestion() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env.proudction?
                      ^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
        "#});
    }

    #[test]
    fn flags_second_predicate_typo_with_suggestion() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env.developpment?
                      ^^^^^^^^^^^^^ Unknown environment `developpment`. Did you mean `development`?
        "#});
    }

    #[test]
    fn flags_predicate_without_suggestion() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env.something?
                      ^^^^^^^^^^ Unknown environment `something`.
        "#});
    }

    #[test]
    fn flags_local_predicate_below_rails_71() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env.local?
                      ^^^^^^ Unknown environment `local`.
        "#});
    }

    #[test]
    fn flags_cbase_rails_predicate() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            ::Rails.env.proudction?
                        ^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
        "#});
    }

    #[test]
    fn flags_string_method_predicate() {
        // Upstream has no allowlist here: zero-arg `empty?` flags.
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env.empty?
                      ^^^^^^ Unknown environment `empty`.
        "#});
    }

    #[test]
    fn does_not_flag_predicate_with_arguments() {
        // Upstream `(send #rails_env? $...)` requires zero arguments.
        rails70(test::<UnknownEnv>()).expect_no_offenses("Rails.env.include?(\"production\")\n");
    }

    #[test]
    fn does_not_flag_non_predicate() {
        rails70(test::<UnknownEnv>()).expect_no_offenses("Rails.env.foo\n");
    }

    #[test]
    fn does_not_flag_known_predicate() {
        rails70(test::<UnknownEnv>()).expect_no_offenses(indoc! {r#"
            Rails.env.production?
            Rails.env == 'production'
        "#});
    }

    #[test]
    fn flags_equality_typo_with_suggestion() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env == 'proudction'
                         ^^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
        "#});
    }

    #[test]
    fn flags_reversed_equality_typo() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            'developpment' == Rails.env
            ^^^^^^^^^^^^^^ Unknown environment `developpment`. Did you mean `development`?
        "#});
    }

    #[test]
    fn flags_strict_equality_without_suggestion() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            'something' === Rails.env
            ^^^^^^^^^^^ Unknown environment `something`.
        "#});
    }

    #[test]
    fn flags_inequality_typo() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            Rails.env != 'proudction'
                         ^^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
        "#});
    }

    #[test]
    fn does_not_flag_symbol_equality() {
        rails70(test::<UnknownEnv>()).expect_no_offenses("Rails.env == :production\n");
    }

    #[test]
    fn flags_case_when_typo() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            case Rails.env
            when 'proudction'
                 ^^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
              something
            end
        "#});
    }

    #[test]
    fn flags_second_case_condition_only() {
        rails70(test::<UnknownEnv>()).expect_offense(indoc! {r#"
            case Rails.env
            when 'development', 'proudction'
                                ^^^^^^^^^^^^ Unknown environment `proudction`. Did you mean `production`?
              something
            end
        "#});
    }

    #[test]
    fn does_not_flag_non_string_case_condition() {
        rails70(test::<UnknownEnv>()).expect_no_offenses(indoc! {r#"
            case Rails.env
            when proudction
              something
            end
        "#});
    }

    #[test]
    fn does_not_flag_case_without_rails_env_subject() {
        rails70(test::<UnknownEnv>()).expect_no_offenses(indoc! {r#"
            case Rails.env.foo
            when 'proudction'
              something
            end
        "#});
    }

    #[test]
    fn accepts_local_predicate_on_rails_71() {
        test::<UnknownEnv>()
            .with_target_rails_version(7, 1)
            .expect_no_offenses("Rails.env.local?\n");
    }

    #[test]
    fn flags_local_equality_on_rails_71() {
        // `local` never counts for `==`, even on Rails >= 7.1.
        test::<UnknownEnv>()
            .with_target_rails_version(7, 1)
            .expect_offense(indoc! {r#"
                Rails.env == 'local'
                             ^^^^^^^ Unknown environment `local`.
            "#});
    }

    #[test]
    fn flags_local_case_condition_on_rails_71() {
        test::<UnknownEnv>()
            .with_target_rails_version(7, 1)
            .expect_offense(indoc! {r#"
                case Rails.env
                when 'local'
                     ^^^^^^^ Unknown environment `local`.
                  something
                end
            "#});
    }

    #[test]
    fn accepts_custom_environments_option() {
        let opts = UnknownEnvOptions {
            environments: vec!["production".to_string(), "staging".to_string()],
        };
        rails70(test::<UnknownEnv>())
            .with_options(&opts)
            .expect_no_offenses("Rails.env.staging?\n");
        rails70(test::<UnknownEnv>())
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                Rails.env.development?
                          ^^^^^^^^^^^^ Unknown environment `development`.
            "#});
    }

    // --- SpellChecker unit tests (reference: real DidYouMean output) ---

    #[test]
    fn spellcheck_suggests_close_typos() {
        let dict = vec![
            "development".to_string(),
            "test".to_string(),
            "production".to_string(),
        ];
        assert_eq!(super::spell_correct("proudction", &dict), vec!["production"]);
        assert_eq!(super::spell_correct("developpment", &dict), vec!["development"]);
    }

    #[test]
    fn spellcheck_suggests_nothing_for_distant_names() {
        let dict = vec![
            "development".to_string(),
            "test".to_string(),
            "production".to_string(),
        ];
        for name in ["something", "local", "include", "empty", "foo", "locall", "production"] {
            assert_eq!(super::spell_correct(name, &dict), Vec::<String>::new(), "{name}");
        }
    }
}
murphy_plugin_api::submit_cop!(UnknownEnv);

