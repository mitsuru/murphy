//! `Rails/Validation` — prefer new-style `validates` over `validates_*_of`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Validation
//! upstream_version_checked: 2.35.0
//! version_added: "0.9"
//! version_changed: "0.41"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND (12
//!   `validates_*_of` names): bare receiver only, last argument must exist.
//!   Offense on the selector with `Prefer the new style ...` message.
//!   Upstream's `return` inside the `add_offense` autocorrect block aborts
//!   `on_send` (Ruby `return` in block), so non-correctable trailing args
//!   (bare `b` send, `B` const, lvar) register NO offense — ported as a
//!   correctability gate (`is_literal` or splat or `.freeze` send).
//!   Autocorrect: selector -> `validates`; hash last-arg -> `type: {...}`
//!   (braced when needed), array/frozen-array -> `attrs, type: true`
//!   (`%i` percent arrays get `:` prefix), otherwise insert `, type: true`
//!   after last arg. `size` -> `length` for the type. Include path gating
//!   (`**/app/models/**/*.rb`) via pack default.yml.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Validation;

const TYPES: &[&str] = &[
    "acceptance",
    "comparison",
    "confirmation",
    "exclusion",
    "format",
    "inclusion",
    "length",
    "numericality",
    "presence",
    "absence",
    "size",
    "uniqueness",
];

fn method_for_type(ty: &str) -> String {
    format!("validates_{ty}_of")
}

fn validate_type(method: &str) -> Option<String> {
    // `validates_X_of` -> X, with `size` -> `length`.
    let inner = method.strip_prefix("validates_")?.strip_suffix("_of")?;
    if inner == "size" {
        Some("length".to_owned())
    } else {
        Some(inner.to_owned())
    }
}

fn preferred_message(method: &str) -> Option<String> {
    // ALLOWLIST: `validates :column, {p}: value` for each TYPE (size stays size).
    let idx = TYPES.iter().position(|&t| method_for_type(t) == method)?;
    let prefer = format!("validates :column, {}: value", TYPES[idx]);
    Some(format!(
        "Prefer the new style validations `{prefer}` over `{method}`."
    ))
}

#[cop(
    name = "Rails/Validation",
    description = "Use validates :attribute, hash of validations.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Validation {
    #[on_node(
        kind = "send",
        methods = [
            "validates_acceptance_of",
            "validates_comparison_of",
            "validates_confirmation_of",
            "validates_exclusion_of",
            "validates_format_of",
            "validates_inclusion_of",
            "validates_length_of",
            "validates_numericality_of",
            "validates_presence_of",
            "validates_absence_of",
            "validates_size_of",
            "validates_uniqueness_of"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let Some(vtype) = validate_type(&method) else {
        return;
    };
    let Some(msg) = preferred_message(&method) else {
        return;
    };
    let args = cx.call_arguments(node);
    let Some(&last) = args.last() else {
        return;
    };
    // Upstream correctability gate (see module docs): literal, splat, or `.freeze`.
    if !(cx.is_literal(last) || is_splat(cx, last) || is_freeze_send(cx, last)) {
        return;
    }
    let selector = cx.selector(node);
    cx.emit_offense(selector, &msg, None);
    // Selector -> `validates`.
    cx.emit_edit(selector, "validates");
    if matches!(*cx.kind(last), NodeKind::Hash(_)) {
        let hash_src = cx.raw_source(cx.range(last)).to_owned();
        let braced = if hash_has_braces(cx, last) {
            hash_src
        } else {
            format!("{{ {hash_src} }}")
        };
        cx.emit_edit(cx.range(last), &format!("{vtype}: {braced}"));
    } else if matches!(*cx.kind(last), NodeKind::Array(_)) {
        let attrs = array_attributes(cx, last);
        cx.emit_edit(cx.range(last), &format!("{attrs}, {vtype}: true"));
    } else if is_freeze_send(cx, last) {
        // `[...].freeze` — replace the whole freeze send.
        if let NodeKind::Send { receiver, .. } = *cx.kind(last)
            && let Some(arr) = receiver.get()
            && matches!(*cx.kind(arr), NodeKind::Array(_))
        {
            let attrs = array_attributes(cx, arr);
            cx.emit_edit(cx.range(last), &format!("{attrs}, {vtype}: true"));
            return;
        }
        // Fallback: treat like generic (should not happen for array freeze).
        let end = cx.range(last).end;
        cx.emit_edit(Range { start: end, end }, &format!(", {vtype}: true"));
    } else {
        let end = cx.range(last).end;
        cx.emit_edit(Range { start: end, end }, &format!(", {vtype}: true"));
    }
}

