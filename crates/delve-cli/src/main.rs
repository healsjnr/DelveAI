#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use delve_domain::{
    ArtifactKind, NodeId, NodeKind, NodeStatus, SessionId, SessionNode, SessionState, SessionTree,
};
use delve_orchestrator::{
    execute_review, generate_artifact_streaming_with_thread, suggest_next_prompt_with_provider,
};
use delve_providers::{
    AmpProvider, ClaudeProvider, CompletionProvider, EchoProvider, ProviderError, ProviderResponse,
};
use delve_storage::{
    acquire_session_lock, append_session_event, clear_session_checkpoint, read_session_checkpoint,
    read_session_events, read_session_json, session_file_path, write_session_checkpoint,
    write_session_json, SessionCheckpoint, SessionEvent, SessionEventKind,
};
use serde::Serialize;
use serde_json::json;

const DEFAULT_MAX_AUTO_STEPS: u32 = 5;
const DEFAULT_REVIEW_CONFIDENCE_THRESHOLD: f32 = 0.6;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = if err.use_stderr() {
                StableExitCode::Usage
            } else {
                StableExitCode::Success
            };
            let _ = err.print();
            return code.as_exit_code();
        }
    };

    match run(cli) {
        Ok(()) => StableExitCode::Success.as_exit_code(),
        Err(err) => {
            if !matches!(err, AppError::Interrupted(_)) {
                eprintln!("error: {err}");
            }
            err.exit_code().as_exit_code()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum StableExitCode {
    Success = 0,
    Failure = 1,
    Usage = 2,
    NotFound = 3,
    Conflict = 4,
    InvalidState = 5,
    ProviderFailure = 6,
    Interrupted = 130,
}

impl StableExitCode {
    fn as_exit_code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

#[derive(Debug)]
enum AppError {
    NotFound(String),
    Conflict(String),
    InvalidState(String),
    Provider(ProviderError),
    Interrupted(String),
    Internal(String),
}

impl AppError {
    fn from_io(context: &str, err: io::Error) -> Self {
        let message = format!("{context}: {err}");
        match err.kind() {
            io::ErrorKind::NotFound => Self::NotFound(message),
            io::ErrorKind::AlreadyExists => Self::Conflict(message),
            io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => Self::InvalidState(message),
            _ => Self::Internal(message),
        }
    }

    fn exit_code(&self) -> StableExitCode {
        match self {
            Self::NotFound(_) => StableExitCode::NotFound,
            Self::Conflict(_) => StableExitCode::Conflict,
            Self::InvalidState(_) => StableExitCode::InvalidState,
            Self::Provider(_) => StableExitCode::ProviderFailure,
            Self::Interrupted(_) => StableExitCode::Interrupted,
            Self::Internal(_) => StableExitCode::Failure,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message)
            | Self::Conflict(message)
            | Self::InvalidState(message)
            | Self::Interrupted(message)
            | Self::Internal(message) => f.write_str(message),
            Self::Provider(err) => write!(f, "{err}"),
        }
    }
}

impl Error for AppError {}

impl From<ProviderError> for AppError {
    fn from(value: ProviderError) -> Self {
        Self::Provider(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputMode {
    Text,
    Json,
}

#[derive(Clone, Debug)]
struct RuntimeConfig {
    output_mode: OutputMode,
    quiet: bool,
    no_color: bool,
}

impl RuntimeConfig {
    fn text_line(&self, line: impl AsRef<str>) {
        if self.output_mode == OutputMode::Text && !self.quiet {
            println!("{}", line.as_ref());
        }
    }

    fn text_block(&self, lines: &[String]) {
        if self.output_mode == OutputMode::Text && !self.quiet {
            for line in lines {
                println!("{line}");
            }
        }
    }

    fn emit_json<T: Serialize>(&self, value: &T) -> Result<(), AppError> {
        if self.output_mode == OutputMode::Json {
            let serialized = serde_json::to_string_pretty(value)
                .map_err(|err| AppError::Internal(format!("serialize json output: {err}")))?;
            println!("{serialized}");
        }

        Ok(())
    }

    fn begin_streaming(&self) {
        if self.quiet {
            return;
        }

        if self.output_mode == OutputMode::Text {
            println!("Provider output (streaming):");
        }
    }

    fn stream_chunk(&self, chunk: &str) {
        if self.quiet {
            return;
        }

        let rendered_chunk = if self.no_color {
            strip_ansi_codes(chunk)
        } else {
            chunk.to_string()
        };

        if self.output_mode == OutputMode::Json {
            let _ = io::stderr().write_all(rendered_chunk.as_bytes());
            let _ = io::stderr().flush();
        } else {
            let _ = io::stdout().write_all(rendered_chunk.as_bytes());
            let _ = io::stdout().flush();
        }
    }

    fn end_streaming(&self, emitted_output: &str) {
        if self.quiet || emitted_output.is_empty() {
            return;
        }

        if self.output_mode == OutputMode::Json {
            eprintln!();
        } else {
            println!();
        }
    }
}

fn strip_ansi_codes(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            let _ = chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        output.push(ch);
    }

    output
}

#[derive(Debug, Parser)]
#[command(name = "delve")]
#[command(about = "DelveAI non-interactive and interactive CLI")]
#[command(
    after_help = "Examples:\n  delve session create --intent \"Map rollout plan\" --provider echo\n  delve session continue --session session-123 --prompt \"Refine the architecture\" --provider echo\n  delve session list --json\n  delve artifact accept --artifact artifact-123 --session session-123\n  delve session auto --session session-123 --provider echo --max-steps 3"
)]
struct Cli {
    #[arg(long, global = true, help = "Emit machine-readable JSON output")]
    json: bool,
    #[arg(long, global = true, help = "Suppress non-essential text output")]
    quiet: bool,
    #[arg(long, global = true, help = "Disable ANSI color output")]
    no_color: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },
    Completion(CompletionArgs),
}

#[derive(Debug, Args)]
#[command(about = "Generate shell completion scripts")]
struct CompletionArgs {
    #[arg(long, value_enum)]
    shell: Shell,
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Create(SessionCreateArgs),
    Continue(SessionContinueArgs),
    Show(SessionShowArgs),
    List(SessionListArgs),
    Complete(SessionCompleteArgs),
    Interactive(SessionInteractiveArgs),
    Auto(SessionAutoArgs),
}

#[derive(Debug, Subcommand)]
enum ArtifactCommand {
    Show(ArtifactShowArgs),
    Accept(ArtifactMutateArgs),
    Reject(ArtifactMutateArgs),
}

#[derive(Debug, Args)]
#[command(
    about = "Create a session from an intent and generate the first artifact",
    after_help = "Examples:\n  delve session create --intent \"Design a migration plan\"\n  delve session create --intent \"Draft release note\" --provider echo --json"
)]
struct SessionCreateArgs {
    #[arg(long)]
    intent: String,
    #[arg(long, value_enum, default_value_t = ProviderCli::Amp)]
    provider: ProviderCli,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(
    about = "Continue an existing session with a new prompt",
    after_help = "Examples:\n  delve session continue --session session-123 --prompt \"Add tests\"\n  delve session continue --session session-123 --prompt \"Summarize risk\" --provider echo --json"
)]
struct SessionContinueArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    prompt: String,
    #[arg(long, value_enum, default_value_t = ProviderCli::Amp)]
    provider: ProviderCli,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SessionShowArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SessionListArgs {
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SessionCompleteArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(about = "Open an interactive session launcher")]
struct SessionInteractiveArgs {
    #[arg(long)]
    session: Option<String>,
    #[arg(long, value_enum, default_value_t = ProviderCli::Amp)]
    provider: ProviderCli,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(
    about = "Run auto-interactive orchestration loops with checkpoints",
    after_help = "Examples:\n  delve session auto --session session-123 --provider echo\n  delve session auto --session session-123 --resume --max-steps 10 --no-confirm"
)]
struct SessionAutoArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long, value_enum, default_value_t = ProviderCli::Amp)]
    provider: ProviderCli,
    #[arg(long, default_value_t = DEFAULT_MAX_AUTO_STEPS)]
    max_steps: u32,
    #[arg(long)]
    resume: bool,
    #[arg(long, default_value_t = DEFAULT_REVIEW_CONFIDENCE_THRESHOLD)]
    review_confidence_threshold: f32,
    #[arg(long)]
    no_confirm: bool,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ArtifactShowArgs {
    #[arg(long)]
    artifact: String,
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ArtifactMutateArgs {
    #[arg(long)]
    artifact: String,
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProviderCli {
    Amp,
    Claude,
    Echo,
}

impl ProviderCli {
    fn as_str(self) -> &'static str {
        match self {
            Self::Amp => "amp",
            Self::Claude => "claude",
            Self::Echo => "echo",
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct SessionSummary {
    session_id: String,
    thread_id: String,
    state: SessionState,
    current_node_id: String,
    intent_label: String,
    node_count: usize,
}

#[derive(Debug)]
struct LocatedArtifact {
    session_dir: PathBuf,
    session: SessionTree,
    artifact_node_index: usize,
}

#[derive(Debug)]
struct GeneratedNodes {
    prompt_node_id: NodeId,
    artifact_node_id: NodeId,
    artifact_file_rel: String,
}

#[derive(Debug, Serialize)]
struct SessionCreateOutput {
    session_id: String,
    thread_id: String,
    provider: ProviderCli,
    session_path: String,
    current_node: String,
    prompt_node_id: String,
    artifact_node_id: String,
    artifact_path: String,
    suggested_next_prompt: String,
}

#[derive(Debug, Serialize)]
struct SessionContinueOutput {
    session_id: String,
    thread_id: String,
    provider: ProviderCli,
    new_current_node: String,
    prompt_node_id: String,
    artifact_node_id: String,
    artifact_path: String,
    suggested_next_prompt: String,
}

#[derive(Debug, Serialize)]
struct SessionShowOutput {
    session_id: String,
    thread_id: String,
    state: SessionState,
    current_node: String,
    intent: String,
    node_count: usize,
    event_count: usize,
    session_path: String,
}

#[derive(Debug, Serialize)]
struct SessionCompleteOutput {
    session_id: String,
    state: SessionState,
    session_path: String,
}

#[derive(Debug, Serialize)]
struct ArtifactShowOutput {
    artifact_id: String,
    session_id: String,
    status: NodeStatus,
    kind: Option<ArtifactKind>,
    label: String,
    payload_path: Option<String>,
    payload: Option<String>,
}

#[derive(Debug, Serialize)]
struct ArtifactMutateOutput {
    artifact_id: String,
    session_id: String,
    previous_status: NodeStatus,
    current_status: NodeStatus,
    superseded_siblings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SessionAutoOutput {
    session_id: String,
    thread_id: String,
    steps_executed: u32,
    final_state: SessionState,
    current_node: String,
    completion_detected: bool,
    resumed_from_checkpoint: bool,
}

fn run(cli: Cli) -> Result<(), AppError> {
    let runtime = RuntimeConfig {
        output_mode: if cli.json {
            OutputMode::Json
        } else {
            OutputMode::Text
        },
        quiet: cli.quiet,
        no_color: cli.no_color,
    };

    match cli.command {
        Command::Session { command } => run_session(command, &runtime),
        Command::Artifact { command } => run_artifact(command, &runtime),
        Command::Completion(args) => run_completion(args),
    }
}

fn run_completion(args: CompletionArgs) -> Result<(), AppError> {
    let mut command = Cli::command();
    generate(args.shell, &mut command, "delve", &mut io::stdout());
    io::stdout()
        .flush()
        .map_err(|err| AppError::from_io("flush completion output", err))
}

fn run_session(command: SessionCommand, runtime: &RuntimeConfig) -> Result<(), AppError> {
    match command {
        SessionCommand::Create(args) => run_session_create(args, runtime),
        SessionCommand::Continue(args) => run_session_continue(args, runtime),
        SessionCommand::Show(args) => run_session_show(args, runtime),
        SessionCommand::List(args) => run_session_list(args, runtime),
        SessionCommand::Complete(args) => run_session_complete(args, runtime),
        SessionCommand::Interactive(args) => run_session_interactive(args, runtime),
        SessionCommand::Auto(args) => run_session_auto(args, runtime),
    }
}

fn run_artifact(command: ArtifactCommand, runtime: &RuntimeConfig) -> Result<(), AppError> {
    match command {
        ArtifactCommand::Show(args) => run_artifact_show(args, runtime),
        ArtifactCommand::Accept(args) => run_artifact_mutation(args, NodeStatus::Accepted, runtime),
        ArtifactCommand::Reject(args) => run_artifact_mutation(args, NodeStatus::Rejected, runtime),
    }
}

fn run_session_create(args: SessionCreateArgs, runtime: &RuntimeConfig) -> Result<(), AppError> {
    let intent_text = args.intent;
    let mut session = SessionTree::new(intent_text.clone());
    let session_id = build_session_id();
    session.session_id = SessionId::from(session_id.clone());
    session.thread_id = create_thread_id_for_provider(args.provider)?;

    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_dir = sessions_dir.join(&session_id);

    fs::create_dir_all(&session_dir)
        .map_err(|err| AppError::from_io("create session directory", err))?;
    let _lock = acquire_session_lock(&session_dir)
        .map_err(|err| AppError::from_io("acquire session lock", err))?;

    fs::write(session_dir.join("intent.md"), &intent_text)
        .map_err(|err| AppError::from_io("write intent payload", err))?;

    append_event(
        &session_dir,
        SessionEventKind::SessionCreated,
        &session.session_id,
        Some(session.intent_node_id.clone()),
        json!({"provider":args.provider, "intent":intent_text, "thread_id":session.thread_id.clone()}),
    )?;

    runtime.begin_streaming();
    let artifact_response = execute_provider_prompt_streaming(
        args.provider,
        &session.thread_id,
        &intent_text,
        runtime,
    )?;
    if let Some(thread_id) = artifact_response.thread_id.clone() {
        session.thread_id = thread_id;
    }
    let artifact_output = artifact_response.output;

    let intent_node_id = session.intent_node_id.clone();
    let generated = append_generated_prompt_and_artifact(
        &mut session,
        &session_dir,
        &intent_node_id,
        &intent_text,
        &artifact_output,
    )?;
    session.current_node_id = generated.prompt_node_id.clone();

    validate_session(&session)?;
    write_session_json(&session_dir, &session)
        .map_err(|err| AppError::from_io("write session json", err))?;

    let suggestion = suggest_next_prompt_for_provider(args.provider, &session.thread_id)?;
    if let Some(thread_id) = suggestion.thread_id.clone() {
        session.thread_id = thread_id;
    }
    write_session_json(&session_dir, &session)
        .map_err(|err| AppError::from_io("write session json", err))?;

    append_event(
        &session_dir,
        SessionEventKind::PromptAdded,
        &session.session_id,
        Some(generated.prompt_node_id.clone()),
        json!({"from":"session_create"}),
    )?;
    append_event(
        &session_dir,
        SessionEventKind::ArtifactProposed,
        &session.session_id,
        Some(generated.artifact_node_id.clone()),
        json!({"provider":args.provider, "prompt":intent_text, "thread_id":session.thread_id.clone()}),
    )?;
    append_event(
        &session_dir,
        SessionEventKind::OrchestrationDecision,
        &session.session_id,
        Some(generated.prompt_node_id.clone()),
        json!({
            "stage":"suggest_next_prompt",
            "next_prompt":suggestion.next_prompt.clone(),
            "artifacts":suggestion.artifacts.clone(),
            "thread_id":session.thread_id.clone()
        }),
    )?;

    runtime.text_block(&[
        String::from("session create"),
        format!("Session ID: {}", session.session_id),
        format!("Thread ID: {}", session.thread_id),
        format!("Provider: {:?}", args.provider),
        format!("Session path: {}", session_dir.display()),
        format!("Current node: {}", session.current_node_id),
        format!("Suggested next prompt: {}", suggestion.next_prompt),
    ]);

    runtime.emit_json(&SessionCreateOutput {
        session_id,
        thread_id: session.thread_id.clone(),
        provider: args.provider,
        session_path: session_dir.display().to_string(),
        current_node: session.current_node_id.to_string(),
        prompt_node_id: generated.prompt_node_id.to_string(),
        artifact_node_id: generated.artifact_node_id.to_string(),
        artifact_path: session_dir
            .join(&generated.artifact_file_rel)
            .display()
            .to_string(),
        suggested_next_prompt: suggestion.next_prompt,
    })?;

    Ok(())
}

fn run_session_continue(
    args: SessionContinueArgs,
    runtime: &RuntimeConfig,
) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_dir = sessions_dir.join(&args.session);
    let _lock = acquire_session_lock(&session_dir)
        .map_err(|err| AppError::from_io("acquire session lock", err))?;
    let mut session = read_session_json(&session_dir)
        .map_err(|err| AppError::from_io("load session for continue", err))?;
    ensure_session_thread_id(args.provider, &mut session)?;

    runtime.begin_streaming();
    let artifact_response = execute_provider_prompt_streaming(
        args.provider,
        &session.thread_id,
        &args.prompt,
        runtime,
    )?;
    if let Some(thread_id) = artifact_response.thread_id.clone() {
        session.thread_id = thread_id;
    }
    let artifact_output = artifact_response.output;

    let parent_node_id = session.current_node_id.clone();
    let generated = append_generated_prompt_and_artifact(
        &mut session,
        &session_dir,
        &parent_node_id,
        &args.prompt,
        &artifact_output,
    )?;
    session.current_node_id = generated.prompt_node_id.clone();

    validate_session(&session)?;
    write_session_json(&session_dir, &session)
        .map_err(|err| AppError::from_io("write session json", err))?;

    let suggestion = suggest_next_prompt_for_provider(args.provider, &session.thread_id)?;
    if let Some(thread_id) = suggestion.thread_id.clone() {
        session.thread_id = thread_id;
    }
    write_session_json(&session_dir, &session)
        .map_err(|err| AppError::from_io("write session json", err))?;
    append_event(
        &session_dir,
        SessionEventKind::PromptAdded,
        &session.session_id,
        Some(generated.prompt_node_id.clone()),
        json!({"from":"session_continue"}),
    )?;
    append_event(
        &session_dir,
        SessionEventKind::ArtifactProposed,
        &session.session_id,
        Some(generated.artifact_node_id.clone()),
        json!({"provider":args.provider, "prompt":args.prompt, "thread_id":session.thread_id.clone()}),
    )?;
    append_event(
        &session_dir,
        SessionEventKind::OrchestrationDecision,
        &session.session_id,
        Some(generated.prompt_node_id.clone()),
        json!({
            "stage":"suggest_next_prompt",
            "next_prompt":suggestion.next_prompt.clone(),
            "artifacts":suggestion.artifacts.clone(),
            "thread_id":session.thread_id.clone()
        }),
    )?;

    runtime.text_block(&[
        String::from("session continue"),
        format!("Session ID: {}", session.session_id),
        format!("Thread ID: {}", session.thread_id),
        format!("Provider: {:?}", args.provider),
        format!("New current node: {}", session.current_node_id),
        format!(
            "Artifact path: {}",
            session_dir.join(&generated.artifact_file_rel).display()
        ),
    ]);

    runtime.emit_json(&SessionContinueOutput {
        session_id: session.session_id.to_string(),
        thread_id: session.thread_id.clone(),
        provider: args.provider,
        new_current_node: session.current_node_id.to_string(),
        prompt_node_id: generated.prompt_node_id.to_string(),
        artifact_node_id: generated.artifact_node_id.to_string(),
        artifact_path: session_dir
            .join(&generated.artifact_file_rel)
            .display()
            .to_string(),
        suggested_next_prompt: suggestion.next_prompt,
    })?;

    Ok(())
}

fn run_session_show(args: SessionShowArgs, runtime: &RuntimeConfig) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_dir = sessions_dir.join(&args.session);
    let session = read_session_json(&session_dir)
        .map_err(|err| AppError::from_io("load session for show", err))?;
    let events = read_session_events(&session_dir)
        .map_err(|err| AppError::from_io("load session events", err))?;
    let intent_label = find_node(&session, &session.intent_node_id)
        .map_or("<missing intent node>", |node| node.label.as_str());

    runtime.text_block(&[
        String::from("session show"),
        format!("Session ID: {}", session.session_id),
        format!("Thread ID: {}", session.thread_id),
        format!("State: {:?}", session.state),
        format!("Current node: {}", session.current_node_id),
        format!("Intent: {intent_label}"),
        format!("Node count: {}", session.nodes.len()),
        format!("Event count: {}", events.len()),
        format!("Session path: {}", session_dir.display()),
    ]);

    runtime.emit_json(&SessionShowOutput {
        session_id: session.session_id.to_string(),
        thread_id: session.thread_id.clone(),
        state: session.state,
        current_node: session.current_node_id.to_string(),
        intent: intent_label.to_string(),
        node_count: session.nodes.len(),
        event_count: events.len(),
        session_path: session_dir.display().to_string(),
    })?;

    Ok(())
}

fn run_session_list(args: SessionListArgs, runtime: &RuntimeConfig) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let sessions = load_session_summaries(&sessions_dir)?;

    if runtime.output_mode == OutputMode::Text && !runtime.quiet {
        println!("session list");
        println!("Sessions dir: {}", sessions_dir.display());

        if sessions.is_empty() {
            println!("No sessions found");
        } else {
            for summary in &sessions {
                println!(
                    "{} thread={} [{:?}] current={} nodes={} intent=\"{}\"",
                    summary.session_id,
                    summary.thread_id,
                    summary.state,
                    summary.current_node_id,
                    summary.node_count,
                    summary.intent_label
                );
            }
        }
    }

    runtime.emit_json(&sessions)
}

fn run_session_complete(
    args: SessionCompleteArgs,
    runtime: &RuntimeConfig,
) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_dir = sessions_dir.join(&args.session);
    let _lock = acquire_session_lock(&session_dir)
        .map_err(|err| AppError::from_io("acquire session lock", err))?;

    let mut session = read_session_json(&session_dir)
        .map_err(|err| AppError::from_io("load session for complete", err))?;
    session.state = SessionState::Completed;
    validate_session(&session)?;
    write_session_json(&session_dir, &session)
        .map_err(|err| AppError::from_io("write completed session", err))?;
    clear_session_checkpoint(&session_dir)
        .map_err(|err| AppError::from_io("clear session checkpoint", err))?;

    append_event(
        &session_dir,
        SessionEventKind::SessionCompleted,
        &session.session_id,
        Some(session.current_node_id.clone()),
        json!({"source":"session_complete"}),
    )?;

    runtime.text_block(&[
        String::from("session complete"),
        format!("Session ID: {}", session.session_id),
        format!("State: {:?}", session.state),
        format!("Session path: {}", session_dir.display()),
    ]);

    runtime.emit_json(&SessionCompleteOutput {
        session_id: session.session_id.to_string(),
        state: session.state,
        session_path: session_dir.display().to_string(),
    })?;

    Ok(())
}

fn run_artifact_show(args: ArtifactShowArgs, runtime: &RuntimeConfig) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let artifact_id = NodeId::from(args.artifact);
    let located = resolve_artifact_node(
        &sessions_dir,
        args.session.as_deref(),
        &artifact_id,
        "artifact show",
    )?;

    let artifact = &located.session.nodes[located.artifact_node_index];
    let payload_path = artifact
        .payload_ref
        .as_ref()
        .map(|payload_ref| located.session_dir.join(payload_ref));
    let payload = payload_path
        .as_ref()
        .filter(|path| path.is_file())
        .map(fs::read_to_string)
        .transpose()
        .map_err(|err| AppError::from_io("read artifact payload", err))?;

    if runtime.output_mode == OutputMode::Text && !runtime.quiet {
        println!("artifact show");
        println!("Artifact ID: {}", artifact.id);
        println!("Session ID: {}", located.session.session_id);
        println!("Status: {:?}", artifact.status);
        println!("Kind: {:?}", artifact.artifact_kind);
        println!("Label: {}", artifact.label);
        match &payload_path {
            Some(path) => println!("Payload path: {}", path.display()),
            None => println!("Payload path: <none>"),
        }

        if let Some(payload) = &payload {
            println!("---");
            print!("{payload}");
            if !payload.ends_with('\n') {
                println!();
            }
            println!("---");
        }
    }

    runtime.emit_json(&ArtifactShowOutput {
        artifact_id: artifact.id.to_string(),
        session_id: located.session.session_id.to_string(),
        status: artifact.status,
        kind: artifact.artifact_kind,
        label: artifact.label.clone(),
        payload_path: payload_path.as_ref().map(|path| path.display().to_string()),
        payload,
    })?;

    Ok(())
}

