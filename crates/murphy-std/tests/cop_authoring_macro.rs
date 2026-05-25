use std::fs;
use std::path::Path;

#[test]
fn space_inside_parens_uses_authoring_macros() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = Path::new(manifest_dir).join("src/cops/layout/space_inside_parens.rs");
    let source = fs::read_to_string(path).expect("read Layout/SpaceInsideParens source");

    assert!(
        source.contains("#[cop("),
        "Layout/SpaceInsideParens should use the documented #[cop] authoring API",
    );
    assert!(
        source.contains("#[on_new_investigation]"),
        "Layout/SpaceInsideParens should use macro-based dispatch",
    );
    assert!(
        !source.contains("impl Cop for SpaceInsideParens")
            && !source.contains("impl NodeCop for SpaceInsideParens"),
        "Layout/SpaceInsideParens should not hand-write Cop/NodeCop impls",
    );
}
