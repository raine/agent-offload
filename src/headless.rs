use crate::config::{AgentInterface, Profile, PromptDelivery};
use crate::monitor::{CompactRenderer, RenderedTail, RendererKind};
use crate::run_dir;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub fn run_headless(profile_name: &str, profile: &Profile, prompt: &str) -> Result<i32> {
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
    let headless_run =
        if signal.captures_stdout() || matches!(profile.prompt, PromptDelivery::PromptFileArg) {
            Some(run_dir::create()?)
        } else {
            None
        };

    let mut args: Vec<String> = interface_headless_flags(profile.interface)
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    args.extend(profile.args.iter().cloned());

    if let Some(run_dir) = headless_run.as_ref() {
        fs::write(&run_dir.prompt_file, prompt).context("could not write prompt file")?;
    }

    if matches!(profile.prompt, PromptDelivery::PromptFileArg) {
        let run_dir = headless_run
            .as_ref()
            .context("headless run directory was not created")?;
        let prompt_file = run_dir.prompt_file.to_string_lossy().to_string();
        for arg in args.iter_mut() {
            if arg.contains("{prompt_file}") {
                *arg = arg.replace("{prompt_file}", &prompt_file);
            }
        }
    }

    cmd.args(&args);

    if signal.captures_stdout() {
        cmd.stdout(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit());
    }

    let mut recorder = if signal.captures_stdout() {
        let run_dir = headless_run
            .as_ref()
            .context("headless run directory was not created")?;
        let recorder = HeadlessRunRecorder::new(
            run_dir.metadata_file.clone(),
            profile_name,
            profile,
            &args,
            Utc::now(),
        );
        recorder.write()?;
        Some(recorder)
    } else {
        None
    };

    let stdout_log_file = if signal.captures_stdout() {
        Some(
            headless_run
                .as_ref()
                .context("headless run directory was not created")?
                .stdout_file
                .clone(),
        )
    } else {
        None
    };

    let result = match spawn_and_wait(
        cmd,
        profile.prompt,
        prompt,
        signal,
        stdout_log_file.as_deref(),
        |pid| {
            if let Some(recorder) = recorder.as_mut() {
                recorder.record_pid(pid)?;
            }
            Ok(())
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            if let Some(recorder) = recorder.as_mut() {
                let _ = recorder.fail_without_exit(format!("{error:#}"));
            }
            return Err(error);
        }
    };
    let missing_completion = signal.requires_event() && !result.saw_completion;
    let missing_completion_failure = missing_completion.then(|| {
        format!(
            "headless {} agent exited without a completion event",
            signal.name()
        )
    });
    let failure = missing_completion_failure
        .clone()
        .or_else(|| result.completion_failure.clone());
    if let Some(recorder) = recorder.as_mut() {
        recorder.finish(
            &result.status,
            result.saw_completion,
            signal.requires_event(),
            failure,
        )?;
    }
    let exit_code = result.status.code().unwrap_or(1);
    if let Some(failure) = missing_completion_failure {
        if exit_code == 0 {
            bail!(failure);
        }
        eprintln!("{failure}");
    } else if let Some(failure) = result.completion_failure.as_deref()
        && exit_code == 0
    {
        bail!(failure.to_string());
    }

    if let Some(stdout_log_file) = &stdout_log_file {
        print_rendered_tail(&result.rendered, stdout_log_file)?;
    }

    Ok(exit_code)
}

struct HeadlessRun {
    status: ExitStatus,
    saw_completion: bool,
    completion_failure: Option<String>,
    rendered: RenderedTail,
}

#[derive(Serialize)]
struct HeadlessRunMetadata {
    profile: HeadlessRunProfileMetadata,
    interface: String,
    prompt_delivery: String,
    started_at: String,
    completed_at: Option<String>,
    status: String,
    exit_code: Option<i32>,
    completion_event_seen: Option<bool>,
    failure: Option<String>,
    pid: Option<u32>,
}

#[derive(Serialize)]
struct HeadlessRunProfileMetadata {
    name: String,
    command: String,
    args: Vec<String>,
}

struct HeadlessRunRecorder {
    metadata_file: PathBuf,
    metadata: HeadlessRunMetadata,
}

