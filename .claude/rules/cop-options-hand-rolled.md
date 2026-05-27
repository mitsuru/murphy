# Hand-rolled `CopOptions::from_config_json` must mirror the derive's error contract

`#[derive(CopOptions)]` covers `bool`, `i64`, `String`, `Vec<String>`, `Option<bool|i64|String>`, and `CopOptionEnum` fields. Anything outside this set (nested maps, custom types, conditional decoding) requires a hand-rolled `impl CopOptions`. When you go hand-rolled, **match the derive's error contract** — silent default-fallback on shape mismatch lets configuration typos go unnoticed.

## Required error shapes

| Condition | Return |
|---|---|
| `serde_json` parse failure | `ConfigError::parse(err)` |
| Top-level JSON is not an object | `ConfigError::not_an_object()` |
| Named field absent | `Ok(Self::default())` for that field (matches derive's handling of omitted fields) |
| Named field present but wrong shape | `ConfigError::type_mismatch(field, expected)` |

The `field` argument should be a **path-qualified** name when the mismatch is inside a nested structure (e.g. `"IgnoredMetadata.type[1]"`), so diagnostics tell users which leaf is wrong.

## Pattern

```rust
impl CopOptions for MyOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(ConfigError::parse)?;
        let obj = value.as_object()
            .ok_or_else(ConfigError::not_an_object)?;

        // Missing field → defaults (consistent with derive).
        let Some(field_value) = obj.get("MyField") else {
            return Ok(Self::default());
        };

        // Present-but-wrong-shape → typed error.
        let field_obj = field_value.as_object()
            .ok_or_else(|| ConfigError::type_mismatch("MyField", "object"))?;

        // ... decode the inner structure, surfacing typed errors with
        // path-qualified field names like "MyField.<key>[<i>]".
    }

    fn to_config_json(&self) -> String { /* must roundtrip via from_config_json */ }
}
```

## Anti-pattern

```rust
// Avoid: silent fallback hides typos. A user who writes
// `IgnoreMetadata` (missing 'd') gets the default and never learns.
let Some(serde_json::Value::Object(map)) = obj.get("IgnoredMetadata") else {
    return Ok(Self::default());
};
// Avoid: silent skip on inner shape mismatch. A user who writes
// `IgnoredMetadata = { "type": "request" }` (forgot the array) gets
// an empty BTreeSet and the cop silently fires on what they wanted to ignore.
if let Some(arr) = vs.as_array() { ... } // wrong shape → silently empty
```

## Test pins

For each error path, add a test that calls `from_config_json` directly and asserts on the returned `ConfigErrorKind`. The harness pattern:

```rust
let err = <MyOptions as CopOptions>::from_config_json(json)
    .expect_err("wrong shape is invalid");
let ConfigErrorKind::TypeMismatch { field, expected } = err.kind() else {
    panic!("expected TypeMismatch, got {:?}", err.kind());
};
assert_eq!(field, "MyField.<key>[<i>]");
assert_eq!(*expected, "string");
```

## See also

- `crates/murphy-rspec/src/cops/rspec/describe_class.rs` — canonical hand-rolled impl with the full error surface (root, nested object, value array, element string).
- `crates/murphy-plugin-macros/src/cop_options.rs` (`generate_copoptions`) — the derive-generated `from_config_json` that defines the contract.
- `crates/murphy-plugin-api/src/config_error.rs` — `ConfigError` constructors.