fn run_artifact_mutation(
    args: ArtifactMutateArgs,
    target_status: NodeStatus,
    runtime: &RuntimeConfig,
) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let artifact_id = NodeId::from(args.artifact);
    let located = resolve_artifact_node(
        &sessions_dir,
        args.session.as_deref(),
        &artifact_id,
        "artifact status update",
    )?;

    let _lock = acquire_session_lock(&located.session_dir)
        .map_err(|err| AppError::from_io("acquire session lock", err))?;
    let mut session = read_session_json(&located.session_dir)
        .map_err(|err| AppError::from_io("load session for artifact mutation", err))?;
    let artifact_index = find_artifact_index(&session, &artifact_id).ok_or_else(|| {
        AppError::NotFound(format!(
            "artifact status update: artifact '{}' not found",
            artifact_id
        ))
    })?;

    let current_status = session.nodes[artifact_index].status;
    current_status
        .validate_transition(target_status)
        .map_err(|err| AppError::InvalidState(format!("invalid artifact transition: {err:?}")))?;
    session.nodes[artifact_index].status = target_status;

    let superseded = if target_status == NodeStatus::Accepted {
        supersede_sibling_accepted_artifacts(&mut session, artifact_index)?
    } else {
        Vec::new()
    };

    validate_session(&session)?;
    write_session_json(&located.session_dir, &session)
        .map_err(|err| AppError::from_io("write artifact mutation", err))?;

    let event_kind = match target_status {
        NodeStatus::Accepted => SessionEventKind::ArtifactAccepted,
        NodeStatus::Rejected => SessionEventKind::ArtifactRejected,
        _ => SessionEventKind::OrchestrationDecision,
    };
    append_event(
        &located.session_dir,
        event_kind,
        &session.session_id,
        Some(artifact_id.clone()),
        json!({"previous_status":current_status, "status":target_status}),
    )?;

    if runtime.output_mode == OutputMode::Text && !runtime.quiet {
        let operation_name = match target_status {
            NodeStatus::Accepted => "artifact accept",
            NodeStatus::Rejected => "artifact reject",
            _ => "artifact update",
        };

        println!("{operation_name}");
        println!("Artifact ID: {}", artifact_id);
        println!("Session ID: {}", session.session_id);
        println!("Status: {:?} -> {:?}", current_status, target_status);
        if !superseded.is_empty() {
            let superseded_ids = superseded
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            println!("Superseded siblings: {superseded_ids}");
        }
    }

    runtime.emit_json(&ArtifactMutateOutput {
        artifact_id: artifact_id.to_string(),
        session_id: session.session_id.to_string(),
        previous_status: current_status,
        current_status: target_status,
        superseded_siblings: superseded.iter().map(ToString::to_string).collect(),
    })?;

    Ok(())
}

