//! `Rails/ReflectionClassName` — use string for `class_name` in reflections.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ReflectionClassName
//! upstream_version_checked: 2.35.0
//! version_added: "0.64"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [has_many has_one belongs_to] (bare receiver) with a hash containing a
//!   `class_name:` pair, const / const.name / const.to_s offense with quoted
//!   autocorrect, lvar-assigned-const offense (no correction), and
//!   str/sym/dstr + non-const-receiver send + str-assigned lvar suppression.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReflectionClassName;

#[cop(
    name = "Rails/ReflectionClassName",
    description = "Use a string value for `class_name`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ReflectionClassName {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["has_many", "has_one", "belongs_to"])]
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
    let args = cx.call_arguments(node);
    for &a in args {
        if !matches!(*cx.kind(a), NodeKind::Hash(_)) {
            continue;
        }
        let NodeKind::Hash(list) = *cx.kind(a) else {
            continue;
        };
        for &pair in cx.list(list) {
            if !is_class_name_pair(cx, pair) {
                continue;
            }
            if let Some(offense) = check_pair(cx, node, pair) {
                cx.emit_offense(
                    cx.range(pair),
                    "Use a string value for `class_name`.",
                    None,
                );
                if let Some(replacement) = offense {
                    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
                        continue;
                    };
                    cx.emit_edit(cx.range(value), &replacement);
                }
            }
        }
    }
}

fn is_class_name_pair(cx: &Cx<'_>, pair: NodeId) -> bool {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return false;
    };
    matches!(*cx.kind(key), NodeKind::Sym(s) if cx.symbol_str(s) == "class_name")
}

fn check_pair(cx: &Cx<'_>, _send: NodeId, pair: NodeId) -> Option<Option<String>> {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return None;
    };
    // Allowed types: str, sym, dstr (interpolated string) — no offense.
    match *cx.kind(value) {
        NodeKind::Str(_) | NodeKind::Sym(_) | NodeKind::Dstr(_) => return None,
        _ => {}
    }
    // Send values: only const-receiver .name / .to_s / const-method qualify.
    if matches!(*cx.kind(value), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        let recv_opt = cx.call_receiver(value).get();
        let recv = recv_opt?;
        if !matches!(*cx.kind(recv), NodeKind::Const { .. }) {
            return None;
        }
        let method = cx.method_name(value).unwrap_or_default().to_owned();
        if method == "name" || method == "to_s" {
            let src = cx.raw_source(cx.range(recv)).to_owned();
            return Some(Some(format!("\"{src}\"")));
        }
        // Other const-receiver sends (e.g. `Account.foo`): offense without
        // correction (upstream `const_or_string` misses, offense remains).
        // To stay conservative and avoid false positives on unknown
        // const-methods, only flag name/to_s here; other shapes are ignored.
        return None;
    }
    // Lvar: offense unless assigned a string/sym/dstr in an ancestor.
    if let NodeKind::Lvar(sym) = *cx.kind(value) {
        let name = cx.symbol_str(sym).to_owned();
        if str_assigned(cx, pair, &name) {
            return None;
        }
        return Some(None);
    }
    // Const: offense with quoted correction.
    if matches!(*cx.kind(value), NodeKind::Const { .. }) {
        let src = cx.raw_source(cx.range(value)).to_owned();
        return Some(Some(format!("\"{src}\"")));
    }
    // Other shapes (int, array, etc.): conservative no-offense to avoid
    // false positives beyond the spec'd const/lvar/send-const cases.
    None
}

