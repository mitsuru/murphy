//! `#[on_new_investigation]` lowers to `KINDS = &[]` + a `check` that
//! delegates to the marked method. Imports `Cx` from
//! `murphy_plugin_api` to demonstrate the single-surface authorship
//! path the macro now supports (no `use murphy_ast::*;` needed by the
//! pack). The user method signature is `fn(&self, &Cx<'_>)` — no
//! `NodeId`, matching RuboCop's `on_new_investigation(&self)`.

use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct Comments;

#[cop(name = "Plugin/Comments")]
impl Comments {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic, Comments);

fn main() {}