fn run_session_interactive(
    args: SessionInteractiveArgs,
    runtime: &RuntimeConfig,
) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_id = match args.session {
        Some(session) => session,
        None => pick_recent_session(&sessions_dir)?,
    };
    let session_dir = sessions_dir.join(&session_id);

    runtime.text_line(format!("Launching interactive session '{session_id}'"));

    loop {
        let session = read_session_json(&session_dir)
            .map_err(|err| AppError::from_io("load session for interactive mode", err))?;
        if !runtime.quiet {
            println!();
            println!("Session {} [{:?}]", session.session_id, session.state);
            println!("Tree:");
            for line in render_tree_lines(&session) {
                println!("{line}");
            }
            println!("Actions: [p]rompt [b]rowse artifacts [s]how artifact [a]ccept [r]eject [c]omplete [q]uit");
        }

        let action = prompt_input("interactive> ")?.to_lowercase();
        match action.as_str() {
            "p" | "prompt" => {
                let prompt = compose_prompt()?;
                if !prompt.trim().is_empty() {
                    run_session_continue(
                        SessionContinueArgs {
                            session: session_id.clone(),
                            prompt,
                            provider: args.provider,
                            sessions_dir: Some(sessions_dir.clone()),
                        },
                        runtime,
                    )?;
                }
            }
            "b" | "browse" => {
                render_artifact_panel(&session, runtime);
            }
            "s" | "show" => {
                let artifact = prompt_input("artifact id> ")?;
                run_artifact_show(
                    ArtifactShowArgs {
                        artifact,
                        session: Some(session_id.clone()),
                        sessions_dir: Some(sessions_dir.clone()),
                    },
                    runtime,
                )?;
            }
            "a" | "accept" => {
                let artifact = prompt_input("artifact id> ")?;
                run_artifact_mutation(
                    ArtifactMutateArgs {
                        artifact,
                        session: Some(session_id.clone()),
                        sessions_dir: Some(sessions_dir.clone()),
                    },
                    NodeStatus::Accepted,
                    runtime,
                )?;
            }
            "r" | "reject" => {
                let artifact = prompt_input("artifact id> ")?;
                run_artifact_mutation(
                    ArtifactMutateArgs {
                        artifact,
                        session: Some(session_id.clone()),
                        sessions_dir: Some(sessions_dir.clone()),
                    },
                    NodeStatus::Rejected,
                    runtime,
                )?;
            }
            "c" | "complete" => {
                run_session_complete(
                    SessionCompleteArgs {
                        session: session_id.clone(),
                        sessions_dir: Some(sessions_dir.clone()),
                    },
                    runtime,
                )?;
                break;
            }
            "q" | "quit" => break,
            _ => runtime.text_line("Unknown action"),
        }
    }

    Ok(())
}

