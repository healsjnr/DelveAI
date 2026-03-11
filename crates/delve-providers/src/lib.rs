#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::io::Read;
use std::process::Command;
use std::process::Stdio;
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResponse {
    pub output: String,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoProvider;

impl CompletionProvider for EchoProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            output: format!("echo:{}", request.prompt),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmpProvider;

impl CompletionProvider for AmpProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        run_command_provider(
            ProviderKind::Amp,
            "amp",
            ["--no-color", "-x", request.prompt.as_str()],
        )
    }

    fn generate_streaming(
        &self,
        request: &ProviderRequest,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<ProviderResponse, ProviderError> {
        run_command_provider_streaming(
            ProviderKind::Amp,
            "amp",
            ["--no-color", "-x", request.prompt.as_str()],
            on_chunk,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeProvider;

impl CompletionProvider for ClaudeProvider {
    fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        run_command_provider(
            ProviderKind::Claude,
            "claude",
            ["-p", "--output-format", "text", request.prompt.as_str()],
        )
    }

    fn generate_streaming(
        &self,
        request: &ProviderRequest,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<ProviderResponse, ProviderError> {
        run_command_provider_streaming(
            ProviderKind::Claude,
            "claude",
            ["-p", "--output-format", "text", request.prompt.as_str()],
            on_chunk,
        )
    }
}

fn run_command_provider<const N: usize>(
    provider: ProviderKind,
    binary: &str,
    args: [&str; N],
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
    })
}

fn run_command_provider_streaming<const N: usize>(
    provider: ProviderKind,
    binary: &str,
    args: [&str; N],
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
    })
}

#[cfg(test)]
mod tests {
    use super::{CompletionProvider, EchoProvider, ProviderRequest};

    #[test]
    fn echo_provider_returns_wrapped_prompt() {
        let provider = EchoProvider;
        let response = provider
            .generate(&ProviderRequest {
                prompt: String::from("hello"),
            })
            .expect("echo provider should never fail");

        assert_eq!(response.output, "echo:hello");
    }

    #[test]
    fn echo_provider_streaming_calls_chunk_handler() {
        let provider = EchoProvider;
        let mut chunks = String::new();
        let response = provider
            .generate_streaming(
                &ProviderRequest {
                    prompt: String::from("stream"),
                },
                &mut |chunk| chunks.push_str(chunk),
            )
            .expect("echo streaming should never fail");

        assert_eq!(response.output, "echo:stream");
        assert_eq!(chunks, "echo:stream");
    }
}
