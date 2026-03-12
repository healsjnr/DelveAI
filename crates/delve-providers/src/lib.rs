#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::io::Read;
use std::process::Command;
use std::process::Stdio;
use std::thread;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequest {
    pub prompt: String,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResponse {
    pub output: String,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Echo,
    Amp,
    Claude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    CommandFailed {
        provider: ProviderKind,
        status_code: Option<i32>,
        stderr: String,
    },
    CommandExecutionFailed {
        provider: ProviderKind,
        error_message: String,
    },
    MissingThreadId {
        provider: ProviderKind,
    },
    ThreadIdParseFailed {
        provider: ProviderKind,
        output: String,
    },
    StreamJsonParseFailed {
        provider: ProviderKind,
        line: String,
        error_message: String,
    },
    MissingResult {
        provider: ProviderKind,
        output: String,
    },
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed {
                provider,
                status_code,
                stderr,
            } => {
                write!(
                    f,
                    "provider {:?} failed with status {:?}: {}",
                    provider, status_code, stderr
                )
            }
            Self::CommandExecutionFailed {
                provider,
                error_message,
            } => {
                write!(
                    f,
                    "provider {:?} command failed to execute: {}",
                    provider, error_message
                )
            }
            Self::MissingThreadId { provider } => {
                write!(f, "provider {:?} requires a thread id", provider)
            }
            Self::ThreadIdParseFailed { provider, output } => {
                write!(
                    f,
                    "provider {:?} did not return a parseable thread id: {}",
                    provider, output
                )
            }
            Self::StreamJsonParseFailed {
                provider,
                line,
                error_message,
            } => {
                write!(
                    f,
                    "provider {:?} returned invalid stream-json line '{}': {}",
                    provider, line, error_message
                )
            }
            Self::MissingResult { provider, output } => {
                write!(
                    f,
                    "provider {:?} stream-json response missing final result: {}",
                    provider, output
                )
            }
        }
    }
}

impl Error for ProviderError {}

pub trait CompletionProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError>;

    fn generate_streaming(
        &self,
        request: &ProviderRequest,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<ProviderResponse, ProviderError> {
        let response = self.generate(request)?;
        on_chunk(&response.output);
        Ok(response)
    }

    fn create_thread(&self) -> Result<Option<String>, ProviderError> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoProvider;

impl CompletionProvider for EchoProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            output: format!("echo:{}", request.prompt),
            thread_id: request.thread_id.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmpProvider;

impl CompletionProvider for AmpProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let thread_id = request
            .thread_id
            .as_deref()
            .ok_or(ProviderError::MissingThreadId {
                provider: ProviderKind::Amp,
            })?;
        let args = build_amp_continue_args(thread_id, &request.prompt);
        let mut noop_on_chunk = |_chunk: &str| {};
        let response = run_amp_stream_json(ProviderKind::Amp, "amp", &args, &mut noop_on_chunk)?;
        Ok(ProviderResponse {
            output: response.output,
            thread_id: Some(thread_id.to_string()),
        })
    }

    fn generate_streaming(
        &self,
        request: &ProviderRequest,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<ProviderResponse, ProviderError> {
        let thread_id = request
            .thread_id
            .as_deref()
            .ok_or(ProviderError::MissingThreadId {
                provider: ProviderKind::Amp,
            })?;
        let args = build_amp_continue_args(thread_id, &request.prompt);
        let response = run_amp_stream_json(ProviderKind::Amp, "amp", &args, on_chunk)?;
        Ok(ProviderResponse {
            output: response.output,
            thread_id: Some(thread_id.to_string()),
        })
    }

    fn create_thread(&self) -> Result<Option<String>, ProviderError> {
        let args = vec![String::from("threads"), String::from("new")];
        let response = run_command_provider(ProviderKind::Amp, "amp", &args)?;
        let thread_id = extract_thread_id(&response.output).ok_or_else(|| {
            ProviderError::ThreadIdParseFailed {
                provider: ProviderKind::Amp,
                output: response.output.clone(),
            }
        })?;

        Ok(Some(thread_id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeProvider;

impl CompletionProvider for ClaudeProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let args = vec![
            String::from("-p"),
            String::from("--output-format"),
            String::from("text"),
            request.prompt.clone(),
        ];
        let response = run_command_provider(ProviderKind::Claude, "claude", &args)?;
        Ok(ProviderResponse {
            output: response.output,
            thread_id: request.thread_id.clone(),
        })
    }

    fn generate_streaming(
        &self,
        request: &ProviderRequest,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<ProviderResponse, ProviderError> {
        let args = vec![
            String::from("-p"),
            String::from("--output-format"),
            String::from("text"),
            request.prompt.clone(),
        ];
        let response =
            run_command_provider_streaming(ProviderKind::Claude, "claude", &args, on_chunk)?;
        Ok(ProviderResponse {
            output: response.output,
            thread_id: request.thread_id.clone(),
        })
    }
}

fn build_amp_continue_args(thread_id: &str, prompt: &str) -> Vec<String> {
    vec![
        String::from("--no-color"),
        String::from("threads"),
        String::from("continue"),
        thread_id.to_string(),
        String::from("-x"),
        prompt.to_string(),
        String::from("--stream-json"),
    ]
}

fn run_amp_stream_json(
    provider: ProviderKind,
    binary: &str,
    args: &[String],
    on_chunk: &mut dyn FnMut(&str),
) -> Result<ProviderResponse, ProviderError> {
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: err.to_string(),
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: String::from("unable to capture provider stdout"),
        })?;

    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: String::from("unable to capture provider stderr"),
        })?;

    let stderr_handle = thread::spawn(move || {
        let mut stderr_text = String::new();
        let _ = stderr.read_to_string(&mut stderr_text);
        stderr_text
    });

    let mut pending_buffer = String::new();
    let mut raw_output = String::new();
    let mut final_result = None;
    let mut buffer = [0_u8; 2048];

    loop {
        let read_count =
            stdout
                .read(&mut buffer)
                .map_err(|err| ProviderError::CommandExecutionFailed {
                    provider: provider.clone(),
                    error_message: err.to_string(),
                })?;

        if read_count == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buffer[..read_count]);
        pending_buffer.push_str(&chunk);

        while let Some(newline_index) = pending_buffer.find('\n') {
            let mut line = pending_buffer.drain(..=newline_index).collect::<String>();
            while matches!(line.chars().last(), Some('\n' | '\r')) {
                let _ = line.pop();
            }

            parse_amp_stream_json_line(
                &provider,
                &line,
                on_chunk,
                &mut final_result,
                &mut raw_output,
            )?;
        }
    }

    if !pending_buffer.trim().is_empty() {
        parse_amp_stream_json_line(
            &provider,
            pending_buffer.trim(),
            on_chunk,
            &mut final_result,
            &mut raw_output,
        )?;
    }

    let status = child
        .wait()
        .map_err(|err| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: err.to_string(),
        })?;

    let stderr_text = stderr_handle.join().unwrap_or_default();
    if !status.success() {
        return Err(ProviderError::CommandFailed {
            provider,
            status_code: status.code(),
            stderr: stderr_text.trim().to_string(),
        });
    }

    let output = final_result.ok_or_else(|| ProviderError::MissingResult {
        provider,
        output: raw_output.trim().to_string(),
    })?;

    Ok(ProviderResponse {
        output,
        thread_id: None,
    })
}