fn run_session_auto(args: SessionAutoArgs, runtime: &RuntimeConfig) -> Result<(), AppError> {
    let sessions_dir = args.sessions_dir.unwrap_or_else(default_sessions_dir);
    let session_dir = sessions_dir.join(&args.session);
    let _lock = acquire_session_lock(&session_dir)
        .map_err(|err| AppError::from_io("acquire session lock", err))?;

    let mut session = read_session_json(&session_dir)
        .map_err(|err| AppError::from_io("load session for auto mode", err))?;
    ensure_session_thread_id(args.provider, &mut session)?;
    let resume_state = load_auto_resume_state(&session_dir, &session, args.resume, args.prompt)?;
    session
        .set_current_node(resume_state.current_node_id.clone())
        .map_err(|err| AppError::InvalidState(format!("restore current node: {err:?}")))?;

    let interrupt_controller = InterruptController::default();
    interrupt_controller.install_handler()?;

    let mut step = resume_state.step;
    let mut pending_prompt = resume_state.pending_prompt;
    let mut completion_detected = false;

    while step < args.max_steps {
        if interrupt_controller.should_gracefully_stop() {
            runtime.text_line("Interrupt received. Stopping after current checkpoint.");
            break;
        }

        let prompt = if let Some(existing_prompt) = pending_prompt.take() {
            existing_prompt
        } else {
            let suggestion = suggest_next_prompt_for_provider(args.provider, &session.thread_id)?;
            if let Some(thread_id) = suggestion.thread_id {
                session.thread_id = thread_id;
            }
            suggestion.next_prompt
        };
        write_session_checkpoint(
            &session_dir,
            &SessionCheckpoint::new(
                session.session_id.clone(),
                session.current_node_id.clone(),
                step,
                Some(prompt.clone()),
                json!({"stage":"before_generation"}),
            ),
        )
        .map_err(|err| AppError::from_io("write auto checkpoint", err))?;

        runtime.begin_streaming();
        let artifact_response =
            execute_provider_prompt_streaming(args.provider, &session.thread_id, &prompt, runtime)?;
        if let Some(thread_id) = artifact_response.thread_id.clone() {
            session.thread_id = thread_id;
        }
        let artifact_output = artifact_response.output;

        let parent_node_id = session.current_node_id.clone();
        let generated = append_generated_prompt_and_artifact(
            &mut session,
            &session_dir,
            &parent_node_id,
            &prompt,
            &artifact_output,
        )?;
        session.current_node_id = generated.prompt_node_id.clone();

        let review_rubric = build_review_rubric(&prompt, args.review_confidence_threshold);
        let review_result = execute_review(&review_rubric, &artifact_output);
        let artifact_index = find_artifact_index(&session, &generated.artifact_node_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "generated artifact '{}' missing from session",
                    generated.artifact_node_id
                ))
            })?;
        session.nodes[artifact_index].status = if review_result.accepted {
            NodeStatus::Accepted
        } else {
            NodeStatus::Rejected
        };

        validate_session(&session)?;
        write_session_json(&session_dir, &session)
            .map_err(|err| AppError::from_io("write session in auto mode", err))?;

        append_event(
            &session_dir,
            SessionEventKind::PromptAdded,
            &session.session_id,
            Some(generated.prompt_node_id.clone()),
            json!({"from":"session_auto","step":step}),
        )?;
        append_event(
            &session_dir,
            SessionEventKind::ArtifactProposed,
            &session.session_id,
            Some(generated.artifact_node_id.clone()),
            json!({"provider":args.provider,"step":step,"thread_id":session.thread_id.clone()}),
        )?;
        append_event(
            &session_dir,
            SessionEventKind::OrchestrationDecision,
            &session.session_id,
            Some(generated.artifact_node_id.clone()),
            json!({
                "stage":"review",
                "step":step,
                "accepted":review_result.accepted,
                "confidence":review_result.confidence,
                "missing_keywords":review_result.missing_keywords,
                "thread_id":session.thread_id.clone(),
            }),
        )?;

        let status_event = if review_result.accepted {
            SessionEventKind::ArtifactAccepted
        } else {
            SessionEventKind::ArtifactRejected
        };
        append_event(
            &session_dir,
            status_event,
            &session.session_id,
            Some(generated.artifact_node_id.clone()),
            json!({"step":step}),
        )?;

        completion_detected = detect_completion_signal(&artifact_output);
        if completion_detected {
            session.state = SessionState::Completed;
            write_session_json(&session_dir, &session)
                .map_err(|err| AppError::from_io("write completed auto session", err))?;
            append_event(
                &session_dir,
                SessionEventKind::SessionCompleted,
                &session.session_id,
                Some(session.current_node_id.clone()),
                json!({"source":"session_auto", "step":step}),
            )?;
            clear_session_checkpoint(&session_dir)
                .map_err(|err| AppError::from_io("clear session checkpoint", err))?;
            break;
        }

        let suggestion = suggest_next_prompt_for_provider(args.provider, &session.thread_id)?;
        if let Some(thread_id) = suggestion.thread_id.clone() {
            session.thread_id = thread_id;
        }
        append_event(
            &session_dir,
            SessionEventKind::OrchestrationDecision,
            &session.session_id,
            Some(session.current_node_id.clone()),
            json!({
                "stage":"suggest_next_prompt",
                "step":step,
                "next_prompt":suggestion.next_prompt.clone(),
                "artifacts":suggestion.artifacts.clone(),
                "thread_id":session.thread_id.clone()
            }),
        )?;
        pending_prompt = Some(suggestion.next_prompt);
        step += 1;

        write_session_checkpoint(
            &session_dir,
            &SessionCheckpoint::new(
                session.session_id.clone(),
                session.current_node_id.clone(),
                step,
                pending_prompt.clone(),
                json!({"stage":"after_step"}),
            ),
        )
        .map_err(|err| AppError::from_io("write auto checkpoint", err))?;

        if !args.no_confirm && !prompt_yes_no("Continue auto loop? [y/N] ")? {
            break;
        }
    }

    runtime.emit_json(&SessionAutoOutput {
        session_id: session.session_id.to_string(),
        thread_id: session.thread_id.clone(),
        steps_executed: step,
        final_state: session.state,
        current_node: session.current_node_id.to_string(),
        completion_detected,
        resumed_from_checkpoint: resume_state.resumed,
    })?;

    if runtime.output_mode == OutputMode::Text && !runtime.quiet {
        println!("session auto");
        println!("Session ID: {}", session.session_id);
        println!("Thread ID: {}", session.thread_id);
        println!("Steps executed: {step}");
        println!("State: {:?}", session.state);
        println!("Current node: {}", session.current_node_id);
    }

    if interrupt_controller.should_gracefully_stop() {
        return Err(AppError::Interrupted(String::from(
            "received interrupt and stopped with checkpoint",
        )));
    }

    Ok(())
}

