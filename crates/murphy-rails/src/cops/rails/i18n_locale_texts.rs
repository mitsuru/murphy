//! `Rails/I18nLocaleTexts` — move locale texts to locale files.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/I18nLocaleTexts
//! upstream_version_checked: 2.35.0
//! version_added: "2.14"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [validates redirect_to redirect_back redirect_back_or_to []= mail]
//!   with `validation_message` (message: str), `redirect_to_flash`
//!   (notice:/alert: str), `flash_assignment?` (flash / flash.now []= str),
//!   and `mail_subject` (subject: str). Offense is the Str value node.
//!   No autocorrect upstream.
//! ```
//!
//! Enforces use of I18n and locale files instead of locale specific strings.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct I18nLocaleTexts;

#[cop(
    name = "Rails/I18nLocaleTexts",
    description = "Move locale texts to the locale files in the `config/locales` directory.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl I18nLocaleTexts {
    #[on_node(
        kind = "send",
        methods = [
            "validates",
            "redirect_to",
            "redirect_back",
            "redirect_back_or_to",
            "[]=",
            "mail"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        let m = cx.symbol_str(method).to_owned();
        match m.as_str() {
            "validates" => {
                for text in validation_messages(cx, node) {
                    cx.emit_offense(
                        cx.range(text),
                        "Move locale texts to the locale files in the `config/locales` directory.",
                        None,
                    );
                }
            }
            "redirect_to" | "redirect_back" | "redirect_back_or_to" => {
                if let Some(text) = redirect_flash_text(cx, node) {
                    cx.emit_offense(
                        cx.range(text),
                        "Move locale texts to the locale files in the `config/locales` directory.",
                        None,
                    );
                }
            }
            "[]=" => {
                if let Some(text) = flash_assignment_text(cx, node) {
                    cx.emit_offense(
                        cx.range(text),
                        "Move locale texts to the locale files in the `config/locales` directory.",
                        None,
                    );
                }
            }
            "mail" => {
                if let Some(text) = mail_subject_text(cx, node) {
                    cx.emit_offense(
                        cx.range(text),
                        "Move locale texts to the locale files in the `config/locales` directory.",
                        None,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Upstream `validation_message`: `(pair (sym :message) $str)` search under
/// the validates call. Only Str values flag; sym / send do not.
fn validation_messages(cx: &Cx<'_>, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &arg in cx.call_arguments(node) {
        collect_message_strs(cx, arg, &mut out);
    }
    out
}

fn collect_message_strs(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
    match *cx.kind(id) {
        NodeKind::Hash(pairs) => {
            for &p in cx.list(pairs) {
                collect_message_strs(cx, p, out);
            }
        }
        NodeKind::Pair { key, value } => {
            if let NodeKind::Sym(k) = *cx.kind(key)
                && cx.symbol_str(k) == "message"
                && matches!(*cx.kind(value), NodeKind::Str(_))
            {
                out.push(value);
                return;
            }
            // Recurse into nested hashes (e.g. presence: { message: "..." }).
            collect_message_strs(cx, key, out);
            collect_message_strs(cx, value, out);
        }
        NodeKind::Array(list) => {
            for &e in cx.list(list) {
                collect_message_strs(cx, e, out);
            }
        }
        _ => {
            for child in cx.children(id) {
                // Only descend into hash-like structure; avoid descending
                // into arbitrary sends. Children of hash/pair/array already
                // handled; other nodes have no message pairs.
                let _ = child;
            }
        }
    }
}

/// Upstream `redirect_to_flash`: `(pair (sym {:notice :alert}) $str)`
/// search under the call. Returns the last match (upstream `.to_a.last`).
fn redirect_flash_text(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let mut found = None;
    for &arg in cx.call_arguments(node) {
        if let NodeKind::Hash(pairs) = *cx.kind(arg) {
            for &p in cx.list(pairs) {
                let NodeKind::Pair { key, value } = *cx.kind(p) else {
                    continue;
                };
                let NodeKind::Sym(k) = *cx.kind(key) else {
                    continue;
                };
                if matches!(cx.symbol_str(k), "notice" | "alert")
                    && matches!(*cx.kind(value), NodeKind::Str(_))
                {
                    found = Some(value);
                }
            }
        }
    }
    found
}

/// Upstream `flash_assignment?`: `(send {(send nil? :flash) (send (send nil?
/// :flash) :now)} :[]= _ $str)`. Bare `flash` / `flash.now` receivers only.
fn flash_assignment_text(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "[]=" {
        return None;
    }
    let recv = receiver.get()?;
    if !is_flash_or_flash_now(cx, recv) {
        return None;
    }
    let args = cx.call_arguments(node);
    if args.len() != 2 {
        return None;
    }
    let value = args[1];
    if matches!(*cx.kind(value), NodeKind::Str(_)) {
        Some(value)
    } else {
        None
    }
}

fn is_flash_or_flash_now(cx: &Cx<'_>, id: NodeId) -> bool {
    // `(send nil? :flash)`
    if let NodeKind::Send { receiver, method, args } = *cx.kind(id) {
        if cx.symbol_str(method) != "flash" {
            // Maybe `flash.now`?
            if cx.symbol_str(method) == "now"
                && cx.list(args).is_empty()
                && let Some(inner) = receiver.get()
            {
                return is_bare_flash(cx, inner);
            }
            return false;
        }
        if receiver.get().is_some() {
            return false;
        }
        return cx.list(args).is_empty();
    }
    false
}

fn is_bare_flash(cx: &Cx<'_>, id: NodeId) -> bool {
    if let NodeKind::Send { receiver, method, args } = *cx.kind(id) {
        return receiver.get().is_none()
            && cx.symbol_str(method) == "flash"
            && cx.list(args).is_empty();
    }
    false
}

/// Upstream `mail_subject`: `(pair (sym :subject) $str)` search under mail.
/// Returns last match.
fn mail_subject_text(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let mut found = None;
    for &arg in cx.call_arguments(node) {
        if let NodeKind::Hash(pairs) = *cx.kind(arg) {
            for &p in cx.list(pairs) {
                let NodeKind::Pair { key, value } = *cx.kind(p) else {
                    continue;
                };
                let NodeKind::Sym(k) = *cx.kind(key) else {
                    continue;
                };
                if cx.symbol_str(k) == "subject"
                    && matches!(*cx.kind(value), NodeKind::Str(_))
                {
                    found = Some(value);
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::I18nLocaleTexts;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_validates_message() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            validates :email, presence: { message: "must be present" },
                                                   ^^^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
              format: { with: /@/, message: "not an email" },
                                            ^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
              length: { maximum: 64 }
        "#});
    }

    #[test]
    fn does_not_flag_validates_localized() {
        test::<I18nLocaleTexts>().expect_no_offenses(indoc! {r#"
            validates :email, presence: { message: :email_missing },
              format: { with: /@/, message: I18n.t('email_format') },
              length: { maximum: 64 }
        "#});
    }

    #[test]
    fn flags_redirect_to_notice() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            redirect_to root_path, notice: "Post created!"
                                           ^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn flags_redirect_back_notice() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            redirect_back fallback_location: root_path, notice: "Post created!"
                                                                ^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn flags_redirect_back_or_to() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            redirect_back_or_to root_path, notice: "Post created!"
                                                   ^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            redirect_back_or_to root_path, alert: "Failed to update!"
                                                  ^^^^^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn does_not_flag_redirect_localized() {
        test::<I18nLocaleTexts>().expect_no_offenses(
            "redirect_back_or_to root_path, notice: t(\".success\")\n",
        );
        test::<I18nLocaleTexts>()
            .expect_no_offenses("redirect_to root_path, notice: t(\".success\")\n");
    }

    #[test]
    fn flags_flash_assignment() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            flash[:notice] = "Post created!"
                             ^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn flags_flash_now_assignment() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            flash.now[:notice] = "Post created!"
                                 ^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn does_not_flag_flash_localized() {
        test::<I18nLocaleTexts>()
            .expect_no_offenses("flash[:notice] = t(\".success\")\n");
        test::<I18nLocaleTexts>()
            .expect_no_offenses("flash.now[:notice] = t(\".success\")\n");
    }

    #[test]
    fn flags_mail_subject() {
        test::<I18nLocaleTexts>().expect_offense(indoc! {r#"
            mail(to: user.email, subject: "Welcome to My Awesome Site")
                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Move locale texts to the locale files in the `config/locales` directory.
        "#});
    }

    #[test]
    fn does_not_flag_mail_localized() {
        test::<I18nLocaleTexts>().expect_no_offenses("mail(to: user.email)\n");
        test::<I18nLocaleTexts>().expect_no_offenses(
            "mail(to: user.email, subject: t(\"mailers.users.welcome\"))\n",
        );
    }
}
murphy_plugin_api::submit_cop!(I18nLocaleTexts);
