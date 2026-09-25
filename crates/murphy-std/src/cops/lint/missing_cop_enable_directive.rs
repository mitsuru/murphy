//! `Lint/MissingCopEnableDirective` — require re-enabling disabled cops.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MissingCopEnableDirective
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `CommentConfig#analyze` range tracking with
//!   registry-backed `all`/department expansion (`PACK_COPS`, excluding
//!   `Lint/Syntax` + `Lint/RedundantCopDisableDirective` like
//!   `DirectiveComment`), own-line vs trailing single-line handling,
//!   `todo` as disable, `MaximumRangeSize` (`max - min < max + 2`), config-
//!   disabled exemptions (`Cx::config_disabled_cops`, no wire change), and
//!   `rubocop:push`/`pop` stack handling (bare + `-/+` args, department
//!   expansion, pop closes at `pop-1` and reopens at `pop`). Only `# rubocop:`
//!   directives are considered (`# murphy:` ignored). One offense per disable
//!   comment (first unacceptable expansion, RuboCop dedupe). Short
//!   unqualified names are tracked as-is; department messages use the
//!   expanded prefix.
//! ```

use std::collections::{HashMap, HashSet};

use murphy_plugin_api::{CommentKind, CopOptions, Cx, Range, cop};

#[derive(Default)]
pub struct MissingCopEnableDirective;

#[derive(CopOptions)]
pub struct Options {
    #[option(name = "MaximumRangeSize", default = 2147483647, description = "Maximum disabled range size in lines.")]
    pub max_range_size: i64,
}

