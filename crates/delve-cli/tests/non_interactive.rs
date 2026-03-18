use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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
    let initial_thread_id = read_session_thread_id(&sessions_dir, &session_id);
    assert!(!initial_thread_id.is_empty());

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

    let continued_thread_id = read_session_thread_id(&sessions_dir, &session_id);
    assert_eq!(continued_thread_id, initial_thread_id);

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
    let error_log = fs::read_to_string(sessions_dir.join("errors.log"))
        .expect("error log should be created for command failures");
    assert!(error_log.contains("context=command_error"));
    assert!(error_log.contains("artifact-does-not-exist"));

    let completion = run_delve(&["completion", "--shell", "bash"]);
    assert_success(&completion);
    assert!(stdout_string(&completion).contains("delve"));
}

#[test]
fn amp_continue_refreshes_legacy_thread() {
    let test_root = unique_test_dir("amp-thread-refresh");
    let sessions_dir = test_root.join("sessions");
    let fake_bin_dir = test_root.join("fake-bin");
    fs::create_dir_all(&fake_bin_dir).expect("fake bin dir should be created");

    let fake_amp_log_path = test_root.join("fake-amp.log");
    let fake_amp_path = fake_bin_dir.join("amp");
    write_fake_amp_binary(&fake_amp_path).expect("fake amp should be created");

    let create = run_delve(&[
        "session",
        "create",
        "--intent",
        "Seed legacy thread",
        "--provider",
        "echo",
        "--sessions-dir",
        sessions_dir.to_str().expect("sessions path should be utf8"),
    ]);
    assert_success(&create);
    let session_id = parse_line_value(&stdout_string(&create), "Session ID: ");

    let session_dir = sessions_dir.join(&session_id);
    let session_path = session_dir.join("session.json");
    let mut session_payload: Value = serde_json::from_str(
        &fs::read_to_string(&session_path).expect("session json should exist"),
    )
    .expect("session json should parse");
    session_payload["thread_id"] = Value::String(String::from("thread-amp-legacy-not-compatible"));
    fs::write(
        &session_path,
        serde_json::to_vec_pretty(&session_payload).expect("session json should serialize"),
    )
    .expect("session json should update");

    let fake_path = format!(
        "{}:{}",
        fake_bin_dir.display(),
        env::var("PATH").unwrap_or_default()
    );

    let continue_output = run_delve_with_env(
        &[
            "--json",
            "session",
            "continue",
            "--session",
            &session_id,
            "--prompt",
            "Continue with amp",
            "--provider",
            "amp",
            "--sessions-dir",
            sessions_dir.to_str().expect("sessions path should be utf8"),
        ],
        &[
            ("PATH", fake_path.as_str()),
            (
                "DELVE_FAKE_AMP_LOG",
                fake_amp_log_path
                    .to_str()
                    .expect("fake amp log path should be utf8"),
            ),
        ],
    );
    assert_success(&continue_output);

    let continue_json: Value = serde_json::from_str(&stdout_string(&continue_output))
        .expect("continue output should be valid json");
    assert_eq!(
        continue_json["thread_id"].as_str(),
        Some("T-12345678-1234-1234-1234-1234567890ab")
    );

    let session_thread_id = read_session_thread_id(&sessions_dir, &session_id);
    assert_eq!(session_thread_id, "T-12345678-1234-1234-1234-1234567890ab");

    let amp_log = fs::read_to_string(&fake_amp_log_path).expect("fake amp log should be readable");
    assert!(amp_log.contains("threads new"));
    assert!(amp_log.contains(
        "threads continue T-12345678-1234-1234-1234-1234567890ab -x Continue with amp --stream-json"
    ));
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

fn read_session_thread_id(sessions_dir: &Path, session_id: &str) -> String {
    let session_dir = sessions_dir.join(session_id);
    let session_json =
        fs::read_to_string(session_dir.join("session.json")).expect("session json should exist");
    let payload: Value = serde_json::from_str(&session_json).expect("session json should parse");
    payload["thread_id"]
        .as_str()
        .expect("thread_id should be present")
        .to_string()
}

fn run_delve(args: &[&str]) -> Output {
    run_delve_with_env(args, &[])
}

fn run_delve_with_env(args: &[&str], env_overrides: &[(&str, &str)]) -> Output {
    let mut command = Command::new(delve_binary_path());
    command.args(args);
    for (key, value) in env_overrides {
        command.env(key, value);
    }
    command.output().expect("delve command should execute")
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

#[cfg(unix)]
fn write_fake_amp_binary(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"#!/bin/sh
set -eu

if [ -z "${DELVE_FAKE_AMP_LOG:-}" ]; then
  echo "missing DELVE_FAKE_AMP_LOG" >&2
  exit 97
fi

printf '%s\n' "$*" >> "$DELVE_FAKE_AMP_LOG"

if [ "${1:-}" = "--no-color" ]; then
  shift
fi

if [ "${1:-}" = "--dangerously-allow-all" ]; then
  shift
fi

if [ "${1:-}" = "threads" ] && [ "${2:-}" = "new" ]; then
  echo "Created thread T-12345678-1234-1234-1234-1234567890ab"
  exit 0
fi

if [ "${1:-}" = "threads" ] && [ "${2:-}" = "continue" ]; then
  thread_id="${3:-}"
  if ! printf '%s' "$thread_id" | grep -Eq '^T-[0-9a-fA-F-]{36}$'; then
    echo "invalid thread id: $thread_id" >&2
    exit 98
  fi

  prompt="${5:-}"
  stream_mode="${6:-}"
  if [ "$stream_mode" != "--stream-json" ]; then
    echo "missing --stream-json" >&2
    exit 98
  fi

  printf '{"type":"assistant","message":{"content":[{"text":"amp-update "}]}}\n'
  if [ "$prompt" = "Based on the context available, what it is the suggested to next step to complete this Intent?" ]; then
    printf '{"type":"result","result":"AMP-SUGGESTION"}\n'
  else
    printf '{"type":"result","result":"AMP-ARTIFACT:%s"}\n' "$prompt"
  fi
  exit 0
fi

echo "unexpected fake amp invocation: $*" >&2
exit 99
"#;

    fs::write(path, script)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}