fn parse_amp_stream_json_line(
    provider: &ProviderKind,
    line: &str,
    on_chunk: &mut dyn FnMut(&str),
    final_result: &mut Option<String>,
    raw_output: &mut String,
) -> Result<(), ProviderError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if !raw_output.is_empty() {
        raw_output.push('\n');
    }
    raw_output.push_str(trimmed);

    let payload: Value =
        serde_json::from_str(trimmed).map_err(|err| ProviderError::StreamJsonParseFailed {
            provider: provider.clone(),
            line: trimmed.to_string(),
            error_message: err.to_string(),
        })?;

    let entry_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if entry_type == "assistant" {
        emit_assistant_text_chunks(&payload, on_chunk);
    }

    if entry_type == "result" {
        let result_value = payload
            .get("result")
            .ok_or_else(|| ProviderError::MissingResult {
                provider: provider.clone(),
                output: raw_output.trim().to_string(),
            })?;
        let rendered_result = if let Some(result_text) = result_value.as_str() {
            result_text.to_string()
        } else {
            result_value.to_string()
        };
        *final_result = Some(rendered_result);
    }

    Ok(())
}

fn emit_assistant_text_chunks(payload: &Value, on_chunk: &mut dyn FnMut(&str)) {
    let Some(content_entries) = payload
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for content_entry in content_entries {
        if let Some(text) = content_entry.get("text").and_then(Value::as_str) {
            on_chunk(text);
        }
    }
}

