use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use super::runs::RunState;
use super::{MonitorCore, RunSummary};

pub(crate) fn run(args: crate::MonitorArgs) -> Result<()> {
    let runs_root = match args.runs_root {
        Some(path) => path,
        None => MonitorCore::default_root()?,
    };
    let poll_interval = Duration::from_millis(args.poll_interval_ms.max(50));
    let mut app = MonitorApp::new(MonitorCore::new(runs_root), poll_interval);
    app.poll()?;

    if args.once {
        print_snapshot(&app);
        return Ok(());
    }

    run_tui(app)
}

#[derive(Default)]
struct RunTranscript {
    lines: VecDeque<String>,
    pending_raw: Option<String>,
}

struct MonitorApp {
    core: MonitorCore,
    poll_interval: Duration,
    runs: Vec<RunSummary>,
    selected: usize,
    selected_run_path: Option<PathBuf>,
    transcripts: HashMap<PathBuf, RunTranscript>,
}

impl MonitorApp {
    const MAX_TRANSCRIPT_LINES: usize = 1_000;

    fn new(core: MonitorCore, poll_interval: Duration) -> Self {
        Self {
            core,
            poll_interval,
            runs: Vec::new(),
            selected: 0,
            selected_run_path: None,
            transcripts: HashMap::new(),
        }
    }

    fn poll(&mut self) -> Result<()> {
        let previous_selected = self.selected_run_path.clone();
        self.runs = self.core.poll_runs()?;
        if self.runs.is_empty() {
            self.selected = 0;
            self.selected_run_path = None;
            return Ok(());
        }

        self.selected = previous_selected
            .as_ref()
            .and_then(|path| self.runs.iter().position(|run| &run.path == path))
            .unwrap_or_else(|| self.selected.min(self.runs.len() - 1));

        let selected_run = self.runs[self.selected].clone();
        self.selected_run_path = Some(selected_run.path.clone());
        let update = self.core.poll_stdout(&selected_run)?;
        let transcript = self.transcripts.entry(selected_run.path).or_default();
        for line in update.lines {
            if transcript.lines.len() == Self::MAX_TRANSCRIPT_LINES {
                transcript.lines.pop_front();
            }
            transcript.lines.push_back(line);
        }
        transcript.pending_raw = update.pending_raw;
        Ok(())
    }

    fn set_selected(&mut self, selected: usize) {
        self.selected = if self.runs.is_empty() {
            0
        } else {
            selected.min(self.runs.len() - 1)
        };
        self.selected_run_path = self.runs.get(self.selected).map(|run| run.path.clone());
    }

    fn select_next(&mut self) {
        if !self.runs.is_empty() {
            self.set_selected(self.selected + 1);
        }
    }

    fn select_previous(&mut self) {
        self.set_selected(self.selected.saturating_sub(1));
    }

    fn select_first(&mut self) {
        self.set_selected(0);
    }

    fn select_last(&mut self) {
        if !self.runs.is_empty() {
            self.set_selected(self.runs.len() - 1);
        }
    }
}

fn cleanup_terminal_startup<W: io::Write>(writer: &mut W, alternate_screen_entered: bool) {
    if alternate_screen_entered {
        let _ = execute!(writer, LeaveAlternateScreen);
    }
    let _ = terminal::disable_raw_mode();
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let mut alternate_screen_entered = false;

        let entered = (|| -> Result<Self> {
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            alternate_screen_entered = true;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;
            terminal.clear()?;
            Ok(Self { terminal })
        })();

        if entered.is_err() {
            cleanup_terminal_startup(&mut io::stdout(), alternate_screen_entered);
        }

        entered
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn run_tui(mut app: MonitorApp) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    loop {
        guard.terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(app.poll_interval)?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
                KeyCode::Home | KeyCode::Char('g') => app.select_first(),
                KeyCode::End | KeyCode::Char('G') => app.select_last(),
                _ => {}
            }
        }

        app.poll()?;
    }
    Ok(())
}

fn state_label(state: RunState) -> &'static str {
    match state {
        RunState::Active => "running",
        RunState::Success => "success",
        RunState::Failed => "failed",
        RunState::Unknown => "unknown",
    }
}

