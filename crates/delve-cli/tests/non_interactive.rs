use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn non_interactive_workflow_create_show_accept_complete() {
    let test_root = unique_test_dir("workflow");
    let sessions_dir = test_root.join("sessions");

    let create = run_delve(&[
        "session",
        "create",
        "--intent",
        "Build a release checklist",
        "--provider",
        "echo",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&create);
    let create_stdout = stdout_string(&create);
    assert!(create_stdout.contains("session create"));
    assert!(create_stdout.contains("echo:Build a release checklist"));
    let session_id = parse_line_value(&create_stdout, "Session ID: ");

    let list = run_delve(&[
        "session",
        "list",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&list);
    assert!(stdout_string(&list).contains(&session_id));

    let show = run_delve(&[
        "session",
        "show",
        "--session",
        &session_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&show);
    assert!(stdout_string(&show).contains("session show"));

    let artifact_id = find_latest_artifact_id(&sessions_dir, &session_id);
    let artifact_show = run_delve(&[
        "artifact",
        "show",
        "--session",
        &session_id,
        "--artifact",
        &artifact_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&artifact_show);
    assert!(stdout_string(&artifact_show).contains("echo:Build a release checklist"));

    let accept = run_delve(&[
        "artifact",
        "accept",
        "--session",
        &session_id,
        "--artifact",
        &artifact_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&accept);

    let complete = run_delve(&[
        "session",
        "complete",
        "--session",
        &session_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&complete);
    assert!(stdout_string(&complete).contains("State: Completed"));
}

#[test]
fn provider_backed_continue_streams_and_persists_artifact_payload() {
    let test_root = unique_test_dir("continue-stream");
    let sessions_dir = test_root.join("sessions");

    let create = run_delve(&[
        "session",
        "create",
        "--intent",
        "Seed session",
        "--provider",
        "echo",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&create);
    let session_id = parse_line_value(&stdout_string(&create), "Session ID: ");

    let continue_output = run_delve(&[
        "session",
        "continue",
        "--session",
        &session_id,
        "--prompt",
        "Continue with implementation details",
        "--provider",
        "echo",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&continue_output);
    let continue_stdout = stdout_string(&continue_output);
    assert!(continue_stdout.contains("echo:Continue with implementation details"));

    let (artifact_id, artifact_payload_path) =
        find_latest_artifact_id_and_path(&sessions_dir, &session_id);
    let artifact_payload = fs::read_to_string(&artifact_payload_path)
        .expect("artifact payload should be readable after continue");
    assert!(artifact_payload.contains("echo:Continue with implementation details"));

    let artifact_show = run_delve(&[
        "artifact",
        "show",
        "--session",
        &session_id,
        "--artifact",
        &artifact_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&artifact_show);
    assert!(stdout_string(&artifact_show).contains("echo:Continue with implementation details"));

    let reject = run_delve(&[
        "artifact",
        "reject",
        "--session",
        &session_id,
        "--artifact",
        &artifact_id,
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&reject);
}

#[test]
fn json_mode_and_exit_codes_are_script_friendly() {
    let test_root = unique_test_dir("json-and-exit-codes");
    let sessions_dir = test_root.join("sessions");

    let create = run_delve(&[
        "--json",
        "session",
        "create",
        "--intent",
        "Script mode",
        "--provider",
        "echo",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&create);
    let parsed: Value = serde_json::from_str(&stdout_string(&create))
        .expect("json mode should produce valid json output");
    let session_id = parsed["session_id"]
        .as_str()
        .expect("json output should include session_id")
        .to_string();

    let missing_artifact = run_delve(&[
        "artifact",
        "show",
        "--session",
        &session_id,
        "--artifact",
        "artifact-does-not-exist",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_eq!(missing_artifact.status.code(), Some(3));

    let completion = run_delve(&["completion", "--shell", "bash"]);
    assert_success(&completion);
    assert!(stdout_string(&completion).contains("delve"));
}

fn find_latest_artifact_id(sessions_dir: &Path, session_id: &str) -> String {
    let (artifact_id, _) = find_latest_artifact_id_and_path(sessions_dir, session_id);
    artifact_id
}

fn find_latest_artifact_id_and_path(sessions_dir: &Path, session_id: &str) -> (String, PathBuf) {
    let session_dir = sessions_dir.join(session_id);
    let session_json =
        fs::read_to_string(session_dir.join("session.json")).expect("session json should exist");
    let payload: Value = serde_json::from_str(&session_json).expect("session json should parse");

    let artifact_node = payload["nodes"]
        .as_array()
        .expect("nodes should be an array")
        .iter()
        .rev()
        .find(|node| node["kind"] == "Artifact")
        .expect("artifact node should exist");

    let artifact_id = artifact_node["id"]
        .as_str()
        .expect("artifact id should be present")
        .to_string();
    let artifact_rel_path = artifact_node["payload_ref"]
        .as_str()
        .expect("artifact payload_ref should be present");

    (artifact_id, session_dir.join(artifact_rel_path))
}

fn parse_line_value(output: &str, prefix: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .map(ToOwned::to_owned)
        .expect("expected line to exist in output")
}

fn run_delve(args: &[&str]) -> Output {
    Command::new(delve_binary_path())
        .args(args)
        .output()
        .expect("delve command should execute")
}

fn delve_binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_delve")
}

fn assert_success(output: &Output) {
    if !output.status.success() {
        panic!(
            "command failed with code {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            stdout_string(output),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn stdout_string(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf8")
}

fn unique_test_dir(label: &str) -> PathBuf {
    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!("delve-cli-integration-{label}-{epoch_nanos}"))
}
