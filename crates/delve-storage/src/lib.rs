#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use delve_domain::{NodeId, SessionId, SessionTree};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SESSION_SCHEMA_VERSION: u32 = 1;
pub const SESSION_FILE_NAME: &str = "session.json";
pub const EVENTS_FILE_NAME: &str = "events.jsonl";
pub const LOCK_FILE_NAME: &str = "session.lock";
pub const CHECKPOINT_FILE_NAME: &str = "checkpoint.json";
const LABEL_COLLISION_MAX_ATTEMPTS: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    Intent,
    Prompt,
    Artifact,
}

impl LabelKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Prompt => "prompt",
            Self::Artifact => "artifact",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    SessionCreated,
    PromptAdded,
    ArtifactProposed,
    ArtifactAccepted,
    ArtifactRejected,
    SessionCompleted,
    OrchestrationDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub schema_version: u32,
    pub event_kind: SessionEventKind,
    pub session_id: SessionId,
    pub node_id: Option<NodeId>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCheckpoint {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub current_node_id: NodeId,
    pub step: u32,
    pub pending_prompt: Option<String>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub metadata: Value,
}

impl SessionCheckpoint {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        current_node_id: NodeId,
        step: u32,
        pending_prompt: Option<String>,
        metadata: Value,
    ) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id,
            current_node_id,
            step,
            pending_prompt,
            timestamp_ms: now_millis(),
            metadata,
        }
    }
}

#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
    released: bool,
}

impl SessionLock {
    pub fn release(&mut self) -> io::Result<()> {
        if self.released {
            return Ok(());
        }

        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        self.released = true;
        Ok(())
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

impl SessionEvent {
    #[must_use]
    pub fn new(
        event_kind: SessionEventKind,
        session_id: SessionId,
        node_id: Option<NodeId>,
        metadata: Value,
    ) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            event_kind,
            session_id,
            node_id,
            timestamp_ms: now_millis(),
            metadata,
        }
    }
}

pub trait SessionMigration {
    fn source_version(&self) -> u32;
    fn target_version(&self) -> u32;
    fn migrate(&self, raw_session: Value) -> io::Result<Value>;
}

#[must_use]
pub fn session_file_path(base_dir: &Path) -> PathBuf {
    base_dir.join(SESSION_FILE_NAME)
}

#[must_use]
pub fn events_file_path(base_dir: &Path) -> PathBuf {
    base_dir.join(EVENTS_FILE_NAME)
}

#[must_use]
pub fn session_lock_path(base_dir: &Path) -> PathBuf {
    base_dir.join(LOCK_FILE_NAME)
}

#[must_use]
pub fn checkpoint_file_path(base_dir: &Path) -> PathBuf {
    base_dir.join(CHECKPOINT_FILE_NAME)
}

#[must_use]
pub fn session_folder_path(sessions_root: &Path, session_label: &str) -> PathBuf {
    sessions_root.join(normalize_label_segment(session_label, 64))
}

#[must_use]
pub fn generate_intent_label(intent: &str) -> String {
    generate_label(LabelKind::Intent, intent)
}

#[must_use]
pub fn generate_prompt_label(prompt: &str) -> String {
    generate_label(LabelKind::Prompt, prompt)
}

#[must_use]
pub fn generate_artifact_label(artifact: &str) -> String {
    generate_label(LabelKind::Artifact, artifact)
}

#[must_use]
pub fn generate_label(kind: LabelKind, source: &str) -> String {
    build_label(kind, source, 0)
}

pub fn generate_unique_label<F>(
    kind: LabelKind,
    source: &str,
    mut label_exists: F,
) -> io::Result<String>
where
    F: FnMut(&str) -> bool,
{
    for attempt in 0..LABEL_COLLISION_MAX_ATTEMPTS {
        let candidate = build_label(kind, source, attempt);
        if !label_exists(&candidate) {
            return Ok(candidate);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "unable to create unique label for '{}' after {} attempts",
            kind.prefix(),
            LABEL_COLLISION_MAX_ATTEMPTS
        ),
    ))
}

pub fn write_json_atomic<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = serde_json::to_string_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let temp_path = temporary_path(path);

    {
        let mut temp_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp_file.write_all(serialized.as_bytes())?;
        temp_file.sync_all()?;
    }

    fs::rename(&temp_path, path).or_else(|err| {
        let _ = fs::remove_file(path);
        fs::rename(&temp_path, path).map_err(|_| err)
    })
}