fn is_splat(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(*cx.kind(id), NodeKind::Splat(_))
}

fn is_freeze_send(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    cx.method_name(id) == Some("freeze")
}

fn hash_has_braces(cx: &Cx<'_>, hash: NodeId) -> bool {
    let src = cx.raw_source(cx.range(hash));
    src.trim_start().starts_with('{')
}

fn array_attributes(cx: &Cx<'_>, arr: NodeId) -> String {
    let NodeKind::Array(list) = *cx.kind(arr) else {
        return String::new();
    };
    let elems = cx.list(list);
    let percent = cx.is_percent_literal(arr);
    let mut parts = Vec::new();
    for &e in elems {
        if percent {
            // Upstream: `":#{child.source}"` for percent literals.
            let src = cx.raw_source(cx.range(e)).to_owned();
            // If already starts with `:`, keep; else prefix.
            if src.starts_with(':') {
                parts.push(src);
            } else {
                parts.push(format!(":{src}"));
            }
        } else {
            parts.push(cx.raw_source(cx.range(e)).to_owned());
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::Validation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_attribute() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, numericality: true\n",
        );
    }

    #[test]
    fn flags_multi_attributes() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a, :b
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, :b, numericality: true\n",
        );
    }

    #[test]
    fn flags_paren_args() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_presence_of(:full_name, :birth_date)
                ^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, presence: value` over `validates_presence_of`.
            "#},
            "validates(:full_name, :birth_date, presence: true)\n",
        );
    }

    #[test]
    fn flags_array_literal() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_presence_of [:full_name, :birth_date]
                ^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, presence: value` over `validates_presence_of`.
            "#},
            "validates :full_name, :birth_date, presence: true\n",
        );
    }

    #[test]
    fn flags_frozen_array() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_presence_of [:full_name, :birth_date].freeze
                ^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, presence: value` over `validates_presence_of`.
            "#},
            "validates :full_name, :birth_date, presence: true\n",
        );
    }

    #[test]
    fn flags_percent_array() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_presence_of %i[full_name birth_date]
                ^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, presence: value` over `validates_presence_of`.
            "#},
            "validates :full_name, :birth_date, presence: true\n",
        );
    }

    #[test]
    fn flags_frozen_percent_array() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_presence_of %i[full_name birth_date].freeze
                ^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, presence: value` over `validates_presence_of`.
            "#},
            "validates :full_name, :birth_date, presence: true\n",
        );
    }

    #[test]
    fn flags_non_braced_hash() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a, :b, minimum: 1
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, :b, numericality: { minimum: 1 }\n",
        );
    }

    #[test]
    fn flags_braced_hash() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a, :b, { minimum: 1 }
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, :b, numericality: { minimum: 1 }\n",
        );
    }

    #[test]
    fn flags_proc_hash_value() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_comparison_of :a, :b, greater_than: -> { Time.zone.today }
                ^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, comparison: value` over `validates_comparison_of`.
            "#},
            "validates :a, :b, comparison: { greater_than: -> { Time.zone.today } }\n",
        );
    }

    #[test]
    fn flags_splat() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a, *b
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, *b, numericality: true\n",
        );
    }

    #[test]
    fn flags_splat_with_options() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_numericality_of :a, *b, :c, minimum: 1
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, numericality: value` over `validates_numericality_of`.
            "#},
            "validates :a, *b, :c, numericality: { minimum: 1 }\n",
        );
    }

    #[test]
    fn flags_size_as_length_correction() {
        test::<Validation>().expect_correction(
            indoc! {r#"
                validates_size_of :a
                ^^^^^^^^^^^^^^^^^ Prefer the new style validations `validates :column, size: value` over `validates_size_of`.
            "#},
            "validates :a, length: true\n",
        );
    }

    #[test]
    fn no_offense_with_receiver() {
        test::<Validation>().expect_no_offenses("foo.validates_presence_of :a\n");
    }

    #[test]
    fn no_offense_trailing_send() {
        test::<Validation>().expect_no_offenses("validates_numericality_of :a, b\n");
    }

    #[test]
    fn no_offense_trailing_const() {
        test::<Validation>().expect_no_offenses("validates_numericality_of :a, B\n");
    }

    #[test]
    fn no_offense_trailing_lvar() {
        test::<Validation>().expect_no_offenses(indoc! {r#"
            b = { minimum: 1 }
            validates_numericality_of :a, b
        "#});
    }

    #[test]
    fn no_offense_no_args() {
        test::<Validation>().expect_no_offenses("validates_numericality_of\n");
    }
}

murphy_plugin_api::submit_cop!(Validation);