fn load_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>, AppError> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(sessions_dir)
        .map_err(|err| AppError::from_io("list sessions directory", err))?
    {
        let entry = entry.map_err(|err| AppError::from_io("read sessions entry", err))?;
        if !entry
            .file_type()
            .map_err(|err| AppError::from_io("read session entry file type", err))?
            .is_dir()
        {
            continue;
        }

        let session_dir = entry.path();
        let session_file = session_file_path(&session_dir);
        if !session_file.is_file() {
            continue;
        }

        let session = match read_session_json(&session_dir) {
            Ok(session) => session,
            Err(err) => {
                eprintln!(
                    "warning: skipping unreadable session at {}: {}",
                    session_dir.display(),
                    err
                );
                continue;
            }
        };

        let intent_label = find_node(&session, &session.intent_node_id).map_or_else(
            || String::from("<missing intent node>"),
            |node| node.label.clone(),
        );

        sessions.push(SessionSummary {
            session_id: session.session_id.to_string(),
            thread_id: session.thread_id.clone(),
            state: session.state,
            current_node_id: session.current_node_id.to_string(),
            intent_label,
            node_count: session.nodes.len(),
        });
    }

    sessions.sort_by(|left, right| right.session_id.cmp(&left.session_id));
    Ok(sessions)
}

