//! Guards the machine-readable CLI runtime adoption receipt and its example evidence.

use std::collections::BTreeSet;

const SCHEMA: &str = include_str!("../contracts/cli-runtime/v1/adoption-receipt.schema.json");
const EXAMPLE: &str = include_str!("../contracts/cli-runtime/v1/adoption-receipt.example.json");

#[test]
fn adoption_receipt_schema_and_example_cover_fleet_admission_boundaries() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).expect("valid receipt schema");
    let example: serde_json::Value = serde_json::from_str(EXAMPLE).expect("valid receipt example");

    assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(example["schema_version"], "cli-runtime-adoption-v1");
    assert_eq!(example["diagnostics_stream"], "stderr");
    assert_eq!(example["structured_stdout_ansi_free"], true);
    assert_eq!(example["broken_pipe_policy"], "consumer_closed");

    let required: BTreeSet<_> = schema["required"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|value| value.as_str().expect("required field name"))
        .collect();

    for field in [
        "runtime_contract_revision",
        "binary_surfaces",
        "implementation",
        "parser_adapter",
        "output_policy",
        "diagnostics_stream",
        "structured_stdout_ansi_free",
        "broken_pipe_policy",
        "deviations",
        "evidence",
    ] {
        assert!(required.contains(field), "missing required receipt field: {field}");
        assert!(example.get(field).is_some(), "example omits receipt field: {field}");
    }

    assert!(
        schema["properties"]["deviations"]["items"]["properties"]["reviewed"].is_object(),
        "deviations must retain explicit review evidence"
    );
}