pub fn write_session_json(base_dir: &Path, session: &SessionTree) -> io::Result<()> {
    if session.schema_version != SESSION_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unsupported session schema version {} for write; expected {}",
                session.schema_version, SESSION_SCHEMA_VERSION
            ),
        ));
    }

    let path = session_file_path(base_dir);
    write_json_atomic(&path, session)
}

pub fn read_session_json(base_dir: &Path) -> io::Result<SessionTree> {
    load_session_json(base_dir)
}

pub fn acquire_session_lock(base_dir: &Path) -> io::Result<SessionLock> {
    fs::create_dir_all(base_dir)?;
    let path = session_lock_path(base_dir);
    let mut lock_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    writeln!(lock_file, "pid={}", std::process::id())?;
    writeln!(lock_file, "timestamp_ms={}", now_millis())?;

    Ok(SessionLock {
        path,
        released: false,
    })
}

pub fn load_session_json(base_dir: &Path) -> io::Result<SessionTree> {
    let path = session_file_path(base_dir);
    let serialized = fs::read_to_string(path)?;
    let session = serde_json::from_str::<SessionTree>(&serialized)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    if session.schema_version != SESSION_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported session schema version {}; expected {}",
                session.schema_version, SESSION_SCHEMA_VERSION
            ),
        ));
    }

    Ok(session)
}

pub fn load_session_json_with_migrations(
    base_dir: &Path,
    migrations: &[&dyn SessionMigration],
) -> io::Result<SessionTree> {
    let path = session_file_path(base_dir);
    let serialized = fs::read_to_string(path)?;
    let mut raw_session: Value = serde_json::from_str(&serialized)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut version = extract_schema_version(&raw_session)?;

    while version != SESSION_SCHEMA_VERSION {
        let migration = migrations
            .iter()
            .find(|candidate| candidate.source_version() == version)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "no migration found from schema version {} to {}",
                        version, SESSION_SCHEMA_VERSION
                    ),
                )
            })?;

        raw_session = migration.migrate(raw_session)?;
        version = extract_schema_version(&raw_session)?;
        if version != migration.target_version() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "migration from {} reported target {} but produced {}",
                    migration.source_version(),
                    migration.target_version(),
                    version
                ),
            ));
        }
    }

    serde_json::from_value(raw_session)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub fn append_session_event(base_dir: &Path, event: &SessionEvent) -> io::Result<()> {
    if event.schema_version != SESSION_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unsupported event schema version {} for append; expected {}",
                event.schema_version, SESSION_SCHEMA_VERSION
            ),
        ));
    }

    fs::create_dir_all(base_dir)?;
    let path = events_file_path(base_dir);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let serialized = serde_json::to_string(event)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writeln!(file, "{serialized}")
}

pub fn write_session_checkpoint(base_dir: &Path, checkpoint: &SessionCheckpoint) -> io::Result<()> {
    if checkpoint.schema_version != SESSION_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unsupported checkpoint schema version {} for write; expected {}",
                checkpoint.schema_version, SESSION_SCHEMA_VERSION
            ),
        ));
    }

    let path = checkpoint_file_path(base_dir);
    write_json_atomic(&path, checkpoint)
}

pub fn read_session_checkpoint(base_dir: &Path) -> io::Result<Option<SessionCheckpoint>> {
    let path = checkpoint_file_path(base_dir);
    if !path.exists() {
        return Ok(None);
    }

    let serialized = fs::read_to_string(path)?;
    let checkpoint = serde_json::from_str::<SessionCheckpoint>(&serialized)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    if checkpoint.schema_version != SESSION_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported checkpoint schema version {}; expected {}",
                checkpoint.schema_version, SESSION_SCHEMA_VERSION
            ),
        ));
    }

    Ok(Some(checkpoint))
}

pub fn clear_session_checkpoint(base_dir: &Path) -> io::Result<()> {
    let path = checkpoint_file_path(base_dir);
    if path.exists() {
        fs::remove_file(path)?;
    }

    Ok(())
}

pub fn read_session_events(base_dir: &Path) -> io::Result<Vec<SessionEvent>> {
    let path = events_file_path(base_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = OpenOptions::new().read(true).open(path)?;
    let mut events = Vec::new();

    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let event: SessionEvent = serde_json::from_str(&line)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        events.push(event);
    }

    Ok(events)
}

fn build_label(kind: LabelKind, source: &str, attempt: u32) -> String {
    let slug = normalize_label_segment(source, 16);
    let token = short_hash_token(&format!("{}:{}:{attempt}", kind.prefix(), source));
    format!("{}-{slug}-{token}", kind.prefix())
}

