use murphy_plugin_api::{Cop, NoOptions, Severity};

struct NoTabs;
struct NoSpaces;

impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}

impl Cop for NoSpaces {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoSpaces";
    const DESCRIPTION: &'static str = "Forbids trailing spaces.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
}

murphy_plugin_macros::register_cops!(NoTabs, NoSpaces);

fn main() {}
