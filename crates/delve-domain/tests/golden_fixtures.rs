use std::fs;
use std::path::Path;

use delve_domain::SessionTree;

fn load_fixture(path: &str) -> SessionTree {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path);
    let fixture_body = fs::read_to_string(&fixture_path).expect("fixture should be readable");
    serde_json::from_str(&fixture_body).expect("fixture should parse")
}

#[test]
fn valid_prompt_continuation_fixture_is_accepted() {
    let session = load_fixture("valid-prompt-continuation.json");

    session
        .validate_tree_invariants()
        .expect("fixture should satisfy tree invariants");

    let context_nodes = session
        .resolve_eligible_context_node_ids()
        .expect("context should resolve");

    assert_eq!(
        context_nodes,
        vec![
            delve_domain::NodeId::from("artifact-shared-context"),
            delve_domain::NodeId::from("artifact-accepted"),
        ]
    );
}

#[test]
fn invalid_artifact_parent_fixture_is_rejected() {
    let session = load_fixture("invalid-artifact-parent.json");

    assert!(session.validate_tree_invariants().is_err());
}
