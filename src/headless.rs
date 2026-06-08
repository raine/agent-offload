use crate::config::{AgentInterface, Profile, PromptDelivery};
use crate::run_dir;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitStatus, Stdio};

pub fn run_headless(profile: &Profile, prompt: &str) -> Result<i32> {
    let mut cmd = Command::new(&profile.command);
    cmd.stderr(Stdio::inherit());

    for (key, value) in &profile.env {
        match value {
            crate::config::EnvValue::Literal(value) => {
                cmd.env(key, value);
            }
            crate::config::EnvValue::FromEnv(from_env) => {
                let resolved = std::env::var(&from_env.from_env).with_context(|| {
                    format!("{} is not set in the environment", from_env.from_env)
                })?;
                cmd.env(key, resolved);
            }
        }
    }

    let signal = completion_signal(profile.interface);
    let mut args: Vec<String> = interface_headless_flags(profile.interface)
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    args.extend(profile.args.iter().cloned());

    if matches!(profile.prompt, PromptDelivery::PromptFileArg) {
        let run_dir = run_dir::create()?;
        fs::write(&run_dir.prompt_file, prompt).context("could not write prompt file")?;
        let prompt_file = run_dir.prompt_file.to_string_lossy().to_string();
        for arg in args.iter_mut() {
            if arg.contains("{prompt_file}") {
                *arg = arg.replace("{prompt_file}", &prompt_file);
            }
        }
    }

    cmd.args(&args);

    if signal.requires_stdout() {
        cmd.stdout(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit());
    }

    let result = spawn_and_wait(cmd, profile.prompt, prompt, signal)?;
    if signal.requires_event() && !result.saw_completion {
        bail!(
            "headless {} agent exited without a completion event",
            signal.name()
        );
    }

    Ok(result.status.code().unwrap_or(1))
}

struct HeadlessRun {
    status: ExitStatus,
    saw_completion: bool,
}

#[derive(Clone, Copy)]
enum CompletionSignal {
    Exit,
    ClaudeResult,
    CodexTurnFinished,
    CursorResult,
    OpencodeJsonExit,
}

impl CompletionSignal {
    fn requires_stdout(self) -> bool {
        !matches!(self, CompletionSignal::Exit)
    }

    fn requires_event(self) -> bool {
        !matches!(
            self,
            CompletionSignal::Exit | CompletionSignal::OpencodeJsonExit
        )
    }

    fn name(self) -> &'static str {
        match self {
            CompletionSignal::Exit => "generic",
            CompletionSignal::ClaudeResult => "Claude",
            CompletionSignal::CodexTurnFinished => "Codex",
            CompletionSignal::CursorResult => "Cursor",
            CompletionSignal::OpencodeJsonExit => "opencode",
        }
    }

    fn line_is_completion(self, line: &str) -> bool {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };

        match self {
            CompletionSignal::ClaudeResult | CompletionSignal::CursorResult => {
                value.get("type").and_then(Value::as_str) == Some("result")
            }
            CompletionSignal::CodexTurnFinished => matches!(
                value.get("type").and_then(Value::as_str),
                Some("turn.completed" | "turn.failed")
            ),
            CompletionSignal::Exit | CompletionSignal::OpencodeJsonExit => false,
        }
    }
}

fn completion_signal(interface: AgentInterface) -> CompletionSignal {
    match interface {
        AgentInterface::Claude => CompletionSignal::ClaudeResult,
        AgentInterface::Codex => CompletionSignal::CodexTurnFinished,
        AgentInterface::Cursor => CompletionSignal::CursorResult,
        AgentInterface::Opencode => CompletionSignal::OpencodeJsonExit,
        AgentInterface::Generic => CompletionSignal::Exit,
    }
}

