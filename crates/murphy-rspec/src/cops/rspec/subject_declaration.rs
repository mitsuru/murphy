//! `RSpec/SubjectDeclaration` — define subject with the subject helper.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SubjectDeclaration
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `offensive_subject_declaration?`
//!   (`(send nil? ${#Subjects.all #Helpers.all} ({sym str} #Subjects.all)
//!   ...)`) with `RESTRICT_ON_SEND = [:subject, :subject!, :let, :let!]`
//!   (bare receiver only). The first positional arg must be a `Sym` / `Str`
//!   naming `subject` / `subject!`. `let` / `let!` report `MSG_LET`
//!   (`Use subject explicitly rather than using let`); `subject` /
//!   `subject!` report `MSG_REDUNDANT` (`Ambiguous declaration of subject`).
//!   The offense range is the hook `Send` trimmed to exclude a wrapping block
//!   via `send_without_block_range`, since Murphy's `Send` range covers the
//!   block while RuboCop's does not. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["subject", "subject!", "let",
//! "let!"]` (bare receiver only). Flags when the first arg names the subject:
//!
//! - `let(:subject) { foo }` — flagged (`Use subject explicitly ...`).
//! - `let!(:subject) { foo }` — flagged.
//! - `subject(:subject) { foo }` — flagged (`Ambiguous declaration ...`).
//! - `let(:subject, &block)` — flagged (first arg still names subject).
//! - `subject(:user) { foo }` — different name, not flagged.
//! - `let(:foo) { foo }` — different name, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; renaming to an explicit subject needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::send_without_block_range;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SubjectDeclaration;

#[cop(
    name = "RSpec/SubjectDeclaration",
    description = "Ensure that subject is defined using subject helper.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SubjectDeclaration {
    #[on_node(kind = "send", methods = ["subject", "subject!", "let", "let!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let method_name = cx.symbol_str(method);
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !is_subject_name_arg(cx, first) {
            return;
        }
        let msg = if matches!(method_name, "let" | "let!") {
            "Use subject explicitly rather than using let"
        } else {
            "Ambiguous declaration of subject"
        };
        cx.emit_offense(send_without_block_range(cx, node), msg, None);
    }
}

/// `true` when `arg` names the implicit subject: a `Sym` / `Str`
/// `subject` / `subject!`.
///
/// Mirrors upstream `({sym str} #Subjects.all)` where `Subjects.all` is
/// `subject` / `subject!`.
fn is_subject_name_arg(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        NodeKind::Sym(sym) => matches!(cx.symbol_str(sym), "subject" | "subject!"),
        NodeKind::Str(id) => matches!(cx.string_str(id), "subject" | "subject!"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::SubjectDeclaration;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_let_subject() {
        test::<SubjectDeclaration>().expect_offense(indoc! {r#"
                let(:subject) { foo }
                ^^^^^^^^^^^^^ Use subject explicitly rather than using let
            "#});
    }

    #[test]
    fn flags_let_bang_subject() {
        test::<SubjectDeclaration>().expect_offense(indoc! {r#"
                let!(:subject) { foo }
                ^^^^^^^^^^^^^^ Use subject explicitly rather than using let
            "#});
    }

    #[test]
    fn flags_subject_subject() {
        test::<SubjectDeclaration>().expect_offense(indoc! {r#"
                subject(:subject) { foo }
                ^^^^^^^^^^^^^^^^^ Ambiguous declaration of subject
            "#});
    }

    #[test]
    fn flags_subject_bang_subject() {
        test::<SubjectDeclaration>().expect_offense(indoc! {r#"
                subject!(:subject) { foo }
                ^^^^^^^^^^^^^^^^^^ Ambiguous declaration of subject
            "#});
    }

    #[test]
    fn flags_let_subject_string() {
        test::<SubjectDeclaration>().expect_offense(indoc! {r#"
                let("subject") { foo }
                ^^^^^^^^^^^^^^ Use subject explicitly rather than using let
            "#});
    }

    #[test]
    fn does_not_flag_subject_user() {
        test::<SubjectDeclaration>().expect_no_offenses(indoc! {r#"
                subject(:user) { foo }
            "#});
    }

    #[test]
    fn does_not_flag_let_foo() {
        test::<SubjectDeclaration>().expect_no_offenses(indoc! {r#"
                let(:foo) { foo }
            "#});
    }

    #[test]
    fn does_not_flag_receiver_let_subject() {
        // Upstream requires a bare (`nil?`) receiver.
        test::<SubjectDeclaration>().expect_no_offenses(indoc! {r#"
                obj.let(:subject) { foo }
            "#});
    }
}

murphy_plugin_api::submit_cop!(SubjectDeclaration);