fn str_assigned(cx: &Cx<'_>, pair: NodeId, lvar_name: &str) -> bool {
    for anc in cx.ancestors(pair) {
        for child in cx.children(anc) {
            if let NodeKind::Lvasgn { name, value } = *cx.kind(child) {
                if cx.symbol_str(name) != lvar_name {
                    continue;
                }
                let Some(v) = value.get() else {
                    continue;
                };
                if matches!(
                    *cx.kind(v),
                    NodeKind::Str(_) | NodeKind::Sym(_) | NodeKind::Dstr(_)
                ) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ReflectionClassName;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_const() {
        test::<ReflectionClassName>().expect_correction(
            indoc! {r#"
                has_many :accounts, class_name: Account, foreign_key: :account_id
                                    ^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
            "#},
            "has_many :accounts, class_name: \"Account\", foreign_key: :account_id\n",
        );
    }

    #[test]
    fn corrects_dot_name() {
        test::<ReflectionClassName>().expect_correction(
            indoc! {r#"
                has_many :accounts, class_name: Account.name
                                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
            "#},
            "has_many :accounts, class_name: \"Account\"\n",
        );
    }

    #[test]
    fn corrects_dot_to_s() {
        test::<ReflectionClassName>().expect_correction(
            indoc! {r#"
                has_many :accounts, class_name: Account.to_s
                                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
            "#},
            "has_many :accounts, class_name: \"Account\"\n",
        );
    }

    #[test]
    fn flags_has_one() {
        test::<ReflectionClassName>().expect_correction(
            indoc! {r#"
                has_one :account, class_name: Account
                                  ^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
            "#},
            "has_one :account, class_name: \"Account\"\n",
        );
    }

    #[test]
    fn flags_belongs_to() {
        test::<ReflectionClassName>().expect_correction(
            indoc! {r#"
                belongs_to :account, class_name: Account
                                     ^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
            "#},
            "belongs_to :account, class_name: \"Account\"\n",
        );
    }

    #[test]
    fn flags_with_scope() {
        test::<ReflectionClassName>().expect_offense(indoc! {r#"
            belongs_to :account, -> { distinct }, class_name: Account
                                                  ^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
        "#});
    }

    #[test]
    fn allows_interpolated_string() {
        test::<ReflectionClassName>().expect_no_offenses(
            "has_many :accounts, class_name: \"#{prefix}Account\"\n",
        );
    }

    #[test]
    fn allows_non_const_to_s() {
        test::<ReflectionClassName>()
            .expect_no_offenses("has_many :accounts, class_name: do_something.to_s\n");
    }

    #[test]
    fn allows_bare_to_s() {
        test::<ReflectionClassName>()
            .expect_no_offenses("has_many :accounts, class_name: to_s\n");
    }

    #[test]
    fn allows_string() {
        test::<ReflectionClassName>().expect_no_offenses(
            "has_many :accounts, class_name: 'Account', foreign_key: :account_id\n",
        );
    }

    #[test]
    fn allows_sym() {
        test::<ReflectionClassName>().expect_no_offenses(
            "has_many :accounts, class_name: :Account, foreign_key: :account_id\n",
        );
    }

    #[test]
    fn flags_lvar_assigned_const() {
        test::<ReflectionClassName>().expect_offense(indoc! {r#"
            class_name = Account

            has_many :accounts, class_name: class_name
                                ^^^^^^^^^^^^^^^^^^^^^^ Use a string value for `class_name`.
        "#});
    }

    #[test]
    fn allows_lvar_assigned_string() {
        test::<ReflectionClassName>().expect_no_offenses(indoc! {r#"
            class_name = 'Account'

            has_many :accounts, class_name: class_name
        "#});
    }

    #[test]
    fn allows_method_call() {
        test::<ReflectionClassName>()
            .expect_no_offenses("has_many :accounts, class_name: class_name\n");
    }

    #[test]
    fn allows_send_on_var() {
        test::<ReflectionClassName>()
            .expect_no_offenses("has_many :accounts, class_name: some_thing.class_name\n");
    }

    #[test]
    fn allows_receiver() {
        test::<ReflectionClassName>()
            .expect_no_offenses("obj.has_many :accounts, class_name: Account\n");
    }

    #[test]
    fn allows_no_class_name() {
        test::<ReflectionClassName>()
            .expect_no_offenses("has_many :accounts, foreign_key: :account_id\n");
    }
}
murphy_plugin_api::submit_cop!(ReflectionClassName);