fn spawn_and_wait(
    mut cmd: Command,
    prompt_delivery: PromptDelivery,
    prompt: &str,
    signal: CompletionSignal,
) -> Result<HeadlessRun> {
    match prompt_delivery {
        PromptDelivery::Stdin => {
            let mut child = cmd
                .stdin(Stdio::piped())
                .spawn()
                .context("could not spawn headless agent")?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .context("could not write prompt to agent stdin")?;
            }

            wait_with_stdout(child, signal)
        }
        PromptDelivery::Argument => {
            let child = cmd
                .stdin(Stdio::null())
                .arg(prompt)
                .spawn()
                .context("could not spawn headless agent")?;
            wait_with_stdout(child, signal)
        }
        PromptDelivery::PromptFileArg => {
            let child = cmd
                .stdin(Stdio::null())
                .spawn()
                .context("could not spawn headless agent")?;
            wait_with_stdout(child, signal)
        }
    }
}

fn wait_with_stdout(
    mut child: std::process::Child,
    signal: CompletionSignal,
) -> Result<HeadlessRun> {
    let saw_completion = if let Some(stdout) = child.stdout.take() {
        stream_stdout(stdout, signal)?
    } else {
        false
    };

    let status = child.wait().context("could not wait for headless agent")?;
    Ok(HeadlessRun {
        status,
        saw_completion,
    })
}

fn stream_stdout(stdout: impl std::io::Read, signal: CompletionSignal) -> Result<bool> {
    let mut saw_completion = false;
    let mut stdout_writer = std::io::stdout().lock();

    for line in BufReader::new(stdout).lines() {
        let line = line.context("could not read headless agent stdout")?;
        writeln!(stdout_writer, "{line}").context("could not write headless agent stdout")?;
        saw_completion |= signal.line_is_completion(&line);
    }

    Ok(saw_completion)
}

fn interface_headless_flags(interface: AgentInterface) -> &'static [&'static str] {
    match interface {
        AgentInterface::Claude => &["-p", "--output-format", "stream-json"],
        AgentInterface::Cursor => &["--print", "--output-format", "stream-json", "--trust"],
        AgentInterface::Codex => &["exec", "--json"],
        AgentInterface::Opencode => &["run", "--format", "json"],
        AgentInterface::Generic => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_headless_flags() {
        assert_eq!(
            interface_headless_flags(AgentInterface::Claude),
            &["-p", "--output-format", "stream-json"]
        );
    }

    #[test]
    fn test_codex_headless_flags() {
        assert_eq!(
            interface_headless_flags(AgentInterface::Codex),
            &["exec", "--json"]
        );
    }

    #[test]
    fn test_opencode_headless_flags() {
        assert_eq!(
            interface_headless_flags(AgentInterface::Opencode),
            &["run", "--format", "json"]
        );
    }

    #[test]
    fn test_generic_headless_flags() {
        assert!(interface_headless_flags(AgentInterface::Generic).is_empty());
    }

    #[test]
    fn test_cursor_headless_flags() {
        assert_eq!(
            interface_headless_flags(AgentInterface::Cursor),
            &["--print", "--output-format", "stream-json", "--trust"]
        );
    }

    #[test]
    fn test_claude_result_line_is_completion() {
        assert!(
            CompletionSignal::ClaudeResult
                .line_is_completion(r#"{"type":"result","subtype":"success"}"#)
        );
    }

    #[test]
    fn test_cursor_result_line_is_completion() {
        assert!(
            CompletionSignal::CursorResult
                .line_is_completion(r#"{"type":"result","subtype":"success"}"#)
        );
    }

    #[test]
    fn test_codex_turn_completed_line_is_completion() {
        assert!(
            CompletionSignal::CodexTurnFinished
                .line_is_completion(r#"{"type":"turn.completed","usage":{}}"#)
        );
    }

    #[test]
    fn test_codex_turn_failed_line_is_completion() {
        assert!(
            CompletionSignal::CodexTurnFinished
                .line_is_completion(r#"{"type":"turn.failed","error":{}}"#)
        );
    }

    #[test]
    fn test_opencode_json_does_not_require_terminal_event() {
        assert!(!CompletionSignal::OpencodeJsonExit.requires_event());
    }

    #[test]
    fn test_other_json_line_is_not_completion() {
        assert!(!CompletionSignal::ClaudeResult.line_is_completion(r#"{"type":"assistant"}"#));
    }

    #[test]
    fn test_non_json_line_is_not_completion() {
        assert!(!CompletionSignal::ClaudeResult.line_is_completion("done"));
    }
}