fn run_command_provider(
    provider: ProviderKind,
    binary: &str,
    args: &[String],
) -> Result<ProviderResponse, ProviderError> {
    let output = Command::new(binary).args(args).output().map_err(|err| {
        ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: err.to_string(),
        }
    })?;

    if !output.status.success() {
        return Err(ProviderError::CommandFailed {
            provider,
            status_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(ProviderResponse {
        output: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        thread_id: None,
    })
}

fn run_command_provider_streaming(
    provider: ProviderKind,
    binary: &str,
    args: &[String],
    on_chunk: &mut dyn FnMut(&str),
) -> Result<ProviderResponse, ProviderError> {
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: err.to_string(),
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: String::from("unable to capture provider stdout"),
        })?;

    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: String::from("unable to capture provider stderr"),
        })?;

    let stderr_handle = thread::spawn(move || {
        let mut stderr_text = String::new();
        let _ = stderr.read_to_string(&mut stderr_text);
        stderr_text
    });

    let mut output = String::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let read_count =
            stdout
                .read(&mut buffer)
                .map_err(|err| ProviderError::CommandExecutionFailed {
                    provider: provider.clone(),
                    error_message: err.to_string(),
                })?;

        if read_count == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buffer[..read_count]);
        on_chunk(&chunk);
        output.push_str(&chunk);
    }

    let status = child
        .wait()
        .map_err(|err| ProviderError::CommandExecutionFailed {
            provider: provider.clone(),
            error_message: err.to_string(),
        })?;

    let stderr_text = stderr_handle.join().unwrap_or_default();
    if !status.success() {
        return Err(ProviderError::CommandFailed {
            provider,
            status_code: status.code(),
            stderr: stderr_text.trim().to_string(),
        });
    }

    Ok(ProviderResponse {
        output: output.trim().to_string(),
        thread_id: None,
    })
}

fn extract_thread_id(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-'))
        .find(|token| looks_like_thread_id(token))
        .map(ToOwned::to_owned)
}

fn looks_like_thread_id(token: &str) -> bool {
    if !token.starts_with("T-") {
        return false;
    }

    let value = &token[2..];
    if value.len() != 36 {
        return false;
    }

    value.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

#[cfg(test)]
mod tests {
    use super::{
        extract_thread_id, looks_like_thread_id, parse_amp_stream_json_line, CompletionProvider,
        EchoProvider, ProviderError, ProviderKind, ProviderRequest,
    };

    #[test]
    fn echo_provider_returns_wrapped_prompt() {
        let provider = EchoProvider;
        let response = provider
            .generate(&ProviderRequest {
                prompt: String::from("hello"),
                thread_id: Some(String::from("thread-123")),
            })
            .expect("echo provider should never fail");

        assert_eq!(response.output, "echo:hello");
        assert_eq!(response.thread_id.as_deref(), Some("thread-123"));
    }

    #[test]
    fn echo_provider_streaming_calls_chunk_handler() {
        let provider = EchoProvider;
        let mut chunks = String::new();
        let response = provider
            .generate_streaming(
                &ProviderRequest {
                    prompt: String::from("stream"),
                    thread_id: Some(String::from("thread-456")),
                },
                &mut |chunk| chunks.push_str(chunk),
            )
            .expect("echo streaming should never fail");

        assert_eq!(response.output, "echo:stream");
        assert_eq!(response.thread_id.as_deref(), Some("thread-456"));
        assert_eq!(chunks, "echo:stream");
    }

    #[test]
    fn thread_id_parser_extracts_amp_thread_tokens() {
        let output = "Created thread T-12345678-1234-1234-1234-1234567890ab";
        let thread_id = extract_thread_id(output).expect("thread id should parse");
        assert_eq!(thread_id, "T-12345678-1234-1234-1234-1234567890ab");
        assert!(looks_like_thread_id(&thread_id));
    }

    #[test]
    fn amp_stream_json_parser_emits_assistant_text_and_extracts_result() {
        let mut streamed_text = String::new();
        let mut final_result = None;
        let mut raw_output = String::new();

        parse_amp_stream_json_line(
            &ProviderKind::Amp,
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"text\":\"First \"},{\"text\":\"second\"}]}}",
            &mut |chunk| streamed_text.push_str(chunk),
            &mut final_result,
            &mut raw_output,
        )
        .expect("assistant message should parse");
        parse_amp_stream_json_line(
            &ProviderKind::Amp,
            "{\"type\":\"result\",\"result\":\"Final artifact\"}",
            &mut |_chunk| {},
            &mut final_result,
            &mut raw_output,
        )
        .expect("result message should parse");

        assert_eq!(streamed_text, "First second");
        assert_eq!(final_result.as_deref(), Some("Final artifact"));
    }

    #[test]
    fn amp_stream_json_parser_rejects_invalid_json_lines() {
        let mut final_result = None;
        let mut raw_output = String::new();

        let error = parse_amp_stream_json_line(
            &ProviderKind::Amp,
            "not-json",
            &mut |_chunk| {},
            &mut final_result,
            &mut raw_output,
        )
        .expect_err("invalid line should fail");

        assert!(matches!(
            error,
            ProviderError::StreamJsonParseFailed {
                provider: ProviderKind::Amp,
                ..
            }
        ));
    }
}