#[cop(
    name = "Lint/MissingCopEnableDirective",
    description = "Require rubocop:disable directives to be re-enabled.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MissingCopEnableDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let max_range = cx.options_or_default::<Options>().max_range_size;
        let source = cx.source();

        // Fast path: no rubocop marker at all.
        if !source.contains("rubocop:") {
            return;
        }

        let registry = registry_names();
        let departments = department_set(&registry);
        let all_expansion = all_cops(&registry);
        let config_disabled: HashSet<String> =
            cx.config_disabled_cops().map(str::to_string).collect();

        let items = parse_items(cx, source);
        if items.is_empty() {
            return;
        }

        let sim = run_analysis(&items, source, &registry, &departments, &all_expansion, &config_disabled);

        // One offense per disable site (first unacceptable expansion).
        let mut emitted: HashSet<Range> = HashSet::new();
        for site in &sim.sites {
            for expanded in &site.expanded {
                let Some((s, e)) = find_range_start(&sim.ranges, expanded, site.line) else {
                    continue;
                };
                if acceptable(s, e, max_range, expanded, &config_disabled) {
                    continue;
                }
                // Dedupe same-comment offenses (RuboCop `current_offense_locations`).
                if !emitted.insert(site.comment_range) {
                    break;
                }
                let (shown, kind) = message_parts(expanded, &site.raws, &departments);
                let text = if max_range >= i64::from(i32::MAX) {
                    format!("Re-enable {shown} {kind} with `# rubocop:enable` after disabling it.")
                } else {
                    format!(
                        "Re-enable {shown} {kind} within {max_range} lines after disabling it."
                    )
                };
                cx.emit_offense(site.comment_range, &text, None);
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Registry helpers (PACK_COPS, no ABI change)
// ---------------------------------------------------------------------------

fn registry_names() -> Vec<String> {
    crate::PACK_COPS
        .iter()
        .filter_map(|cop| {
            // Safety: `cop.name` is a `RawSlice` over `'static` rodata.
            let bytes = unsafe { cop.name.as_bytes() };
            std::str::from_utf8(bytes).ok().map(str::to_string)
        })
        .collect()
}

fn department_set(registry: &[String]) -> HashSet<String> {
    registry
        .iter()
        .filter_map(|n| n.split_once('/').map(|(d, _)| d.to_string()))
        .collect()
}

fn all_cops(registry: &[String]) -> Vec<String> {
    registry
        .iter()
        .filter(|n| {
            n.as_str() != "Lint/Syntax" && n.as_str() != "Lint/RedundantCopDisableDirective"
        })
        .cloned()
        .collect()
}

fn dept_cops(dept: &str, registry: &[String]) -> Vec<String> {
    let prefix = format!("{dept}/");
    registry
        .iter()
        .filter(|n| n.starts_with(&prefix))
        .filter(|n| {
            if dept == "Lint" {
                n.as_str() != "Lint/Syntax"
                    && n.as_str() != "Lint/RedundantCopDisableDirective"
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

fn is_department(name: &str, departments: &HashSet<String>) -> bool {
    departments.contains(name)
}

fn expand_raw(
    raw: &str,
    registry: &[String],
    departments: &HashSet<String>,
) -> Vec<String> {
    if is_department(raw, departments) {
        dept_cops(raw, registry)
    } else {
        vec![raw.to_string()]
    }
}

// ---------------------------------------------------------------------------
// Directive parsing (rubocop: only, push/pop included)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum ItemKind {
    Disable,
    Enable,
    Todo,
    Push,
    Pop,
}

#[derive(Debug, Clone)]
struct Item {
    line: u32,
    comment_range: Range,
    kind: ItemKind,
    is_all: bool,
    raws: Vec<String>,
    push_args: Vec<(char, String)>,
    is_own_line: bool,
}

fn parse_items(cx: &Cx<'_>, source: &str) -> Vec<Item> {
    let mut comments: Vec<_> = cx.comments().to_vec();
    comments.sort_by_key(|c| c.range.start);
    let mut out = Vec::new();
    for comment in comments {
        if comment.kind != CommentKind::Inline {
            continue;
        }
        let Ok(text) = std::str::from_utf8(
            source.as_bytes().get(
                comment.range.start as usize..comment.range.end as usize,
            ).unwrap_or_default(),
        ) else {
            continue;
        };
        let Some((kind, tail)) = parse_header(text) else {
            continue;
        };
        let line = line_1indexed(comment.range.start, source);
        let own = is_own_line(comment.range.start, source);
        match kind {
            ItemKind::Push => {
                let args = parse_push_args(&tail);
                out.push(Item {
                    line,
                    comment_range: comment.range,
                    kind: ItemKind::Push,
                    is_all: false,
                    raws: Vec::new(),
                    push_args: args,
                    is_own_line: own,
                });
            }
            ItemKind::Pop => {
                out.push(Item {
                    line,
                    comment_range: comment.range,
                    kind: ItemKind::Pop,
                    is_all: false,
                    raws: Vec::new(),
                    push_args: Vec::new(),
                    is_own_line: own,
                });
            }
            ItemKind::Disable | ItemKind::Enable | ItemKind::Todo => {
                let t = tail.split_once("--").map_or(tail.as_str(), |(a, _)| a).trim();
                if t.is_empty() {
                    continue;
                }
                if t == "all" {
                    out.push(Item {
                        line,
                        comment_range: comment.range,
                        kind,
                        is_all: true,
                        raws: Vec::new(),
                        push_args: Vec::new(),
                        is_own_line: own,
                    });
                } else {
                    let raws: Vec<String> = t
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                    if raws.is_empty() {
                        continue;
                    }
                    out.push(Item {
                        line,
                        comment_range: comment.range,
                        kind,
                        is_all: false,
                        raws,
                        push_args: Vec::new(),
                        is_own_line: own,
                    });
                }
            }
        }
    }
    out
}

fn parse_header(text: &str) -> Option<(ItemKind, String)> {
    let after_hash = text.strip_prefix('#')?.trim_start();
    let rest = after_hash.strip_prefix("rubocop:")?.trim_start();
    let (kw, tail) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, String::new()), |(k, t)| (k, t.to_string()));
    let kind = match kw {
        "disable" => ItemKind::Disable,
        "enable" => ItemKind::Enable,
        "todo" => ItemKind::Todo,
        "push" => ItemKind::Push,
        "pop" => ItemKind::Pop,
        _ => return None,
    };
    Some((kind, tail))
}

fn parse_push_args(tail: &str) -> Vec<(char, String)> {
    let t = tail.split_once("--").map_or(tail, |(a, _)| a).trim();
    if t.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for tok in t.split_whitespace() {
        let mut chars = tok.chars();
        let Some(op) = chars.next() else { continue };
        if op != '+' && op != '-' {
            continue;
        }
        let name: String = chars.collect();
        if name.is_empty() {
            continue;
        }
        out.push((op, name));
    }
    out
}

// ---------------------------------------------------------------------------
// Line helpers (byte-safe, 1-indexed for ranges)
// ---------------------------------------------------------------------------

fn line_1indexed(offset: u32, source: &str) -> u32 {
    let end = (offset as usize).min(source.len());
    let count = source
        .as_bytes()
        .get(..end)
        .unwrap_or_default()
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32;
    count + 1
}

fn line_start(offset: u32, source: &str) -> usize {
    let off = (offset as usize).min(source.len());
    source.as_bytes()[..off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1)
}

fn is_own_line(comment_start: u32, source: &str) -> bool {
    let s = line_start(comment_start, source);
    let e = (comment_start as usize).min(source.len());
    source
        .as_bytes()
        .get(s..e)
        .unwrap_or_default()
        .iter()
        .all(|&b| matches!(b, b' ' | b'\t'))
}

// Kept for the non-char-boundary unit test and finite-range math.
#[allow(dead_code)]
fn line_number(offset: u32, source: &str) -> u32 {
    let end = (offset as usize).min(source.len());
    source
        .as_bytes()
        .get(..end)
        .unwrap_or_default()
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
}

// ---------------------------------------------------------------------------
// Analysis (mirror of RuboCop CommentConfig#analyze)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Analysis {
    ranges: Vec<(u32, Option<u32>)>,
    start: Option<u32>,
}

#[derive(Debug, Clone)]
struct Site {
    line: u32,
    comment_range: Range,
    expanded: Vec<String>,
    raws: Vec<String>,
}

struct Sim {
    ranges: HashMap<String, Vec<(u32, Option<u32>)>>,
    sites: Vec<Site>,
}

fn run_analysis(
    items: &[Item],
    _source: &str,
    registry: &[String],
    departments: &HashSet<String>,
    all_expansion: &[String],
    config_disabled: &HashSet<String>,
) -> Sim {
    let mut analyses: HashMap<String, Analysis> = HashMap::new();
    // Config-disabled seed: start at sentinel 0 (-Inf).
    for name in config_disabled {
        analyses.entry(name.clone()).or_default().start = Some(0);
    }
    let mut line_info: HashMap<u32, (Range, Vec<String>)> = HashMap::new();
    let mut sites: Vec<Site> = Vec::new();
    let mut stack: Vec<HashMap<String, Analysis>> = Vec::new();

    for item in items {
        match item.kind {
            ItemKind::Push => {
                stack.push(analyses.clone());
                // Apply -/+ args at push line.
                let mut push_raws: Vec<String> = Vec::new();
                for (op, name) in &item.push_args {
                    push_raws.push(name.clone());
                    let expanded = expand_raw(name, registry, departments);
                    for cop in expanded {
                        let a = analyses.entry(cop.clone()).or_default();
                        if *op == '-' {
                            if a.start.is_none() {
                                a.start = Some(item.line);
                            }
                        } else if a.start.is_some() {
                            let s = a.start.take().unwrap();
                            a.ranges.push((s, Some(item.line)));
                        }
                    }
                }
                if !item.push_args.is_empty() {
                    // Record push-disable site for - args (for missing-enable check).
                    let disabled_expanded: Vec<String> = item
                        .push_args
                        .iter()
                        .filter(|(op, _)| *op == '-')
                        .flat_map(|(_, n)| expand_raw(n, registry, departments))
                        .collect();
                    if !disabled_expanded.is_empty() {
                        line_info
                            .entry(item.line)
                            .or_insert((item.comment_range, push_raws.clone()));
                        sites.push(Site {
                            line: item.line,
                            comment_range: item.comment_range,
                            expanded: disabled_expanded,
                            raws: push_raws,
                        });
                    }
                }
            }
            ItemKind::Pop => {
                let Some(restore) = stack.pop() else {
                    continue;
                };
                // Close current opens at pop-1, reopen at pop if restore had open.
                let mut keys: HashSet<String> = HashSet::new();
                for k in analyses.keys() {
                    keys.insert(k.clone());
                }
                for k in restore.keys() {
                    keys.insert(k.clone());
                }
                let mut reopened: Vec<String> = Vec::new();
                for key in keys {
                    let cur_start = analyses.get(&key).and_then(|a| a.start);
                    let restore_start = restore.get(&key).and_then(|a| a.start);
                    // Ensure entry exists.
                    let entry = analyses.entry(key.clone()).or_default();
                    if let Some(s) = cur_start {
                        let end = item.line.saturating_sub(1);
                        entry.ranges.push((s, Some(end)));
                        entry.start = None;
                    }
                    if restore_start.is_some() {
                        entry.start = Some(item.line);
                        reopened.push(key);
                    }
                }
                // Restore analyses map shape for cops only in restore? Already handled
                // via union above; cops only in restore with no current start keep
                // their restore start? Actually pop logic above sets start=pop line
                // when restore had open, regardless of current. Good.
                // For cops that were only in restore (not in analyses before pop),
                // they are now in analyses with start=pop (if restore open). Handled.
                if !reopened.is_empty() {
                    reopened.sort();
                    line_info.entry(item.line).or_insert((item.comment_range, Vec::new()));
                    sites.push(Site {
                        line: item.line,
                        comment_range: item.comment_range,
                        expanded: reopened,
                        raws: Vec::new(),
                    });
                }
                // Note: restore map itself is dropped; analyses now holds merged state.
                let _ = restore;
            }
            ItemKind::Disable | ItemKind::Todo => {
                if !item.is_own_line {
                    // Trailing single-line disable: only that line, always acceptable.
                    continue;
                }
                let expanded: Vec<String> = if item.is_all {
                    all_expansion.to_vec()
                } else {
                    item.raws
                        .iter()
                        .flat_map(|r| expand_raw(r, registry, departments))
                        .collect()
                };
                if expanded.is_empty() {
                    continue;
                }
                line_info
                    .entry(item.line)
                    .or_insert((item.comment_range, item.raws.clone()));
                for cop in &expanded {
                    let a = analyses.entry(cop.clone()).or_default();
                    if let Some(s) = a.start {
                        a.ranges.push((s, Some(item.line)));
                    }
                    a.start = Some(item.line);
                }
                sites.push(Site {
                    line: item.line,
                    comment_range: item.comment_range,
                    expanded,
                    raws: item.raws.clone(),
                });
            }
            ItemKind::Enable => {
                if !item.is_own_line {
                    continue;
                }
                let expanded: Vec<String> = if item.is_all {
                    all_expansion.to_vec()
                } else {
                    item.raws
                        .iter()
                        .flat_map(|r| expand_raw(r, registry, departments))
                        .collect()
                };
                for cop in expanded {
                    if let Some(a) = analyses.get_mut(&cop)
                        && let Some(s) = a.start.take()
                    {
                        a.ranges.push((s, Some(item.line)));
                    }
                }
            }
        }
    }

    // Close opens to infinity.
    let mut ranges: HashMap<String, Vec<(u32, Option<u32>)>> = HashMap::new();
    for (cop, mut a) in analyses {
        if let Some(s) = a.start.take() {
            a.ranges.push((s, None));
        }
        if !a.ranges.is_empty() {
            ranges.insert(cop, a.ranges);
        }
    }
    // line_info is only needed for debugging; sites carry comment ranges.
    let _ = line_info;
    Sim { ranges, sites }
}

fn find_range_start(
    ranges: &HashMap<String, Vec<(u32, Option<u32>)>>,
    cop: &str,
    start: u32,
) -> Option<(u32, Option<u32>)> {
    ranges
        .get(cop)
        .and_then(|v| v.iter().find(|(s, _)| *s == start).copied())
}

fn acceptable(
    start: u32,
    end: Option<u32>,
    max_range: i64,
    cop: &str,
    config_disabled: &HashSet<String>,
) -> bool {
    if let Some(e) = end {
        // Finite range: max-min < max+2 (strict, handles Inf max via i64::MAX branch).
        if max_range >= i64::from(i32::MAX) {
            return true;
        }
        let diff = (e as i64) - (start as i64);
        if diff < max_range + 2 {
            return true;
        }
        // Finite but too long: still acceptable if synthetic config start?
        if start == 0 {
            return true;
        }
        return false;
    }
    // Infinite (no enable).
    if start == 0 {
        return true;
    }
    if config_disabled.contains(cop) {
        return true;
    }
    false
}

fn message_parts(
    expanded: &str,
    raws: &[String],
    departments: &HashSet<String>,
) -> (String, &'static str) {
    for raw in raws {
        if is_department(raw, departments) && expanded.starts_with(raw.as_str()) {
            let shown = expanded.split('/').next().unwrap_or(expanded).to_string();
            return (shown, "department");
        }
    }
    // Push/pop reopen sites have empty raws: fall back to cop (matches RuboCop,
    // whose pop comment carries no department).
    (expanded.to_string(), "cop")
}

murphy_plugin_api::submit_cop!(MissingCopEnableDirective);

#[cfg(test)]
mod tests {
    use super::MissingCopEnableDirective;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_cop_disabled_until_eof() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Layout/SpaceAroundOperators
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn accepts_disable_followed_by_enable() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Layout/SpaceAroundOperators
            x =   0
            # rubocop:enable Layout/SpaceAroundOperators
        "#});
    }

    #[test]
    fn flags_department_disabled_until_eof() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Layout
            ^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout department with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn flags_finite_range_exceeded() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_offense(indoc! {r#"
                # rubocop:disable Layout/SpaceAroundOperators
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop within 2 lines after disabling it.
                x =   0
                y = 1
                # Some other code
                # rubocop:enable Layout/SpaceAroundOperators
            "#});
    }

    #[test]
    fn line_number_accepts_non_char_boundary_offsets() {
        assert_eq!(super::line_number(1, "é\n# rubocop:disable Lint/Foo"), 0);
    }

    #[test]
    fn ignores_murphy_disable_directives() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # murphy:disable Lint/Debugger
            debugger
        "#});
    }

    #[test]
    fn accepts_single_line_trailing_disable() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            x =   0 # rubocop:disable Layout/SpaceAroundOperators
            y = 1
        "#});
    }

    #[test]
    fn flags_todo_without_enable() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:todo Layout/SpaceAroundOperators
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn accepts_department_enable_for_specific_disable() {
        // `enable Layout` expands to all Layout cops, closing the specific disable.
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Layout/SpaceAroundOperators
            x =   0
            # rubocop:enable Layout
        "#});
    }

    #[test]
    fn flags_specific_enable_leaves_other_department_cops_open() {
        // Department disable creates many ranges; enabling one specific cop
        // leaves others open, so with infinite max this still flags (one
        // offense at the department comment).
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Layout
            ^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout department with `# rubocop:enable` after disabling it.
            x =   0
            # rubocop:enable Layout/SpaceAroundOperators
        "#});
    }

    #[test]
    fn accepts_department_disable_followed_by_department_enable() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Layout
            x =   0
            # rubocop:enable Layout
        "#});
    }

    #[test]
    fn flags_finite_disable_without_enable() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_offense(indoc! {r#"
                # rubocop:disable Layout/SpaceAroundOperators
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop within 2 lines after disabling it.
                x =   0
            "#});
    }

    #[test]
    fn flags_finite_department_range_exceeded() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_offense(indoc! {r#"
                # rubocop:disable Layout
                ^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout department within 2 lines after disabling it.
                x =   0
                y = 1
                # Some other code
                # rubocop:enable Layout
            "#});
    }

    #[test]
    fn flags_finite_department_without_enable() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_offense(indoc! {r#"
                # rubocop:disable Layout
                ^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout department within 2 lines after disabling it.
                x =   0
            "#});
    }

    #[test]
    fn accepts_finite_department_within_limit() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_no_offenses(indoc! {r#"
                # rubocop:disable Layout
                x =   0
                y = 1
                # rubocop:enable Layout
            "#});
    }

    #[test]
    fn accepts_push_pop_around_disable() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:push
            # rubocop:disable Layout/SpaceAroundOperators
            x =   0
            # rubocop:pop
            y = 1
        "#});
    }

    #[test]
    fn flags_push_without_pop() {
        test::<MissingCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:push
            # rubocop:disable Layout/SpaceAroundOperators
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Re-enable Layout/SpaceAroundOperators cop with `# rubocop:enable` after disabling it.
            x =   0
        "#});
    }

    #[test]
    fn accepts_bare_push_pop_with_disable_inside() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            def test
              # rubocop:push
              # rubocop:disable Style/RescueStandardError
            rescue => e
              print e
            end
            # rubocop:pop
        "#});
    }

    #[test]
    fn accepts_enable_all_after_disable_all() {
        test::<MissingCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable all
            x =   0
            # rubocop:enable all
            y = 1
        "#});
    }

    #[test]
    fn accepts_enable_within_finite_limit() {
        test::<MissingCopEnableDirective>()
            .with_options(&super::Options { max_range_size: 2 })
            .expect_no_offenses(indoc! {r#"
                # rubocop:disable Layout/SpaceAroundOperators
                x =   0
                y = 1
                # rubocop:enable Layout/SpaceAroundOperators
            "#});
    }

    #[test]
    fn config_disabled_exemption_logic() {
        use std::collections::HashSet;
        // Synthetic config-disabled start (0) is always acceptable.
        assert!(super::acceptable(0, None, i64::from(i32::MAX), "Layout/LineLength", &HashSet::new()));
        // Explicit disable of a config-disabled cop until EOF is acceptable.
        let mut set = HashSet::new();
        set.insert("Layout/LineLength".to_string());
        assert!(super::acceptable(5, None, i64::from(i32::MAX), "Layout/LineLength", &set));
        // Same without config seed is an offense.
        assert!(!super::acceptable(5, None, i64::from(i32::MAX), "Layout/LineLength", &HashSet::new()));
        // Finite ranges within limit are acceptable.
        assert!(super::acceptable(1, Some(3), 2, "Layout/SpaceAroundOperators", &HashSet::new()));
        // Finite but too long flags.
        assert!(!super::acceptable(1, Some(5), 2, "Layout/SpaceAroundOperators", &HashSet::new()));
    }
}
