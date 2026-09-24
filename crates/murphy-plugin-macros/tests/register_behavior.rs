//! Runtime behaviour of the `register_cops!`-generated registration
//! entry point, against the single-surface ABI (murphy-9cr.21).
//!
//! `register_cops!` emits a `#[no_mangle]` `murphy_plugin_register`
//! inside an anonymous `const` block, so its name is not in scope here;
//! the test re-declares the exported symbol through an `extern` block,
//! exactly as the `.so` loader (murphy-9cr.22) will resolve it.

use murphy_ast::NodeId;
use murphy_plugin_api::{
    Cop, Cx, MURPHY_PLUGIN_ABI_VERSION, NoOptions, NodeCop, NodeKindTag, PluginRegistration,
    Severity,
};
use murphy_plugin_macros::register_cops;

#[derive(Default)]
struct NoTabs;
impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}
impl NodeCop for NoTabs {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

#[derive(Default)]
struct NoSpaces;
impl Cop for NoSpaces {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoSpaces";
    const DESCRIPTION: &'static str = "Forbids trailing spaces.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
}
impl NodeCop for NoSpaces {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(2), NodeKindTag(3)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(NoTabs);
murphy_plugin_api::submit_cop!(NoSpaces);

// Duplicate-NAME detection is now an unconditional runtime check
// executed inside murphy_plugin_register when it iterates PACK_COPS.
// A compile-time check is no longer possible because submit_cop! calls
// are distributed across files — the linker collects them, not the macro.

unsafe extern "C" {
    fn murphy_plugin_register(out: *mut PluginRegistration) -> i32;
}

fn empty_registration() -> PluginRegistration {
    PluginRegistration {
        abi_version: 0,
        cops_ptr: std::ptr::null(),
        cops_len: 0,
    }
}

#[test]
fn register_entry_point_fills_the_plugin_registration() {
    let mut reg = empty_registration();
    let rc = unsafe { murphy_plugin_register(&mut reg) };

    assert_eq!(rc, 0);
    assert_eq!(reg.abi_version, MURPHY_PLUGIN_ABI_VERSION);
    assert_eq!(reg.cops_len, 2);

    let cops = unsafe { std::slice::from_raw_parts(reg.cops_ptr, reg.cops_len) };

    // Linker inventory order can vary with the workspace feature graph.
    let no_tabs = cops
        .iter()
        .find(|cop| unsafe { cop.name.as_bytes() == b"Plugin/NoTabs" })
        .expect("NoTabs registration is present");
    let no_spaces = cops
        .iter()
        .find(|cop| unsafe { cop.name.as_bytes() == b"Plugin/NoSpaces" })
        .expect("NoSpaces registration is present");

    assert_eq!(unsafe { no_tabs.description.as_bytes() }, b"");
    assert_eq!(no_tabs.kinds_len, 1);

    assert_eq!(
        unsafe { no_spaces.description.as_bytes() },
        b"Forbids trailing spaces."
    );
    assert_eq!(
        no_spaces.default_severity,
        Severity::to_wire(Some(Severity::Warning))
    );
    assert_eq!(no_spaces.kinds_len, 2);
}

#[test]
fn register_rejects_a_null_out_pointer() {
    let rc = unsafe { murphy_plugin_register(std::ptr::null_mut()) };
    assert_ne!(rc, 0);
}
