use std::collections::BTreeSet;

const VECTORS: &str = include_str!("../contracts/cli-runtime/v1/conformance-vectors.json");

#[test]
fn conformance_vectors_are_well_formed_unique_and_cover_core_boundaries() {
    let document: serde_json::Value = serde_json::from_str(VECTORS).expect("valid JSON vector corpus");
    assert_eq!(document["version"], "cli-runtime-v1");

    let authority_note = document["authority_note"]
        .as_str()
        .expect("authority_note string");
    assert!(authority_note.contains("evidence only"));
    assert!(authority_note.contains("independent peer authorities"));

    let vectors = document["vectors"].as_array().expect("vectors array");
    assert!(vectors.len() >= 16, "expected broad v1 boundary coverage");

    let mut ids = BTreeSet::new();
    for vector in vectors {
        let id = vector["id"].as_str().expect("every vector has an id");
        assert!(
            id.chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'),
            "vector id must be canonical snake_case: {id}"
        );
        assert!(ids.insert(id), "duplicate conformance vector id: {id}");
        assert!(vector["kind"].is_string(), "vector {id} must name its kind");
        assert!(vector.get("expected").is_some(), "vector {id} needs expected evidence");
    }

    for required in [
        "argv_output_conflict",
        "argv_terminator_stops_shared_parser",
        "runtime_tty_auto_human",
        "runtime_redirect_auto_json",
        "runtime_json_stdout_never_colored",
        "log_warn_threshold",
        "stream_machine_record_rejects_ansi",
        "stream_role_separation",
        "stream_broken_pipe_is_clean_consumer_close",
        "stream_permission_denied_is_not_swallowed",
    ] {
        assert!(ids.contains(required), "missing required vector: {required}");
    }
}
