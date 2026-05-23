//! Runtime behaviour of `#[derive(CopOptions)]`-generated code, against
//! the single-surface ABI (murphy-9cr.21).

// `murphy_plugin_api` re-exports both the `CopOptions` trait and the
// `#[derive(CopOptions)]` macro (single-surface, murphy-9cr.23 §12b/§12d),
// so neither this test nor any cop crate needs to name `murphy_plugin_macros`
// directly to use the derive.
use murphy_plugin_api::{ConfigErrorKind, CopOptions, OptionSpec, RawSlice};

#[derive(CopOptions, Debug)]
struct Opts {
    #[option(default = 80, description = "Maximum line width")]
    max: i64,

    #[option(default = "indented", enum_values = ["indented", "aligned"])]
    style: String,

    #[option(default = ["id"], description = "Names always allowed")]
    allowed: Vec<String>,

    required_flag: bool,

    #[option(deprecated = "use max")]
    maybe: Option<i64>,
}

fn slice_str(slice: RawSlice) -> &'static str {
    std::str::from_utf8(unsafe { slice.as_bytes() }).expect("schema slice is UTF-8")
}

#[test]
fn default_reflects_option_defaults() {
    let d = Opts::default();
    assert_eq!(d.max, 80);
    assert_eq!(d.style, "indented");
    assert_eq!(d.allowed, vec!["id".to_string()]);
    assert!(!d.required_flag); // no #[option(default)] -> Default::default()
    assert_eq!(d.maybe, None);
}

#[test]
fn from_config_json_decodes_valid_input() {
    let json = br#"{
        "max": 100,
        "style": "aligned",
        "allowed": ["a", "b"],
        "required_flag": true,
        "maybe": 5
    }"#;
    let o = Opts::from_config_json(json).expect("valid config decodes");
    assert_eq!(o.max, 100);
    assert_eq!(o.style, "aligned");
    assert_eq!(o.allowed, vec!["a".to_string(), "b".to_string()]);
    assert!(o.required_flag);
    assert_eq!(o.maybe, Some(5));
}

#[test]
fn from_config_json_fills_defaults_for_absent_fields() {
    let json = br#"{"required_flag": false}"#;
    let o = Opts::from_config_json(json).expect("valid config decodes");
    assert_eq!(o.max, 80);
    assert_eq!(o.style, "indented");
    assert_eq!(o.allowed, vec!["id".to_string()]);
    assert_eq!(o.maybe, None);
}

#[test]
fn from_config_json_treats_null_option_as_none() {
    let json = br#"{"required_flag": true, "maybe": null}"#;
    let o = Opts::from_config_json(json).expect("valid config decodes");
    assert_eq!(o.maybe, None);
}

#[test]
fn from_config_json_rejects_type_mismatch() {
    let json = br#"{"max": "not a number", "required_flag": true}"#;
    let err = Opts::from_config_json(json).expect_err("type mismatch rejected");
    match err.kind() {
        ConfigErrorKind::TypeMismatch { field, expected } => {
            assert_eq!(field, "max");
            assert_eq!(*expected, "int");
        }
        other => panic!("expected TypeMismatch, got {other:?}"),
    }
}

#[test]
fn from_config_json_rejects_enum_violation() {
    let json = br#"{"style": "nonsense", "required_flag": true}"#;
    let err = Opts::from_config_json(json).expect_err("enum violation rejected");
    match err.kind() {
        ConfigErrorKind::EnumViolation { field, value } => {
            assert_eq!(field, "style");
            assert_eq!(value, "nonsense");
        }
        other => panic!("expected EnumViolation, got {other:?}"),
    }
}

#[test]
fn from_config_json_rejects_missing_required() {
    let json = br#"{"max": 50}"#;
    let err = Opts::from_config_json(json).expect_err("missing required rejected");
    match err.kind() {
        ConfigErrorKind::MissingRequired { field } => assert_eq!(field, "required_flag"),
        other => panic!("expected MissingRequired, got {other:?}"),
    }
}

#[test]
fn from_config_json_rejects_non_object() {
    let json = br#"[1, 2, 3]"#;
    let err = Opts::from_config_json(json).expect_err("non-object rejected");
    assert!(matches!(err.kind(), ConfigErrorKind::NotAnObject));
}

#[test]
fn from_config_json_rejects_invalid_json() {
    let json = br#"{not valid"#;
    let err = Opts::from_config_json(json).expect_err("invalid JSON rejected");
    assert!(matches!(err.kind(), ConfigErrorKind::Parse(_)));
}

#[test]
fn schema_describes_each_field_in_order() {
    let schema: &[OptionSpec] = Opts::SCHEMA;
    assert_eq!(schema.len(), 5);

    assert_eq!(slice_str(schema[0].name), "max");
    assert_eq!(slice_str(schema[0].ty), "int");
    assert_eq!(slice_str(schema[0].default_json), "80");
    assert_eq!(slice_str(schema[0].description), "Maximum line width");

    // An enum field's `ty` stays the base wire type (`string`); the
    // single-surface ABI carries enum-ness in `enum_values_json`, not as
    // a distinct `ty` string (the pre-reboot ABI used `ty = "enum"`).
    assert_eq!(slice_str(schema[1].name), "style");
    assert_eq!(slice_str(schema[1].ty), "string");
    assert_eq!(slice_str(schema[1].default_json), "\"indented\"");
    assert_eq!(
        slice_str(schema[1].enum_values_json),
        "[\"indented\",\"aligned\"]"
    );

    assert_eq!(slice_str(schema[2].name), "allowed");
    assert_eq!(slice_str(schema[2].ty), "string_list");
    assert_eq!(slice_str(schema[2].default_json), "[\"id\"]");

    assert_eq!(slice_str(schema[3].name), "required_flag");
    assert_eq!(slice_str(schema[3].ty), "bool");
    assert_eq!(slice_str(schema[3].default_json), "");

    assert_eq!(slice_str(schema[4].name), "maybe");
    assert_eq!(slice_str(schema[4].ty), "int");
    assert_eq!(slice_str(schema[4].replacement), "use max");
}