fn find_node<'a>(session: &'a SessionTree, node_id: &NodeId) -> Option<&'a SessionNode> {
    session.nodes.iter().find(|node| node.id == *node_id)
}

fn resolve_artifact_node(
    sessions_dir: &Path,
    session_filter: Option<&str>,
    artifact_id: &NodeId,
    operation: &str,
) -> Result<LocatedArtifact, AppError> {
    if let Some(session_id) = session_filter {
        let session_dir = sessions_dir.join(session_id);
        let session = read_session_json(&session_dir)
            .map_err(|err| AppError::from_io("load session for artifact lookup", err))?;
        let Some(artifact_node_index) = find_artifact_index(&session, artifact_id) else {
            return Err(AppError::NotFound(format!(
                "{operation}: artifact '{}' not found in session '{}'",
                artifact_id, session_id
            )));
        };

        return Ok(LocatedArtifact {
            session_dir,
            session,
            artifact_node_index,
        });
    }

    if !sessions_dir.exists() {
        return Err(AppError::NotFound(format!(
            "{operation}: sessions directory '{}' does not exist",
            sessions_dir.display()
        )));
    }

    let mut found: Option<LocatedArtifact> = None;

    for entry in fs::read_dir(sessions_dir)
        .map_err(|err| AppError::from_io("list sessions directory", err))?
    {
        let entry = entry.map_err(|err| AppError::from_io("read sessions entry", err))?;
        if !entry
            .file_type()
            .map_err(|err| AppError::from_io("read session entry file type", err))?
            .is_dir()
        {
            continue;
        }

        let session_dir = entry.path();
        if !session_file_path(&session_dir).is_file() {
            continue;
        }

        let session = match read_session_json(&session_dir) {
            Ok(session) => session,
            Err(err) => {
                eprintln!(
                    "warning: skipping unreadable session at {}: {}",
                    session_dir.display(),
                    err
                );
                continue;
            }
        };

        let Some(artifact_node_index) = find_artifact_index(&session, artifact_id) else {
            continue;
        };

        if let Some(previous) = &found {
            return Err(AppError::Conflict(format!(
                "{operation}: artifact '{}' exists in multiple sessions ('{}' and '{}'); rerun with --session",
                artifact_id,
                previous.session.session_id,
                session.session_id
            )));
        }

        found = Some(LocatedArtifact {
            session_dir,
            session,
            artifact_node_index,
        });
    }

    found.ok_or_else(|| {
        AppError::NotFound(format!("{operation}: artifact '{}' not found", artifact_id))
    })
}

fn find_artifact_index(session: &SessionTree, artifact_id: &NodeId) -> Option<usize> {
    session
        .nodes
        .iter()
        .position(|node| node.id == *artifact_id && node.kind == NodeKind::Artifact)
}

fn supersede_sibling_accepted_artifacts(
    session: &mut SessionTree,
    artifact_node_index: usize,
) -> Result<Vec<NodeId>, AppError> {
    let target_artifact_id = session.nodes[artifact_node_index].id.clone();
    let parent_prompt_id = session.nodes[artifact_node_index]
        .parent_id
        .clone()
        .ok_or_else(|| {
            AppError::InvalidState(format!(
                "artifact '{}' is missing a parent prompt",
                target_artifact_id
            ))
        })?;

    let mut superseded = Vec::new();

    for node in &mut session.nodes {
        if node.id == target_artifact_id {
            continue;
        }

        if node.kind != NodeKind::Artifact || node.parent_id.as_ref() != Some(&parent_prompt_id) {
            continue;
        }

        if node.status != NodeStatus::Accepted {
            continue;
        }

        node.status
            .validate_transition(NodeStatus::Superseded)
            .map_err(|err| {
                AppError::InvalidState(format!("invalid supersede transition: {err:?}"))
            })?;
        node.status = NodeStatus::Superseded;
        superseded.push(node.id.clone());
    }

    Ok(superseded)
}

fn execute_provider_prompt_streaming(
    provider: ProviderCli,
    thread_id: &str,
    prompt: &str,
    runtime: &RuntimeConfig,
) -> Result<ProviderResponse, AppError> {
    let mut on_chunk = |chunk: &str| {
        runtime.stream_chunk(chunk);
    };

    let response = match provider {
        ProviderCli::Amp => {
            generate_artifact_streaming_with_thread(&AmpProvider, prompt, thread_id, &mut on_chunk)
        }
        ProviderCli::Claude => generate_artifact_streaming_with_thread(
            &ClaudeProvider,
            prompt,
            thread_id,
            &mut on_chunk,
        ),
        ProviderCli::Echo => {
            generate_artifact_streaming_with_thread(&EchoProvider, prompt, thread_id, &mut on_chunk)
        }
    }
    .map_err(AppError::from)?;

    runtime.end_streaming(&response.output);
    Ok(response)
}

fn suggest_next_prompt_for_provider(
    provider: ProviderCli,
    thread_id: &str,
) -> Result<delve_orchestrator::PromptExecutionResult, AppError> {
    match provider {
        ProviderCli::Amp => {
            suggest_next_prompt_with_provider(&AmpProvider, thread_id).map_err(AppError::from)
        }
        ProviderCli::Claude => {
            suggest_next_prompt_with_provider(&ClaudeProvider, thread_id).map_err(AppError::from)
        }
        ProviderCli::Echo => {
            suggest_next_prompt_with_provider(&EchoProvider, thread_id).map_err(AppError::from)
        }
    }
}

fn create_thread_id_for_provider(provider: ProviderCli) -> Result<String, AppError> {
    let provider_thread_id = match provider {
        ProviderCli::Amp => AmpProvider.create_thread().map_err(AppError::from)?,
        ProviderCli::Claude => ClaudeProvider.create_thread().map_err(AppError::from)?,
        ProviderCli::Echo => EchoProvider.create_thread().map_err(AppError::from)?,
    };

    match provider {
        ProviderCli::Amp => provider_thread_id.ok_or_else(|| {
            AppError::InvalidState(String::from(
                "amp provider did not return a thread id from create_thread",
            ))
        }),
        ProviderCli::Claude | ProviderCli::Echo => {
            Ok(provider_thread_id.unwrap_or_else(|| build_local_thread_id(provider)))
        }
    }
}

fn ensure_session_thread_id(
    provider: ProviderCli,
    session: &mut SessionTree,
) -> Result<(), AppError> {
    if !session_thread_id_requires_refresh(provider, &session.thread_id) {
        return Ok(());
    }

    session.thread_id = create_thread_id_for_provider(provider)?;
    Ok(())
}

fn session_thread_id_requires_refresh(provider: ProviderCli, thread_id: &str) -> bool {
    let trimmed = thread_id.trim();
    if trimmed.is_empty() || trimmed == "thread-unset" {
        return true;
    }

    matches!(provider, ProviderCli::Amp) && !looks_like_amp_thread_id(trimmed)
}

