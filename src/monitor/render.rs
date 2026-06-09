use serde_json::Value;
use std::collections::{HashMap, VecDeque};

const TRUNC_DEFAULT: usize = 500;
pub(crate) const RENDER_TAIL_LINES: usize = 30;

#[derive(Default)]
pub(crate) struct RenderedTail {
    pub(crate) lines: VecDeque<String>,
    pub(crate) omitted: usize,
}

impl RenderedTail {
    pub(crate) fn push(&mut self, line: String) {
        if self.lines.len() == RENDER_TAIL_LINES {
            self.lines.pop_front();
            self.omitted += 1;
        }
        self.lines.push_back(line);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RendererKind {
    Claude,
    Codex,
    Cursor,
    Opencode,
    Raw,
}

impl RendererKind {
    pub(crate) fn from_interface(interface: &str) -> Self {
        match interface {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "cursor" => Self::Cursor,
            "opencode" => Self::Opencode,
            _ => Self::Raw,
        }
    }
}

pub(crate) struct RenderedJsonlLine {
    pub(crate) value: Option<Value>,
    pub(crate) lines: Vec<String>,
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

pub(crate) struct CompactRenderer {
    kind: RendererKind,
    state: RenderState,
}

impl CompactRenderer {
    pub(crate) fn new(kind: RendererKind) -> Self {
        Self {
            kind,
            state: RenderState::default(),
        }
    }

    pub(crate) fn render_jsonl_line(&mut self, line: &str) -> RenderedJsonlLine {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => {
                let lines = self.render_value(&value);
                RenderedJsonlLine {
                    value: Some(value),
                    lines,
                }
            }
            Err(_) => RenderedJsonlLine {
                value: None,
                lines: vec![format!("[raw]   {}", truncate(line, TRUNC_DEFAULT))],
            },
        }
    }

    pub(crate) fn render_value(&mut self, value: &Value) -> Vec<String> {
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
    use serde_json::Value;

    fn json(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn test_claude_renderer_formats_tool_pair_and_result() {
        let mut renderer = CompactRenderer::new(RendererKind::Claude);
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
        let mut renderer = CompactRenderer::new(RendererKind::Cursor);
        let lines = renderer.render_value(&json(r#"{"type":"result","subtype":"success"}"#));
        assert_eq!(lines, vec!["[turn]  ok"]);
    }

    #[test]
    fn test_codex_renderer_formats_turn_completed() {
        let mut renderer = CompactRenderer::new(RendererKind::Codex);
        let lines = renderer.render_value(&json(
            r#"{"type":"turn.completed","usage":{"input_tokens":1200,"output_tokens":3400}}"#,
        ));
        assert_eq!(lines, vec!["[turn]  ok  in=1.2k  out=3.4k"]);
    }

    #[test]
    fn test_opencode_renderer_formats_text() {
        let mut renderer = CompactRenderer::new(RendererKind::Opencode);
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
    fn test_render_jsonl_line_malformed() {
        let mut renderer = CompactRenderer::new(RendererKind::Claude);
        let rendered = renderer.render_jsonl_line("not json");
        assert!(rendered.value.is_none());
        assert_eq!(rendered.lines, vec!["[raw]   not json"]);
    }
}