fn detail_text(app: &MonitorApp, max_transcript_lines: usize) -> Vec<String> {
    if app.runs.is_empty() {
        return vec!["No headless run archives found.".to_string()];
    }

    let run = &app.runs[app.selected];
    let mut lines = vec![
        format!("id: {}", run.id),
        format!("state: {}", state_label(run.state)),
        format!(
            "profile: {}",
            run.profile_name.as_deref().unwrap_or("unknown")
        ),
        format!(
            "command: {}",
            run.profile_command.as_deref().unwrap_or("unknown")
        ),
        format!(
            "interface: {}",
            run.interface.as_deref().unwrap_or("unknown")
        ),
        format!(
            "started: {}",
            run.started_at.as_deref().unwrap_or("unknown")
        ),
    ];

    if let Some(completed_at) = run.completed_at.as_deref() {
        lines.push(format!("completed: {completed_at}"));
    }
    if let Some(exit_code) = run.exit_code {
        lines.push(format!("exit code: {exit_code}"));
    }
    if let Some(failure) = run.failure.as_deref() {
        lines.push(format!("failure: {failure}"));
    }
    if let Some(error) = run.metadata_error.as_deref() {
        lines.push(format!("metadata error: {error}"));
    }

    lines.push(String::new());
    lines.push("transcript:".to_string());

    let empty_transcript = RunTranscript::default();
    let transcript = app.transcripts.get(&run.path).unwrap_or(&empty_transcript);

    let transcript_lines: Vec<&String> = transcript
        .lines
        .iter()
        .rev()
        .take(max_transcript_lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if transcript_lines.is_empty() {
        lines.push("  (no output yet)".to_string());
    } else {
        for line in transcript_lines {
            lines.push(line.clone());
        }
    }

    if let Some(pending) = transcript.pending_raw.as_deref() {
        lines.push(format!("  (partial) {pending}"));
    }

    lines
}

fn detail_lines(app: &MonitorApp, max_transcript_lines: usize) -> Vec<Line<'static>> {
    detail_text(app, max_transcript_lines)
        .into_iter()
        .map(|line| Line::from(Span::raw(line)))
        .collect()
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &MonitorApp) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(root[0]);

    let runs = app.runs.iter().enumerate().map(|(index, run)| {
        let selected = index == app.selected;
        let state = state_label(run.state);
        let title = run.profile_name.as_deref().unwrap_or(&run.id);
        let started = run.started_at.as_deref().unwrap_or("unknown time");
        let prefix = if selected { "> " } else { "  " };
        ListItem::new(vec![
            Line::from(format!("{prefix}{state} {title}")),
            Line::from(format!("  {started}")),
        ])
    });

    let list = List::new(runs.collect::<Vec<_>>()).block(Block::bordered().title("Headless runs"));
    frame.render_widget(list, chunks[0]);

    let detail_height = chunks[1].height.saturating_sub(2) as usize;
    let detail = Paragraph::new(detail_lines(app, detail_height))
        .block(Block::bordered().title("Run detail"))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::raw("j/k or arrows: navigate  "),
        Span::raw("g/G or Home/End: first/last  "),
        Span::raw("q or Esc: quit"),
    ]))
    .style(Style::default());
    frame.render_widget(footer, root[1]);
}

fn print_snapshot(app: &MonitorApp) {
    println!("Headless runs");
    if app.runs.is_empty() {
        println!("No headless run archives found.");
        return;
    }

    for (index, run) in app.runs.iter().enumerate() {
        let marker = if index == app.selected { ">" } else { " " };
        println!(
            "{marker} {} {}",
            state_label(run.state),
            run.profile_name.as_deref().unwrap_or(&run.id)
        );
    }

    println!();
    println!("Run detail");
    for line in detail_text(app, usize::MAX) {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_run(path: PathBuf, name: &str, started_at: &str, text: &str) {
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("metadata.json"),
            format!(
                r#"{{"profile":{{"name":"{name}","command":"agent","args":[]}},"interface":"claude","started_at":"{started_at}","status":"running"}}"#
            ),
        )
        .unwrap();
        fs::write(
            path.join("stdout.jsonl"),
            format!(
                "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}}]}}}}\n"
            ),
        )
        .unwrap();
    }

    fn line_count(app: &MonitorApp, expected: &str) -> usize {
        detail_text(app, usize::MAX)
            .iter()
            .filter(|line| line.as_str() == expected)
            .count()
    }

    #[test]
    fn app_poll_loads_run_and_transcript() {
        let dir = tempfile::TempDir::new().unwrap();
        let run_path = dir.path().join("run-1");
        fs::create_dir_all(&run_path).unwrap();
        fs::write(
            run_path.join("metadata.json"),
            r#"{"profile":{"name":"demo","command":"agent","args":[]},"interface":"claude","started_at":"2026-06-09T00:00:00Z","status":"running"}"#,
        )
        .unwrap();
        fs::write(
            run_path.join("stdout.jsonl"),
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
        )
        .unwrap();

        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();

        assert_eq!(app.runs.len(), 1);
        assert_eq!(app.transcripts[&run_path].lines, vec!["[text]  hello"]);
    }

    #[test]
    fn app_preserves_transcript_cache_when_switching_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        write_run(
            dir.path().join("run-b"),
            "b",
            "2026-06-09T01:00:00Z",
            "second",
        );

        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        assert_eq!(line_count(&app, "[text]  second"), 1);

        app.select_next();
        app.poll().unwrap();
        assert_eq!(line_count(&app, "[text]  first"), 1);

        app.select_previous();
        app.poll().unwrap();
        assert_eq!(line_count(&app, "[text]  second"), 1);
    }

    #[test]
    fn app_preserves_selected_run_when_poll_resorts_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        write_run(
            dir.path().join("run-b"),
            "b",
            "2026-06-09T01:00:00Z",
            "second",
        );

        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.select_next();
        let selected_path = app.runs[app.selected].path.clone();

        write_run(
            dir.path().join("run-c"),
            "c",
            "2026-06-09T02:00:00Z",
            "third",
        );
        app.poll().unwrap();

        assert_eq!(app.runs[app.selected].path, selected_path);
    }

    #[test]
    fn cleanup_terminal_startup_leaves_alternate_screen_after_enter() {
        let mut output = Vec::new();
        cleanup_terminal_startup(&mut output, true);
        assert_eq!(output, b"\x1b[?1049l");
    }

    #[test]
    fn cleanup_terminal_startup_does_not_leave_alternate_screen_before_enter() {
        let mut output = Vec::new();
        cleanup_terminal_startup(&mut output, false);
        assert!(output.is_empty());
    }
}