fn looks_like_amp_thread_id(thread_id: &str) -> bool {
    if !thread_id.starts_with("T-") {
        return false;
    }

    let value = &thread_id[2..];
    if value.len() != 36 {
        return false;
    }

    value.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

fn append_generated_prompt_and_artifact(
    session: &mut SessionTree,
    session_dir: &Path,
    parent_node_id: &NodeId,
    prompt_text: &str,
    artifact_output: &str,
) -> Result<GeneratedNodes, AppError> {
    let prompt_node_id = NodeId::from(build_node_id("prompt"));
    let artifact_node_id = NodeId::from(build_node_id("artifact"));

    let prompt_file_rel = format!("prompts/{prompt_node_id}/prompt.md");
    let artifact_file_rel = format!("prompts/{prompt_node_id}/artifacts/{artifact_node_id}.md");

    let prompt_dir = session_dir.join("prompts").join(prompt_node_id.as_str());
    fs::create_dir_all(prompt_dir.join("artifacts"))
        .map_err(|err| AppError::from_io("create prompt artifact directory", err))?;
    fs::write(session_dir.join(&prompt_file_rel), prompt_text)
        .map_err(|err| AppError::from_io("write prompt payload", err))?;
    fs::write(session_dir.join(&artifact_file_rel), artifact_output)
        .map_err(|err| AppError::from_io("write artifact payload", err))?;

    let parent_node = session
        .nodes
        .iter_mut()
        .find(|node| node.id == *parent_node_id)
        .ok_or_else(|| {
            AppError::NotFound(format!("current node '{}' not found", parent_node_id))
        })?;
    parent_node.children_ids.push(prompt_node_id.clone());

    let prompt_node = SessionNode {
        id: prompt_node_id.clone(),
        label: build_label("prompt", prompt_text),
        kind: NodeKind::Prompt,
        artifact_kind: None,
        status: NodeStatus::Accepted,
        parent_id: Some(parent_node_id.clone()),
        children_ids: vec![artifact_node_id.clone()],
        input_node_ids: Vec::new(),
        payload_ref: Some(prompt_file_rel),
    };

    let artifact_node = SessionNode {
        id: artifact_node_id.clone(),
        label: build_label("artifact", artifact_output),
        kind: NodeKind::Artifact,
        artifact_kind: Some(ArtifactKind::Implementation),
        status: NodeStatus::Proposed,
        parent_id: Some(prompt_node_id.clone()),
        children_ids: Vec::new(),
        input_node_ids: Vec::new(),
        payload_ref: Some(artifact_file_rel.clone()),
    };

    session.nodes.push(prompt_node);
    session.nodes.push(artifact_node);

    Ok(GeneratedNodes {
        prompt_node_id,
        artifact_node_id,
        artifact_file_rel,
    })
}

fn validate_session(session: &SessionTree) -> Result<(), AppError> {
    session
        .validate_tree_invariants()
        .map_err(|err| AppError::InvalidState(format!("tree invariant failed: {err:?}")))
}

fn append_event(
    session_dir: &Path,
    event_kind: SessionEventKind,
    session_id: &SessionId,
    node_id: Option<NodeId>,
    metadata: serde_json::Value,
) -> Result<(), AppError> {
    let event = SessionEvent::new(event_kind, session_id.clone(), node_id, metadata);
    append_session_event(session_dir, &event)
        .map_err(|err| AppError::from_io("append session event", err))
}

fn build_session_id() -> String {
    let epoch_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());

    format!("session-{epoch_millis}")
}

fn build_node_id(prefix: &str) -> String {
    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());

    format!("{prefix}-{epoch_nanos}")
}

fn build_local_thread_id(provider: ProviderCli) -> String {
    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("thread-{}-{epoch_nanos}", provider.as_str())
}

fn build_label(prefix: &str, value: &str) -> String {
    let slug = value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-");

    let short_slug = if slug.is_empty() {
        String::from("item")
    } else {
        slug
    };

    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_micros())
        % 100_000_000;

    format!("{prefix}-{short_slug}-{token:08}")
}

fn default_sessions_dir() -> PathBuf {
    PathBuf::from(".delve/sessions")
}

fn pick_recent_session(sessions_dir: &Path) -> Result<String, AppError> {
    let summaries = load_session_summaries(sessions_dir)?;
    if summaries.is_empty() {
        return Err(AppError::NotFound(format!(
            "no sessions found in '{}'",
            sessions_dir.display()
        )));
    }

    println!("Recent sessions:");
    for (idx, summary) in summaries.iter().take(10).enumerate() {
        println!(
            "  {}: {} [{:?}] {}",
            idx + 1,
            summary.session_id,
            summary.state,
            summary.intent_label
        );
    }

    let choice = prompt_input("Select session number> ")?;
    let index = choice
        .trim()
        .parse::<usize>()
        .map_err(|_| AppError::InvalidState(String::from("invalid session picker input")))?;
    let selected = summaries.get(index.saturating_sub(1)).ok_or_else(|| {
        AppError::InvalidState(String::from("selected session index is out of range"))
    })?;

    Ok(selected.session_id.clone())
}

fn prompt_input(prompt: &str) -> Result<String, AppError> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|err| AppError::from_io("flush prompt", err))?;

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|err| AppError::from_io("read stdin", err))?;
    Ok(line.trim().to_string())
}

fn compose_prompt() -> Result<String, AppError> {
    println!("Enter prompt text. Finish with a single '.' on its own line.");
    let mut lines = Vec::new();
    loop {
        let line = prompt_input("prompt> ")?;
        if line == "." {
            break;
        }
        lines.push(line);
    }

    Ok(lines.join("\n"))
}

fn render_tree_lines(session: &SessionTree) -> Vec<String> {
    fn walk(session: &SessionTree, node_id: &NodeId, depth: usize, lines: &mut Vec<String>) {
        if let Some(node) = find_node(session, node_id) {
            lines.push(format!(
                "{}- {} [{:?}/{:?}] {}",
                "  ".repeat(depth),
                node.id,
                node.kind,
                node.status,
                node.label
            ));
            for child in &node.children_ids {
                walk(session, child, depth + 1, lines);
            }
        }
    }

    let mut lines = Vec::new();
    walk(session, &session.intent_node_id, 0, &mut lines);
    lines
}

fn render_artifact_panel(session: &SessionTree, runtime: &RuntimeConfig) {
    if runtime.output_mode != OutputMode::Text || runtime.quiet {
        return;
    }

    println!("Artifacts:");
    for node in &session.nodes {
        if node.kind == NodeKind::Artifact {
            println!(
                "  {} [{:?}] parent={} label=\"{}\"",
                node.id,
                node.status,
                node.parent_id
                    .as_ref()
                    .map_or("<none>", |parent| parent.as_str()),
                node.label
            );
        }
    }
}

fn build_review_rubric(
    prompt: &str,
    confidence_threshold: f32,
) -> delve_orchestrator::ReviewRubric {
    let mut required_keywords = prompt
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 5)
        .take(3)
        .collect::<Vec<_>>();

    required_keywords.sort();
    required_keywords.dedup();

    delve_orchestrator::ReviewRubric {
        required_keywords,
        confidence_threshold,
    }
}

