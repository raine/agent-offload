use crate::config::{AgentInterface, Profile, PromptDelivery};
use crate::run_dir;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

const RENDER_TAIL_LINES: usize = 30;
const TRUNC_DEFAULT: usize = 500;

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

    if matches!(profile.prompt, PromptDelivery::PromptFileArg) {
        let run_dir = headless_run
            .as_ref()
            .context("headless run directory was not created")?;
        fs::write(&run_dir.prompt_file, prompt).context("could not write prompt file")?;
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
    if let Some(failure) = missing_completion_failure {
        bail!(failure);
    }

    if let Some(stdout_log_file) = &stdout_log_file {
        print_rendered_tail(&result.rendered, stdout_log_file)?;
    }

    Ok(result.status.code().unwrap_or(1))
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

            wait_with_stdout(child, signal, log_file)
        }
        PromptDelivery::Argument => {
            let child = cmd
                .stdin(Stdio::null())
                .arg(prompt)
                .spawn()
                .context("could not spawn headless agent")?;
            wait_with_stdout(child, signal, log_file)
        }
        PromptDelivery::PromptFileArg => {
            let child = cmd
                .stdin(Stdio::null())
                .spawn()
                .context("could not spawn headless agent")?;
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
    let (saw_completion, completion_failure, rendered) = stream_result?;

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
    let mut renderer = HeadlessRenderer::new(signal.renderer());
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

        match serde_json::from_str::<Value>(&line) {
            Ok(value) => {
                if signal.line_is_completion(&value) {
                    saw_completion = true;
                    if completion_failure.is_none() {
                        completion_failure = signal.line_failure(&value);
                    }
                }
                for line in renderer.render_value(&value) {
                    rendered.push(line);
                }
            }
            Err(_) => rendered.push(format!("[raw]   {}", truncate(&line, TRUNC_DEFAULT))),
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

#[derive(Default)]
struct RenderedTail {
    lines: VecDeque<String>,
    omitted: usize,
}

impl RenderedTail {
    fn push(&mut self, line: String) {
        if self.lines.len() == RENDER_TAIL_LINES {
            self.lines.pop_front();
            self.omitted += 1;
        }
        self.lines.push_back(line);
    }
}

#[derive(Clone, Copy)]
enum RendererKind {
    Claude,
    Codex,
    Cursor,
    Opencode,
    Raw,
}

#[derive(Default)]
struct RenderState {
    tool_num_by_id: HashMap<String, usize>,
    tool_name_by_id: HashMap<String, String>,
    next_tool_num: usize,
}

impl RenderState {
    fn tag_for_tool(&mut self, id: &str, name: Option<&str>) -> String {
        let num = match self.tool_num_by_id.get(id) {
            Some(num) => *num,
            None => {
                self.next_tool_num += 1;
                self.tool_num_by_id
                    .insert(id.to_string(), self.next_tool_num);
                self.next_tool_num
            }
        };
        if let Some(name) = name {
            self.tool_name_by_id
                .insert(id.to_string(), name.to_string());
        }
        let name = name
            .map(str::to_string)
            .or_else(|| self.tool_name_by_id.get(id).cloned())
            .unwrap_or_else(|| "tool".to_string());
        format!("{name}#{num:02}")
    }
}

struct HeadlessRenderer {
    kind: RendererKind,
    state: RenderState,
}

impl HeadlessRenderer {
    fn new(kind: RendererKind) -> Self {
        Self {
            kind,
            state: RenderState::default(),
        }
    }

    fn render_value(&mut self, value: &Value) -> Vec<String> {
        match self.kind {
            RendererKind::Claude => render_claude(value, &mut self.state),
            RendererKind::Codex => render_codex(value),
            RendererKind::Cursor => render_cursor(value, &mut self.state),
            RendererKind::Opencode => render_opencode(value),
            RendererKind::Raw => vec![format!(
                "[json]  {}",
                truncate(&value.to_string(), TRUNC_DEFAULT)
            )],
        }
    }
}

fn render_claude(value: &Value, state: &mut RenderState) -> Vec<String> {
    match str_field(value, "type") {
        Some("system") if str_field(value, "subtype") == Some("init") => {
            let model = str_field(value, "model").unwrap_or("?");
            let session = str_field(value, "session_id").unwrap_or("");
            let cwd = str_field(value, "cwd").unwrap_or("?");
            let permission = str_field(value, "permissionMode").unwrap_or("?");
            vec![format!(
                "[init]  model={model}  session={}  perm={permission}  cwd={cwd}",
                short_id(session)
            )]
        }
        Some("assistant") => render_claude_assistant(value, state),
        Some("user") => render_claude_user(value, state),
        Some("result") => vec![render_claude_result(value)],
        Some(other) => vec![format!(
            "[sdk:{other}]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
        None => vec![format!(
            "[json]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
    }
}

fn render_claude_assistant(value: &Value, state: &mut RenderState) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return lines;
    };

    for block in content {
        match str_field(block, "type") {
            Some("text") => {
                if let Some(text) = str_field(block, "text")
                    && !text.trim().is_empty()
                {
                    lines.push(format!("[text]  {}", truncate(text, TRUNC_DEFAULT)));
                }
            }
            Some("thinking") => {
                let text = str_field(block, "thinking").unwrap_or("");
                lines.push(format!("[think] {}", truncate(text, TRUNC_DEFAULT)));
            }
            Some("redacted_thinking") => lines.push("[think] <redacted>".to_string()),
            Some("tool_use") => {
                let id = str_field(block, "id").unwrap_or("");
                let name = str_field(block, "name").unwrap_or("tool");
                let tag = state.tag_for_tool(id, Some(name));
                let args = format_tool_input(block.get("input"));
                if args.is_empty() {
                    lines.push(format!("[tool→] {tag}"));
                } else {
                    lines.push(format!("[tool→] {tag}  {args}"));
                }
            }
            Some(other) => lines.push(format!(
                "[block:{other}]  {}",
                truncate(&block.to_string(), TRUNC_DEFAULT)
            )),
            None => lines.push(format!(
                "[block:?]  {}",
                truncate(&block.to_string(), TRUNC_DEFAULT)
            )),
        }
    }

    lines
}

fn render_claude_user(value: &Value, state: &mut RenderState) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return lines;
    };

    for block in content {
        if str_field(block, "type") != Some("tool_result") {
            continue;
        }
        let id = str_field(block, "tool_use_id").unwrap_or("");
        let tag = state.tag_for_tool(id, None);
        let text = tool_result_text(block.get("content"));
        if block.get("is_error").and_then(Value::as_bool) == Some(true) {
            lines.push(format!("[tool✗] {tag}  error: {}", truncate(&text, 400)));
        } else {
            lines.push(format!(
                "[tool✓] {tag}  ok ({} chars){}",
                text.len(),
                preview_suffix(&text, 200)
            ));
        }
    }

    lines
}

fn render_claude_result(value: &Value) -> String {
    let subtype = str_field(value, "subtype").unwrap_or("?");
    let status = if subtype == "success" { "ok" } else { subtype };
    let turns = value.get("num_turns").and_then(Value::as_i64).unwrap_or(0);
    let duration_ms = value
        .get("duration_ms")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let cost = value
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let usage = value.get("usage");
    let input = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    format!(
        "[turn]  {status}  turns={turns}  dur={:.1}s  in={}  out={}  cost=${cost:.4}",
        duration_ms / 1000.0,
        format_tokens(input),
        format_tokens(output)
    )
}

fn render_cursor(value: &Value, state: &mut RenderState) -> Vec<String> {
    match str_field(value, "type") {
        Some("assistant") => {
            if let Some(text) = str_field(value, "message").or_else(|| str_field(value, "text")) {
                vec![format!("[text]  {}", truncate(text, TRUNC_DEFAULT))]
            } else {
                render_claude_assistant(value, state)
            }
        }
        Some("tool_call") => {
            let id = str_field(value, "id")
                .or_else(|| str_field(value, "tool_call_id"))
                .unwrap_or("");
            let name = str_field(value, "name")
                .or_else(|| str_field(value, "tool"))
                .unwrap_or("tool");
            let tag = state.tag_for_tool(id, Some(name));
            match str_field(value, "subtype") {
                Some("started") => vec![format!(
                    "[tool→] {tag}  {}",
                    format_tool_input(value.get("input"))
                )],
                Some("completed") => vec![format!("[tool✓] {tag}  completed")],
                Some("failed") => vec![format!("[tool✗] {tag}  failed")],
                other => vec![format!("[tool]  {tag}  subtype={}", other.unwrap_or("?"))],
            }
        }
        Some("result") => vec![render_cursor_result(value)],
        _ => render_claude(value, state),
    }
}

fn render_cursor_result(value: &Value) -> String {
    let subtype = str_field(value, "subtype").unwrap_or("?");
    let status = if subtype == "success" { "ok" } else { subtype };
    format!("[turn]  {status}")
}

fn render_codex(value: &Value) -> Vec<String> {
    match str_field(value, "type") {
        Some("thread.started") => vec![format!(
            "[init]  thread={}",
            short_id(str_field(value, "thread_id").unwrap_or(""))
        )],
        Some("turn.started") => vec!["[turn→] started".to_string()],
        Some("turn.completed") => vec![render_codex_turn(value, "ok")],
        Some("turn.failed") => vec![render_codex_turn(value, "failed")],
        Some("item.started") => value
            .get("item")
            .map(|item| render_codex_item(item, "item→"))
            .into_iter()
            .collect(),
        Some("item.updated") => value
            .get("item")
            .map(|item| render_codex_item(item, "item…"))
            .into_iter()
            .collect(),
        Some("item.completed") => value
            .get("item")
            .map(|item| render_codex_item(item, "item✓"))
            .into_iter()
            .collect(),
        Some("error") => vec![format!(
            "[error] {}",
            truncate(
                str_field(value, "message").unwrap_or(&value.to_string()),
                TRUNC_DEFAULT
            )
        )],
        Some(other) => vec![format!(
            "[codex:{other}]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
        None => vec![format!(
            "[json]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
    }
}

fn render_codex_item(item: &Value, marker: &str) -> String {
    match str_field(item, "type") {
        Some("agent_message") => format!(
            "[text]  {}",
            truncate(str_field(item, "text").unwrap_or(""), TRUNC_DEFAULT)
        ),
        Some("reasoning") => format!(
            "[think] {}",
            truncate(str_field(item, "text").unwrap_or(""), TRUNC_DEFAULT)
        ),
        Some("command_execution") => format!(
            "[{marker}] command  status={}  command={}",
            str_field(item, "status").unwrap_or("?"),
            backtick(str_field(item, "command").unwrap_or(""))
        ),
        Some("file_change") => format!(
            "[{marker}] file_change  status={}  changes={}",
            str_field(item, "status").unwrap_or("?"),
            item.get("changes")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        Some("mcp_tool_call") => format!(
            "[{marker}] mcp  {}  status={}",
            str_field(item, "tool").unwrap_or("tool"),
            str_field(item, "status").unwrap_or("?")
        ),
        Some("todo_list") => format!(
            "[{marker}] todo  items={}",
            item.get("items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        Some(other) => format!(
            "[{marker}] {other}  {}",
            truncate(&item.to_string(), TRUNC_DEFAULT)
        ),
        None => format!("[{marker}] {}", truncate(&item.to_string(), TRUNC_DEFAULT)),
    }
}

fn render_codex_turn(value: &Value, status: &str) -> String {
    let usage = value.get("usage");
    let input = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    format!(
        "[turn]  {status}  in={}  out={}",
        format_tokens(input),
        format_tokens(output)
    )
}

fn render_opencode(value: &Value) -> Vec<String> {
    match str_field(value, "type") {
        Some("text") => vec![format!(
            "[text]  {}",
            truncate(part_text(value).unwrap_or(""), TRUNC_DEFAULT)
        )],
        Some("reasoning") => vec![format!(
            "[think] {}",
            truncate(part_text(value).unwrap_or(""), TRUNC_DEFAULT)
        )],
        Some("tool_use") => {
            let part = value.get("part").unwrap_or(value);
            vec![format!(
                "[tool]  {}  status={}",
                str_field(part, "tool").unwrap_or("tool"),
                part.get("state")
                    .and_then(|state| str_field(state, "status"))
                    .unwrap_or("?")
            )]
        }
        Some("step_start") => vec!["[step→] started".to_string()],
        Some("step_finish") => vec!["[step✓] finished".to_string()],
        Some("error") => vec![format!(
            "[error] {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
        Some(other) => vec![format!(
            "[opencode:{other}]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
        None => vec![format!(
            "[json]  {}",
            truncate(&value.to_string(), TRUNC_DEFAULT)
        )],
    }
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

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn truncate(value: &str, limit: usize) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let head: String = flat.chars().take(limit.saturating_sub(1)).collect();
    let remaining = flat.chars().count().saturating_sub(limit.saturating_sub(1));
    format!("{head}…+{remaining} more")
}

fn format_tokens(value: i64) -> String {
    if value.abs() < 1000 {
        value.to_string()
    } else {
        format!("{:.1}k", value as f64 / 1000.0)
    }
}

fn format_tool_input(input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    match input {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                if key == "command" {
                    format!(
                        "{key}={}",
                        backtick(value.as_str().unwrap_or(&value.to_string()))
                    )
                } else if let Some(value) = value.as_str() {
                    format!("{key}={}", truncate(value, 200))
                } else {
                    format!("{key}={}", truncate(&value.to_string(), 200))
                }
            })
            .collect::<Vec<_>>()
            .join("  "),
        Value::String(value) => truncate(value, 200),
        other => truncate(&other.to_string(), 200),
    }
}

fn tool_result_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| str_field(block, "text"))
            .collect::<Vec<_>>()
            .join(" "),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn preview_suffix(value: &str, limit: usize) -> String {
    let preview = truncate(value, limit);
    if preview.is_empty() {
        String::new()
    } else {
        format!("  {preview}")
    }
}

fn backtick(value: &str) -> String {
    format!("`{}`", value.replace('`', "\\`"))
}

fn part_text(value: &Value) -> Option<&str> {
    value
        .get("part")
        .and_then(|part| str_field(part, "text"))
        .or_else(|| str_field(value, "text"))
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
    fn test_claude_renderer_formats_tool_pair_and_result() {
        let mut renderer = HeadlessRenderer::new(RendererKind::Claude);
        let start = renderer.render_value(&json(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"just check","timeout":120000}}]}}"#,
        ));
        let end = renderer.render_value(&json(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}"#,
        ));
        let result = renderer.render_value(&json(
            r#"{"type":"result","subtype":"success","num_turns":1,"duration_ms":1200,"total_cost_usd":0.01,"usage":{"input_tokens":1200,"output_tokens":300}}"#,
        ));

        assert_eq!(
            start[0],
            "[tool→] Bash#01  command=`just check`  timeout=120000"
        );
        assert_eq!(end[0], "[tool✓] Bash#01  ok (2 chars)  ok");
        assert_eq!(
            result[0],
            "[turn]  ok  turns=1  dur=1.2s  in=1.2k  out=300  cost=$0.0100"
        );
    }

    #[test]
    fn test_cursor_renderer_formats_result() {
        let mut renderer = HeadlessRenderer::new(RendererKind::Cursor);
        let lines = renderer.render_value(&json(r#"{"type":"result","subtype":"success"}"#));
        assert_eq!(lines, vec!["[turn]  ok"]);
    }

    #[test]
    fn test_codex_renderer_formats_turn_completed() {
        let mut renderer = HeadlessRenderer::new(RendererKind::Codex);
        let lines = renderer.render_value(&json(
            r#"{"type":"turn.completed","usage":{"input_tokens":1200,"output_tokens":3400}}"#,
        ));
        assert_eq!(lines, vec!["[turn]  ok  in=1.2k  out=3.4k"]);
    }

    #[test]
    fn test_opencode_renderer_formats_text() {
        let mut renderer = HeadlessRenderer::new(RendererKind::Opencode);
        let lines = renderer.render_value(&json(
            r#"{"type":"text","part":{"text":"done with the change"}}"#,
        ));
        assert_eq!(lines, vec!["[text]  done with the change"]);
    }

    #[test]
    fn test_rendered_tail_omits_old_lines() {
        let mut tail = RenderedTail::default();
        for i in 0..(RENDER_TAIL_LINES + 2) {
            tail.push(format!("line {i}"));
        }
        assert_eq!(tail.omitted, 2);
        assert_eq!(tail.lines.front().unwrap(), "line 2");
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
        let recorder = HeadlessRunRecorder::new(
            dir.path().join("metadata.json"),
            "test-profile",
            &profile,
            &profile.args,
            DateTime::parse_from_rfc3339("2026-06-09T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );

        recorder.write().unwrap();
        let value: JsonValue =
            serde_json::from_str(&fs::read_to_string(dir.path().join("metadata.json")).unwrap())
                .unwrap();

        assert_eq!(value["profile"]["name"], "test-profile");
        assert_eq!(value["profile"]["command"], "agent");
        assert_eq!(value["interface"], "claude");
        assert_eq!(value["prompt_delivery"], "argument");
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