impl HeadlessRunRecorder {
    fn new(
        metadata_file: PathBuf,
        profile_name: &str,
        profile: &Profile,
        args: &[String],
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            metadata_file,
            metadata: HeadlessRunMetadata {
                profile: HeadlessRunProfileMetadata {
                    name: profile_name.to_string(),
                    command: profile.command.clone(),
                    args: args.to_vec(),
                },
                pid: None,
                interface: interface_name(profile.interface).to_string(),
                prompt_delivery: prompt_delivery_name(profile.prompt).to_string(),
                started_at: started_at.to_rfc3339(),
                completed_at: None,
                status: "running".to_string(),
                exit_code: None,
                completion_event_seen: None,
                failure: None,
            },
        }
    }

    fn write(&self) -> Result<()> {
        let tmp = self.metadata_file.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.metadata)
            .context("could not serialize headless run metadata")?;
        fs::write(&tmp, bytes).with_context(|| format!("could not write {}", tmp.display()))?;
        fs::rename(&tmp, &self.metadata_file)
            .with_context(|| format!("could not replace {}", self.metadata_file.display()))?;
        Ok(())
    }

    fn record_pid(&mut self, pid: u32) -> Result<()> {
        self.metadata.pid = Some(pid);
        self.write()
    }

    fn finish(
        &mut self,
        status: &ExitStatus,
        saw_completion: bool,
        requires_event: bool,
        failure: Option<String>,
    ) -> Result<()> {
        self.metadata.completed_at = Some(Utc::now().to_rfc3339());
        self.metadata.exit_code = Some(status.code().unwrap_or(1));
        self.metadata.completion_event_seen = requires_event.then_some(saw_completion);
        self.metadata.failure = failure;
        self.metadata.status = if status.success() && self.metadata.failure.is_none() {
            "success".to_string()
        } else {
            "failed".to_string()
        };
        self.write()
    }

    fn fail_without_exit(&mut self, failure: String) -> Result<()> {
        self.metadata.completed_at = Some(Utc::now().to_rfc3339());
        self.metadata.status = "failed".to_string();
        self.metadata.exit_code = None;
        self.metadata.completion_event_seen = None;
        self.metadata.failure = Some(failure);
        self.write()
    }
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
    fn captures_stdout(self) -> bool {
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

    fn line_is_completion(self, value: &Value) -> bool {
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

    fn line_failure(self, value: &Value) -> Option<String> {
        match self {
            CompletionSignal::ClaudeResult | CompletionSignal::CursorResult => {
                let subtype = str_field(value, "subtype")?;
                (subtype != "success").then(|| format!("result subtype {subtype}"))
            }
            CompletionSignal::CodexTurnFinished => {
                (str_field(value, "type") == Some("turn.failed")).then(|| {
                    value
                        .get("error")
                        .and_then(|error| str_field(error, "message"))
                        .unwrap_or("turn failed")
                        .to_string()
                })
            }
            CompletionSignal::Exit | CompletionSignal::OpencodeJsonExit => None,
        }
    }

    fn renderer(self) -> RendererKind {
        match self {
            CompletionSignal::ClaudeResult => RendererKind::Claude,
            CompletionSignal::CodexTurnFinished => RendererKind::Codex,
            CompletionSignal::CursorResult => RendererKind::Cursor,
            CompletionSignal::OpencodeJsonExit => RendererKind::Opencode,
            CompletionSignal::Exit => RendererKind::Raw,
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

fn interface_name(interface: AgentInterface) -> &'static str {
    match interface {
        AgentInterface::Claude => "claude",
        AgentInterface::Codex => "codex",
        AgentInterface::Cursor => "cursor",
        AgentInterface::Opencode => "opencode",
        AgentInterface::Generic => "generic",
    }
}

fn prompt_delivery_name(prompt: PromptDelivery) -> &'static str {
    match prompt {
        PromptDelivery::Argument => "argument",
        PromptDelivery::Stdin => "stdin",
        PromptDelivery::PromptFileArg => "prompt-file-arg",
    }
}

fn spawn_and_wait(
    mut cmd: Command,
    prompt_delivery: PromptDelivery,
    prompt: &str,
    signal: CompletionSignal,
    log_file: Option<&Path>,
    mut on_spawn: impl FnMut(u32) -> Result<()>,
) -> Result<HeadlessRun> {
    match prompt_delivery {
        PromptDelivery::Stdin => {
            let mut child = cmd
                .stdin(Stdio::piped())
                .spawn()
                .context("could not spawn headless agent")?;

            on_spawn(child.id())?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .context("could not write prompt to agent stdin")?;
            }

            wait_with_stdout(child, signal, log_file)
        }
        PromptDelivery::Argument => {
            let child = cmd
                .stdin(Stdio::null())
                .arg(prompt)
                .spawn()
                .context("could not spawn headless agent")?;
            on_spawn(child.id())?;
            wait_with_stdout(child, signal, log_file)
        }
        PromptDelivery::PromptFileArg => {
            let child = cmd
                .stdin(Stdio::null())
                .spawn()
                .context("could not spawn headless agent")?;
            on_spawn(child.id())?;
            wait_with_stdout(child, signal, log_file)
        }
    }
}

fn wait_with_stdout(
    mut child: std::process::Child,
    signal: CompletionSignal,
    log_file: Option<&Path>,
) -> Result<HeadlessRun> {
    let stream_result = if let Some(stdout) = child.stdout.take() {
        stream_stdout(stdout, signal, log_file)
    } else {
        Ok((false, None, RenderedTail::default()))
    };

    let status = child.wait().context("could not wait for headless agent")?;
    let (saw_completion, completion_failure, rendered) = match stream_result {
        Ok(result) => result,
        Err(error) => (
            false,
            Some(format!("could not record headless stdout: {error:#}")),
            RenderedTail::default(),
        ),
    };

    Ok(HeadlessRun {
        status,
        saw_completion,
        completion_failure,
        rendered,
    })
}

fn stream_stdout(
    stdout: impl std::io::Read,
    signal: CompletionSignal,
    log_file: Option<&Path>,
) -> Result<(bool, Option<String>, RenderedTail)> {
    let mut saw_completion = false;
    let mut completion_failure = None;
    let mut renderer = CompactRenderer::new(signal.renderer());
    let mut rendered = RenderedTail::default();
    let mut log = match log_file {
        Some(path) => Some(
            fs::File::create(path)
                .with_context(|| format!("could not create {}", path.display()))?,
        ),
        None => None,
    };

    for line in BufReader::new(stdout).lines() {
        let line = line.context("could not read headless agent stdout")?;
        if let Some(log) = log.as_mut() {
            log.write_all(format!("{line}\n").as_bytes())
                .context("could not write headless stdout log")?;
        }

        let rendered_line = renderer.render_jsonl_line(&line);
        if let Some(value) = &rendered_line.value
            && signal.line_is_completion(value)
        {
            saw_completion = true;
            if completion_failure.is_none() {
                completion_failure = signal.line_failure(value);
            }
        }
        for line in rendered_line.lines {
            rendered.push(line);
        }
    }

    Ok((saw_completion, completion_failure, rendered))
}

fn print_rendered_tail(rendered: &RenderedTail, log_file: &Path) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    if rendered.omitted > 0 {
        writeln!(stdout, "... {} earlier lines omitted", rendered.omitted)
            .context("could not write headless transcript")?;
    }
    for line in &rendered.lines {
        writeln!(stdout, "{line}").context("could not write headless transcript")?;
    }
    writeln!(stdout).context("could not write headless transcript")?;
    writeln!(stdout, "Full log: {}", log_file.display())
        .context("could not write headless log path")?;
    Ok(())
}