fn detect_completion_signal(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("[complete]")
        || lower.contains("intent complete")
        || lower.contains("done: true")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoResumeState {
    step: u32,
    current_node_id: NodeId,
    pending_prompt: Option<String>,
    resumed: bool,
}

fn load_auto_resume_state(
    session_dir: &Path,
    session: &SessionTree,
    resume: bool,
    explicit_prompt: Option<String>,
) -> Result<AutoResumeState, AppError> {
    if !resume {
        return Ok(AutoResumeState {
            step: 0,
            current_node_id: session.current_node_id.clone(),
            pending_prompt: explicit_prompt,
            resumed: false,
        });
    }

    let checkpoint = read_session_checkpoint(session_dir)
        .map_err(|err| AppError::from_io("read session checkpoint", err))?;
    let Some(checkpoint) = checkpoint else {
        return Ok(AutoResumeState {
            step: 0,
            current_node_id: session.current_node_id.clone(),
            pending_prompt: explicit_prompt,
            resumed: false,
        });
    };

    if checkpoint.session_id != session.session_id {
        return Err(AppError::InvalidState(format!(
            "checkpoint belongs to '{}' but session is '{}'",
            checkpoint.session_id, session.session_id
        )));
    }

    Ok(AutoResumeState {
        step: checkpoint.step,
        current_node_id: checkpoint.current_node_id,
        pending_prompt: explicit_prompt.or(checkpoint.pending_prompt),
        resumed: true,
    })
}

fn prompt_yes_no(prompt: &str) -> Result<bool, AppError> {
    let answer = prompt_input(prompt)?;
    let normalized = answer.to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

#[derive(Debug, Clone)]
struct InterruptController {
    interrupt_count: Arc<AtomicUsize>,
    graceful_stop: Arc<AtomicBool>,
}

impl Default for InterruptController {
    fn default() -> Self {
        Self {
            interrupt_count: Arc::new(AtomicUsize::new(0)),
            graceful_stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl InterruptController {
    fn install_handler(&self) -> Result<(), AppError> {
        let counter = Arc::clone(&self.interrupt_count);
        let graceful_stop = Arc::clone(&self.graceful_stop);

        ctrlc::set_handler(move || {
            let previous = counter.fetch_add(1, Ordering::SeqCst);
            if previous == 0 {
                graceful_stop.store(true, Ordering::SeqCst);
                eprintln!("\nReceived Ctrl+C. Finishing current step and checkpointing.");
                return;
            }

            eprintln!("\nReceived second Ctrl+C. Exiting immediately.");
            process::exit(StableExitCode::Interrupted as i32);
        })
        .map_err(|err| AppError::Internal(format!("install Ctrl+C handler: {err}")))
    }

    fn should_gracefully_stop(&self) -> bool {
        self.graceful_stop.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    fn simulate_interrupt(&self) -> bool {
        let previous = self.interrupt_count.fetch_add(1, Ordering::SeqCst);
        if previous == 0 {
            self.graceful_stop.store(true, Ordering::SeqCst);
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use clap::Parser;
    use delve_domain::{NodeId, SessionId, SessionTree};
    use delve_storage::write_session_checkpoint;
    use serde_json::json;

    use super::{
        load_auto_resume_state, load_session_summaries, looks_like_amp_thread_id,
        session_thread_id_requires_refresh, ArtifactCommand, Cli, Command, InterruptController,
        ProviderCli, SessionCheckpoint, SessionCommand, SessionState,
    };

    #[test]
    fn parses_session_create_command_layout() {
        let cli = Cli::try_parse_from(["delve", "session", "create", "--intent", "Ship V1"])
            .expect("session create should parse");

        assert!(matches!(
            cli.command,
            Command::Session {
                command: SessionCommand::Create(_)
            }
        ));
    }

    #[test]
    fn parses_artifact_accept_command_layout() {
        let cli = Cli::try_parse_from([
            "delve",
            "artifact",
            "accept",
            "--artifact",
            "artifact-implementation-1",
        ])
        .expect("artifact accept should parse");

        assert!(matches!(
            cli.command,
            Command::Artifact {
                command: ArtifactCommand::Accept(_)
            }
        ));
    }

    #[test]
    fn loads_session_summaries_sorted_by_id_desc() {
        let test_dir = unique_test_dir("summaries");
        let sessions_dir = test_dir.join("sessions");

        std::fs::create_dir_all(&sessions_dir).expect("sessions dir should be created");

        let mut older = SessionTree::new("Older intent");
        older.session_id = SessionId::from("session-100");
        older.state = SessionState::Completed;

        let mut newer = SessionTree::new("Newer intent");
        newer.session_id = SessionId::from("session-200");
        newer.state = SessionState::Active;

        let older_dir = sessions_dir.join(older.session_id.as_str());
        let newer_dir = sessions_dir.join(newer.session_id.as_str());
        std::fs::create_dir_all(&older_dir).expect("older session dir should be created");
        std::fs::create_dir_all(&newer_dir).expect("newer session dir should be created");
        delve_storage::write_session_json(&older_dir, &older)
            .expect("older session should persist");
        delve_storage::write_session_json(&newer_dir, &newer)
            .expect("newer session should persist");

        let summaries = load_session_summaries(&sessions_dir).expect("summaries should load");
        assert_eq!(summaries[0].session_id, "session-200");
        assert_eq!(summaries[1].session_id, "session-100");
    }

    #[test]
    fn auto_resume_prefers_explicit_prompt_over_checkpoint_prompt() {
        let test_dir = unique_test_dir("resume");
        let session_dir = test_dir.join("session-1");
        std::fs::create_dir_all(&session_dir).expect("session directory should be created");

        let mut session = SessionTree::new("Intent");
        session.session_id = SessionId::from("session-1");

        write_session_checkpoint(
            &session_dir,
            &SessionCheckpoint::new(
                SessionId::from("session-1"),
                NodeId::from("prompt-3"),
                4,
                Some(String::from("checkpoint prompt")),
                json!({}),
            ),
        )
        .expect("checkpoint should be written");

        let state = load_auto_resume_state(
            &session_dir,
            &session,
            true,
            Some(String::from("explicit prompt")),
        )
        .expect("resume state should load");

        assert!(state.resumed);
        assert_eq!(state.step, 4);
        assert_eq!(state.current_node_id, NodeId::from("prompt-3"));
        assert_eq!(state.pending_prompt, Some(String::from("explicit prompt")));
    }

    #[test]
    fn interrupt_controller_tracks_first_and_second_interrupt() {
        let controller = InterruptController::default();
        assert!(!controller.should_gracefully_stop());

        assert!(!controller.simulate_interrupt());
        assert!(controller.should_gracefully_stop());
        assert!(controller.simulate_interrupt());
    }

    #[test]
    fn amp_thread_id_shape_validation_matches_expected_format() {
        assert!(looks_like_amp_thread_id(
            "T-12345678-1234-1234-1234-1234567890ab"
        ));
        assert!(!looks_like_amp_thread_id("thread-amp-legacy"));
        assert!(!looks_like_amp_thread_id("T-not-a-uuid"));
    }

    #[test]
    fn amp_provider_refreshes_missing_or_legacy_thread_ids() {
        assert!(session_thread_id_requires_refresh(ProviderCli::Amp, ""));
        assert!(session_thread_id_requires_refresh(
            ProviderCli::Amp,
            "thread-amp-legacy"
        ));
        assert!(!session_thread_id_requires_refresh(
            ProviderCli::Amp,
            "T-12345678-1234-1234-1234-1234567890ab"
        ));
        assert!(!session_thread_id_requires_refresh(
            ProviderCli::Echo,
            "thread-echo-123"
        ));
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let epoch_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        env::temp_dir().join(format!("delve-cli-tests-{label}-{epoch_nanos}"))
    }
}