fn normalize_label_segment(input: &str, max_len: usize) -> String {
    let mut normalized = String::new();
    let mut previous_was_hyphen = false;

    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else {
            Some('-')
        };

        if let Some(next) = mapped {
            if next == '-' {
                if previous_was_hyphen || normalized.is_empty() {
                    continue;
                }
                previous_was_hyphen = true;
                normalized.push('-');
            } else {
                previous_was_hyphen = false;
                normalized.push(next);
            }
        }

        if normalized.len() >= max_len {
            break;
        }
    }

    while normalized.ends_with('-') {
        normalized.pop();
    }

    if normalized.is_empty() {
        String::from("item")
    } else {
        normalized
    }
}

fn short_hash_token(value: &str) -> String {
    let mut hash = 0x811C_9DC5_u32;
    for byte in value.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }

    format!("{hash:08x}")
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.json");
    let token = now_millis();
    path.with_file_name(format!(".{file_name}.{token}.tmp"))
}

fn extract_schema_version(raw_session: &Value) -> io::Result<u32> {
    let version = raw_session
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing or invalid schema_version in session payload",
            )
        })?;

    u32::try_from(version).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("schema_version {version} is too large"),
        )
    })
}

fn now_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::env;
    use std::io;
    use std::path::PathBuf;

    use delve_domain::{NodeId, SessionId, SessionTree};
    use serde_json::{json, Value};

    use super::{
        acquire_session_lock, append_session_event, checkpoint_file_path, clear_session_checkpoint,
        generate_artifact_label, generate_intent_label, generate_label, generate_prompt_label,
        generate_unique_label, load_session_json, load_session_json_with_migrations,
        read_session_checkpoint, read_session_events, session_folder_path,
        write_session_checkpoint, write_session_json, LabelKind, SessionCheckpoint, SessionEvent,
        SessionEventKind, SessionMigration, SESSION_FILE_NAME, SESSION_SCHEMA_VERSION,
    };

    #[test]
    fn label_generation_is_deterministic_and_prefixed() {
        let intent = generate_intent_label("Ship V1 with context");
        let prompt = generate_prompt_label("Continue the implementation");
        let artifact = generate_artifact_label("Add CLI tests");

        assert!(intent.starts_with("intent-"));
        assert!(prompt.starts_with("prompt-"));
        assert!(artifact.starts_with("artifact-"));

        assert_eq!(
            intent,
            generate_label(LabelKind::Intent, "Ship V1 with context")
        );
    }

    #[test]
    fn collision_retry_uses_incrementing_attempts() {
        let mut existing = HashSet::new();
        let first = generate_label(LabelKind::Artifact, "same-content");
        existing.insert(first.clone());

        let generated = generate_unique_label(LabelKind::Artifact, "same-content", |candidate| {
            existing.contains(candidate)
        })
        .expect("collision retry should generate a distinct label");

        assert_ne!(generated, first);
        assert!(generated.starts_with("artifact-"));
    }

    #[test]
    fn session_round_trip_uses_schema_checked_loader() {
        let test_dir = unique_test_dir("roundtrip");
        let mut session = SessionTree::new("Intent");
        session.session_id = SessionId::from("session-roundtrip");

        write_session_json(&test_dir, &session).expect("session write should succeed");
        let reloaded = load_session_json(&test_dir).expect("session load should succeed");

        assert_eq!(reloaded.session_id, SessionId::from("session-roundtrip"));
        assert_eq!(reloaded.schema_version, SESSION_SCHEMA_VERSION);

        std::fs::remove_dir_all(test_dir).expect("test directory should be removable");
    }

    #[test]
    fn appends_and_reads_events_jsonl() {
        let test_dir = unique_test_dir("events");
        let event = SessionEvent::new(
            SessionEventKind::SessionCreated,
            SessionId::from("session-1"),
            None,
            json!({"provider":"echo"}),
        );

        append_session_event(&test_dir, &event).expect("event append should succeed");
        let events = read_session_events(&test_dir).expect("event load should succeed");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind, SessionEventKind::SessionCreated);
        assert_eq!(events[0].session_id, SessionId::from("session-1"));

        std::fs::remove_dir_all(test_dir).expect("test directory should be removable");
    }

    #[test]
    fn migration_hook_is_applied_when_schema_is_older() {
        let test_dir = unique_test_dir("migrations");
        let payload = json!({
            "schema_version": 0,
            "session_id": "session-migrate",
            "intent_node_id": "intent-root",
            "current_node_id": "intent-root",
            "state": "Active",
            "nodes": [
                {
                    "id": "intent-root",
                    "label": "Intent",
                    "kind": "Intent",
                    "artifact_kind": null,
                    "status": "Accepted",
                    "parent_id": null,
                    "children_ids": [],
                    "input_node_ids": [],
                    "payload_ref": "intent.md"
                }
            ]
        });
        std::fs::create_dir_all(&test_dir).expect("test directory should be created");
        std::fs::write(
            super::session_file_path(&test_dir),
            serde_json::to_string_pretty(&payload).expect("payload should serialize"),
        )
        .expect("payload should be written");

        struct V0ToV1;
        impl SessionMigration for V0ToV1 {
            fn source_version(&self) -> u32 {
                0
            }

            fn target_version(&self) -> u32 {
                1
            }

            fn migrate(&self, mut raw_session: Value) -> io::Result<Value> {
                raw_session["schema_version"] = Value::from(1);
                Ok(raw_session)
            }
        }

        let migration = V0ToV1;
        let migrated = load_session_json_with_migrations(&test_dir, &[&migration])
            .expect("migrated payload should load");
        assert_eq!(migrated.schema_version, 1);

        std::fs::remove_dir_all(test_dir).expect("test directory should be removable");
    }

    #[test]
    fn session_folder_builder_is_deterministic() {
        let root = unique_test_dir("folder");
        let path_a = session_folder_path(&root, "Intent Label 123");
        let path_b = session_folder_path(&root, "Intent Label 123");

        assert_eq!(path_a, path_b);
    }

    #[test]
    fn acquires_and_releases_session_locks() {
        let test_dir = unique_test_dir("locks");
        std::fs::create_dir_all(&test_dir).expect("test directory should be created");

        let _first_lock = acquire_session_lock(&test_dir).expect("first lock should be acquired");
        assert!(acquire_session_lock(&test_dir).is_err());
    }

    #[test]
    fn writes_reads_and_clears_checkpoints() {
        let test_dir = unique_test_dir("checkpoints");
        let checkpoint = SessionCheckpoint::new(
            SessionId::from("session-checkpoint"),
            NodeId::from("prompt-1"),
            2,
            Some(String::from("continue")),
            json!({"auto":true}),
        );

        write_session_checkpoint(&test_dir, &checkpoint).expect("checkpoint should persist");
        let loaded = read_session_checkpoint(&test_dir)
            .expect("checkpoint should be readable")
            .expect("checkpoint should exist");
        assert_eq!(loaded.session_id, SessionId::from("session-checkpoint"));
        assert_eq!(loaded.current_node_id, NodeId::from("prompt-1"));

        clear_session_checkpoint(&test_dir).expect("checkpoint should be cleared");
        let cleared =
            read_session_checkpoint(&test_dir).expect("checkpoint read should succeed after clear");
        assert!(cleared.is_none());
    }

    #[test]
    fn interrupted_write_temp_files_do_not_break_session_loads() {
        let test_dir = unique_test_dir("interrupted-write");
        let mut session = SessionTree::new("Intent");
        session.session_id = SessionId::from("session-interrupted");
        write_session_json(&test_dir, &session).expect("session should be written");

        std::fs::write(
            test_dir.join(format!(".{SESSION_FILE_NAME}.stale.tmp")),
            "{ malformed json",
        )
        .expect("stale temp file should be written");

        let loaded = load_session_json(&test_dir).expect("stable session file should still load");
        assert_eq!(loaded.session_id, SessionId::from("session-interrupted"));
    }

    #[test]
    fn interrupted_checkpoint_temp_files_do_not_break_checkpoint_reads() {
        let test_dir = unique_test_dir("interrupted-checkpoint");
        let checkpoint = SessionCheckpoint::new(
            SessionId::from("session-checkpoint-interrupted"),
            NodeId::from("prompt-2"),
            3,
            None,
            json!({}),
        );
        write_session_checkpoint(&test_dir, &checkpoint).expect("checkpoint should persist");

        std::fs::write(
            checkpoint_file_path(&test_dir).with_file_name(".checkpoint.json.stale.tmp"),
            "{ malformed json",
        )
        .expect("stale checkpoint temp file should be written");

        let loaded = read_session_checkpoint(&test_dir)
            .expect("checkpoint load should succeed")
            .expect("checkpoint should exist");
        assert_eq!(loaded.current_node_id, NodeId::from("prompt-2"));
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let timestamp = super::now_millis();
        env::temp_dir().join(format!("delve-storage-tests-{label}-{timestamp}"))
    }
}