fn interface_headless_flags(interface: AgentInterface) -> &'static [&'static str] {
    match interface {
        AgentInterface::Claude => &["-p", "--output-format", "stream-json", "--verbose"],
        AgentInterface::Cursor => &["--print", "--output-format", "stream-json", "--trust"],
        AgentInterface::Codex => &["exec", "--json"],
        AgentInterface::Opencode => &["run", "--format", "json"],
        AgentInterface::Generic => &[],
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as JsonValue;

    fn json(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn test_claude_headless_flags() {
        assert_eq!(
            interface_headless_flags(AgentInterface::Claude),
            &["-p", "--output-format", "stream-json", "--verbose"]
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
        let value = json(r#"{"type":"result","subtype":"success"}"#);
        assert!(CompletionSignal::ClaudeResult.line_is_completion(&value));
    }

    #[test]
    fn test_cursor_result_line_is_completion() {
        let value = json(r#"{"type":"result","subtype":"success"}"#);
        assert!(CompletionSignal::CursorResult.line_is_completion(&value));
    }

    #[test]
    fn test_codex_turn_completed_line_is_completion() {
        let value = json(r#"{"type":"turn.completed","usage":{}}"#);
        assert!(CompletionSignal::CodexTurnFinished.line_is_completion(&value));
    }

    #[test]
    fn test_codex_turn_failed_line_is_completion() {
        let value = json(r#"{"type":"turn.failed","error":{}}"#);
        assert!(CompletionSignal::CodexTurnFinished.line_is_completion(&value));
    }

    #[test]
    fn test_opencode_json_does_not_require_terminal_event() {
        assert!(!CompletionSignal::OpencodeJsonExit.requires_event());
    }

    #[test]
    fn test_other_json_line_is_not_completion() {
        let value = json(r#"{"type":"assistant"}"#);
        assert!(!CompletionSignal::ClaudeResult.line_is_completion(&value));
    }

    #[test]
    fn test_headless_recorder_writes_running_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        let profile = Profile {
            command: "agent".to_string(),
            args: vec!["--json".to_string()],
            env: Default::default(),
            interface: AgentInterface::Claude,
            prompt: PromptDelivery::Argument,
            headless: true,
        };
        let mut recorder = HeadlessRunRecorder::new(
            dir.path().join("metadata.json"),
            "test-profile",
            &profile,
            &profile.args,
            DateTime::parse_from_rfc3339("2026-06-09T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );

        recorder.write().unwrap();
        recorder.record_pid(1234).unwrap();
        let value: JsonValue =
            serde_json::from_str(&fs::read_to_string(dir.path().join("metadata.json")).unwrap())
                .unwrap();

        assert_eq!(value["profile"]["name"], "test-profile");
        assert_eq!(value["profile"]["command"], "agent");
        assert_eq!(value["interface"], "claude");
        assert_eq!(value["prompt_delivery"], "argument");
        assert_eq!(value["pid"], 1234);
        assert_eq!(value["status"], "running");
        assert_eq!(value["started_at"], "2026-06-09T00:00:00+00:00");
        assert!(value["exit_code"].is_null());
    }

    #[test]
    fn test_headless_recorder_finish_records_success() {
        let dir = tempfile::TempDir::new().unwrap();
        let profile = Profile {
            command: "agent".to_string(),
            args: vec![],
            env: Default::default(),
            interface: AgentInterface::Claude,
            prompt: PromptDelivery::Argument,
            headless: true,
        };
        let mut recorder = HeadlessRunRecorder::new(
            dir.path().join("metadata.json"),
            "test-profile",
            &profile,
            &profile.args,
            Utc::now(),
        );
        recorder.write().unwrap();

        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        };
        #[cfg(not(unix))]
        let status = { std::process::Command::new("true").status().unwrap() };

        recorder.finish(&status, true, true, None).unwrap();
        let value: JsonValue =
            serde_json::from_str(&fs::read_to_string(dir.path().join("metadata.json")).unwrap())
                .unwrap();

        assert_eq!(value["status"], "success");
        assert_eq!(value["exit_code"], 0);
        assert_eq!(value["completion_event_seen"], true);
        assert!(value["completed_at"].is_string());
    }

    #[test]
    fn test_headless_recorder_fail_without_exit_records_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let profile = Profile {
            command: "agent".to_string(),
            args: vec![],
            env: Default::default(),
            interface: AgentInterface::Claude,
            prompt: PromptDelivery::Argument,
            headless: true,
        };
        let mut recorder = HeadlessRunRecorder::new(
            dir.path().join("metadata.json"),
            "test-profile",
            &profile,
            &profile.args,
            Utc::now(),
        );
        recorder.write().unwrap();

        recorder
            .fail_without_exit("could not spawn agent".to_string())
            .unwrap();
        let value: JsonValue =
            serde_json::from_str(&fs::read_to_string(dir.path().join("metadata.json")).unwrap())
                .unwrap();

        assert_eq!(value["status"], "failed");
        assert!(value["exit_code"].is_null());
        assert_eq!(value["failure"], "could not spawn agent");
        assert!(value["completed_at"].is_string());
    }

    #[test]
    fn test_codex_turn_failed_records_completion_failure() {
        let input = br#"{"type":"turn.failed","error":{"message":"model stopped"}}
"#;

        let (saw_completion, completion_failure, _) =
            stream_stdout(&input[..], CompletionSignal::CodexTurnFinished, None).unwrap();

        assert!(saw_completion);
        assert_eq!(completion_failure.as_deref(), Some("model stopped"));
    }

    #[test]
    fn test_stream_stdout_writes_appendable_jsonl_log() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("stdout.jsonl");
        let input = br#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}
{"type":"result","subtype":"success"}
"#;

        let (saw_completion, completion_failure, rendered) =
            stream_stdout(&input[..], CompletionSignal::ClaudeResult, Some(&log_file)).unwrap();
        let log = fs::read_to_string(&log_file).unwrap();

        assert!(saw_completion);
        assert!(completion_failure.is_none());
        assert_eq!(log.lines().count(), 2);
        assert!(log.ends_with('\n'));
        assert_eq!(
            rendered.lines.back().unwrap(),
            "[turn]  ok  turns=0  dur=0.0s  in=0  out=0  cost=$0.0000"
        );
    }
}
