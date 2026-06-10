use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use pulldown_cmark::{Event as MarkdownEvent, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Row, Table, TableState, Wrap},
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use super::runs::RunState;
use super::{MonitorCore, RunSummary};

const PRIMARY: Color = Color::Rgb(181, 142, 255);
const WHITE: Color = Color::Rgb(245, 241, 255);
const DIM_WHITE: Color = Color::Rgb(186, 178, 205);
const SEPARATOR: Color = Color::Rgb(68, 60, 92);
const BG: Color = Color::Rgb(17, 15, 28);
const GREEN: Color = Color::Rgb(121, 214, 170);
const RED: Color = Color::Rgb(244, 129, 129);
const YELLOW: Color = Color::Rgb(245, 184, 108);
const DIM: Color = Color::Rgb(126, 116, 148);
const SELECTED_BG: Color = Color::Rgb(43, 35, 67);
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const PROMPT_COLLAPSED_LINES: usize = 10;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Table,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Active,
    History,
}

struct MonitorApp {
    core: MonitorCore,
    poll_interval: Duration,
    runs: Vec<RunSummary>,
    active_indices: Vec<usize>,
    history_indices: Vec<usize>,
    filtered_history_indices: Vec<usize>,
    focus: Focus,
    mode: AppMode,
    active_selected: usize,
    history_selected: usize,
    selected_run_path: Option<PathBuf>,
    active_table_state: TableState,
    history_table_state: TableState,
    detail_scroll: u16,
    detail_auto_scroll: bool,
    prompt_expanded: bool,
    prompt_click_area: Option<Rect>,
    prompt_more_area: Option<Rect>,
    prompt_more_hovered: bool,
    transcripts: HashMap<PathBuf, RunTranscript>,
    filter_text: String,
    filter_editing: bool,
    filter_draft: String,
    show_help: bool,
    show_run_info: bool,
    tick: usize,
}

impl MonitorApp {
    const MAX_TRANSCRIPT_LINES: usize = 1_000;

    fn new(core: MonitorCore, poll_interval: Duration) -> Self {
        Self {
            core,
            poll_interval,
            runs: Vec::new(),
            active_indices: Vec::new(),
            history_indices: Vec::new(),
            filtered_history_indices: Vec::new(),
            focus: Focus::Active,
            mode: AppMode::Table,
            active_selected: 0,
            history_selected: 0,
            selected_run_path: None,
            active_table_state: TableState::default(),
            history_table_state: TableState::default(),
            detail_scroll: 0,
            detail_auto_scroll: false,
            prompt_expanded: false,
            prompt_click_area: None,
            prompt_more_area: None,
            prompt_more_hovered: false,
            transcripts: HashMap::new(),
            filter_text: String::new(),
            filter_editing: false,
            filter_draft: String::new(),
            show_help: false,
            show_run_info: false,
            tick: 0,
        }
    }

    fn poll(&mut self) -> Result<()> {
        let previous_selected = self.selected_run_path.clone();
        self.runs = self.core.poll_runs()?;
        self.rebuild_tables(previous_selected.as_ref());
        self.poll_selected_stdout()?;
        self.tick = self.tick.wrapping_add(1);
        Ok(())
    }

    fn rebuild_tables(&mut self, previous_selected: Option<&PathBuf>) {
        self.active_indices = self
            .runs
            .iter()
            .enumerate()
            .filter_map(|(index, run)| (run.state == RunState::Active).then_some(index))
            .collect();
        self.history_indices = self
            .runs
            .iter()
            .enumerate()
            .filter_map(|(index, run)| (run.state != RunState::Active).then_some(index))
            .collect();
        self.rebuild_filter();

        self.active_selected = restore_selection(
            &self.runs,
            &self.active_indices,
            previous_selected,
            self.active_selected,
        );
        self.history_selected = restore_selection(
            &self.runs,
            &self.filtered_history_indices,
            previous_selected,
            self.history_selected,
        );

        if self.active_indices.is_empty() && self.focus == Focus::Active {
            self.focus = Focus::History;
        }
        if self.filtered_history_indices.is_empty() && self.focus == Focus::History {
            self.focus = Focus::Active;
        }
        self.selected_run_path = self.selected_run().map(|run| run.path.clone());
    }

    fn rebuild_filter(&mut self) {
        if self.filter_text.is_empty() {
            self.filtered_history_indices = self.history_indices.clone();
            return;
        }
        let needle = self.filter_text.to_lowercase();
        self.filtered_history_indices = self
            .history_indices
            .iter()
            .copied()
            .filter(|&index| run_matches_filter(&self.runs[index], &needle))
            .collect();
    }

    fn poll_selected_stdout(&mut self) -> Result<()> {
        let Some(run) = self.selected_run().cloned() else {
            self.selected_run_path = None;
            return Ok(());
        };
        self.selected_run_path = Some(run.path.clone());
        let update = self.core.poll_stdout(&run)?;
        let transcript = self.transcripts.entry(run.path).or_default();
        for line in update.lines {
            if transcript.lines.len() == Self::MAX_TRANSCRIPT_LINES {
                transcript.lines.pop_front();
            }
            transcript.lines.push_back(line);
        }
        transcript.pending_raw = update.pending_raw;
        Ok(())
    }

    fn selected_indices(&self) -> &[usize] {
        match self.focus {
            Focus::Active => &self.active_indices,
            Focus::History => &self.filtered_history_indices,
        }
    }

    fn selected_position(&self) -> usize {
        match self.focus {
            Focus::Active => self.active_selected,
            Focus::History => self.history_selected,
        }
    }

    fn set_selected_position(&mut self, selected: usize) {
        match self.focus {
            Focus::Active => {
                self.active_selected = clamp_selection(selected, self.active_indices.len());
            }
            Focus::History => {
                self.history_selected =
                    clamp_selection(selected, self.filtered_history_indices.len());
            }
        }
        self.detail_scroll = 0;
        self.detail_auto_scroll = false;
        self.prompt_expanded = false;
        self.prompt_more_hovered = false;
        self.selected_run_path = self.selected_run().map(|run| run.path.clone());
    }

    fn selected_run(&self) -> Option<&RunSummary> {
        let index = self
            .selected_indices()
            .get(self.selected_position())
            .copied()?;
        self.runs.get(index)
    }

    fn select_next(&mut self) {
        if self.focus == Focus::Active
            && self.active_selected + 1 >= self.active_indices.len()
            && !self.filtered_history_indices.is_empty()
        {
            self.focus = Focus::History;
            self.selected_run_path = self.selected_run().map(|run| run.path.clone());
            return;
        }
        self.set_selected_position(self.selected_position() + 1);
    }

    fn select_previous(&mut self) {
        if self.focus == Focus::History
            && self.history_selected == 0
            && !self.active_indices.is_empty()
        {
            self.focus = Focus::Active;
            self.selected_run_path = self.selected_run().map(|run| run.path.clone());
            return;
        }
        self.set_selected_position(self.selected_position().saturating_sub(1));
    }

    fn select_first(&mut self) {
        self.set_selected_position(0);
    }

    fn select_last(&mut self) {
        let count = self.selected_indices().len();
        if count > 0 {
            self.set_selected_position(count - 1);
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Active => Focus::History,
            Focus::History => Focus::Active,
        };
        if self.selected_indices().is_empty() {
            self.focus = match self.focus {
                Focus::Active => Focus::History,
                Focus::History => Focus::Active,
            };
        }
        self.selected_run_path = self.selected_run().map(|run| run.path.clone());
    }

    fn start_filter(&mut self) {
        self.filter_editing = true;
        self.filter_draft = self.filter_text.clone();
        self.focus = Focus::History;
    }

    fn accept_filter(&mut self) {
        self.filter_text.clone_from(&self.filter_draft);
        self.filter_editing = false;
        self.rebuild_filter();
        self.history_selected =
            clamp_selection(self.history_selected, self.filtered_history_indices.len());
        self.selected_run_path = self.selected_run().map(|run| run.path.clone());
    }

    fn cancel_filter(&mut self) {
        self.filter_editing = false;
        self.filter_draft.clear();
    }
}

fn restore_selection(
    runs: &[RunSummary],
    indices: &[usize],
    previous_selected: Option<&PathBuf>,
    fallback: usize,
) -> usize {
    if indices.is_empty() {
        return 0;
    }
    previous_selected
        .and_then(|path| indices.iter().position(|&index| &runs[index].path == path))
        .unwrap_or_else(|| fallback.min(indices.len() - 1))
}

fn clamp_selection(selected: usize, len: usize) -> usize {
    if len == 0 { 0 } else { selected.min(len - 1) }
}

fn run_matches_filter(run: &RunSummary, needle: &str) -> bool {
    let haystacks = [
        Some(run.id.as_str()),
        run.profile_name.as_deref(),
        run.profile_command.as_deref(),
        run.interface.as_deref(),
        run.prompt_delivery.as_deref(),
        run.started_at.as_deref(),
        run.completed_at.as_deref(),
        run.failure.as_deref(),
    ];
    haystacks
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(needle))
        || state_label(run.state).contains(needle)
}

fn cleanup_terminal_startup<W: io::Write>(
    writer: &mut W,
    alternate_screen_entered: bool,
    mouse_capture_enabled: bool,
) {
    if mouse_capture_enabled {
        let _ = execute!(writer, DisableMouseCapture);
    }
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
        let mut mouse_capture_enabled = false;

        let entered = (|| -> Result<Self> {
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            alternate_screen_entered = true;
            execute!(stdout, EnableMouseCapture)?;
            mouse_capture_enabled = true;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;
            terminal.clear()?;
            Ok(Self { terminal })
        })();

        if entered.is_err() {
            cleanup_terminal_startup(
                &mut io::stdout(),
                alternate_screen_entered,
                mouse_capture_enabled,
            );
        }

        entered
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

fn run_tui(mut app: MonitorApp) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;
    loop {
        guard.terminal.draw(|frame| draw(frame, &mut app))?;

        if event::poll(app.poll_interval)? {
            match event::read()? {
                Event::Key(key) if handle_key(&mut app, key) => break,
                Event::Mouse(mouse) => handle_mouse(&mut app, mouse),
                _ => {}
            }
        }

        app.poll()?;
    }
    Ok(())
}

fn handle_key(app: &mut MonitorApp, key: KeyEvent) -> bool {
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.show_help = false,
            _ => {}
        }
        return false;
    }

    if app.show_run_info {
        match key.code {
            KeyCode::Char('i') | KeyCode::Esc => app.show_run_info = false,
            KeyCode::Char('q') => return true,
            _ => {}
        }
        return false;
    }

    if app.filter_editing {
        match key.code {
            KeyCode::Enter => app.accept_filter(),
            KeyCode::Esc => app.cancel_filter(),
            KeyCode::Backspace => {
                app.filter_draft.pop();
            }
            KeyCode::Char(c) => app.filter_draft.push(c),
            _ => {}
        }
        return false;
    }

    match app.mode {
        AppMode::Table => handle_table_key(app, key),
        AppMode::Detail => handle_detail_key(app, key),
    }
}

fn handle_table_key(app: &mut MonitorApp, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Tab | KeyCode::BackTab => app.toggle_focus(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Home | KeyCode::Char('g') => app.select_first(),
        KeyCode::End | KeyCode::Char('G') => app.select_last(),
        KeyCode::Enter => {
            if let Some(run) = app.selected_run() {
                app.detail_auto_scroll = run.state == RunState::Active;
                app.detail_scroll = if app.detail_auto_scroll { u16::MAX } else { 0 };
                app.mode = AppMode::Detail;
            }
        }
        KeyCode::Char('/') => app.start_filter(),
        _ => {}
    }
    false
}

fn handle_detail_key(app: &mut MonitorApp, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Esc => {
            if app.show_run_info {
                app.show_run_info = false;
            } else {
                app.mode = AppMode::Table;
            }
        }
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('i') => app.show_run_info = !app.show_run_info,
        KeyCode::Char('p') => app.prompt_expanded = !app.prompt_expanded,
        KeyCode::Down | KeyCode::Char('j') => {
            app.detail_scroll = app.detail_scroll.saturating_add(1)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.detail_scroll = app.detail_scroll.saturating_sub(1);
            app.detail_auto_scroll = false;
        }
        KeyCode::PageDown | KeyCode::Char('d') => {
            app.detail_scroll = app.detail_scroll.saturating_add(20)
        }
        KeyCode::PageUp | KeyCode::Char('u') => {
            app.detail_scroll = app.detail_scroll.saturating_sub(20);
            app.detail_auto_scroll = false;
        }
        KeyCode::Home | KeyCode::Char('g') => {
            app.detail_scroll = 0;
            app.detail_auto_scroll = false;
        }
        KeyCode::End | KeyCode::Char('G') => {
            app.detail_scroll = u16::MAX;
            app.detail_auto_scroll = true;
        }
        _ => {}
    }
    false
}

fn handle_mouse(app: &mut MonitorApp, mouse: crossterm::event::MouseEvent) {
    if app.mode != AppMode::Detail || app.show_help || app.show_run_info {
        return;
    }
    update_prompt_hover(app, mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(area) = app.prompt_click_area else {
                return;
            };
            if rect_contains(area, mouse.column, mouse.row) {
                app.prompt_expanded = !app.prompt_expanded;
                app.prompt_more_hovered = false;
            }
        }
        MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {}
        _ => {}
    }
}

fn update_prompt_hover(app: &mut MonitorApp, column: u16, row: u16) {
    app.prompt_more_hovered = app.prompt_more_area.is_some()
        && app
            .prompt_click_area
            .is_some_and(|area| rect_contains(area, column, row));
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn state_label(state: RunState) -> &'static str {
    match state {
        RunState::Active => "running",
        RunState::Success => "success",
        RunState::Failed => "failed",
        RunState::Unknown => "unknown",
    }
}

fn status_icon(state: RunState) -> &'static str {
    match state {
        RunState::Success => "✓",
        RunState::Failed => "✗",
        RunState::Active => "…",
        RunState::Unknown => "?",
    }
}

fn status_color(state: RunState) -> Color {
    match state {
        RunState::Success => GREEN,
        RunState::Failed => RED,
        RunState::Active => YELLOW,
        RunState::Unknown => DIM,
    }
}

fn profile_label(run: &RunSummary) -> &str {
    run.profile_name.as_deref().unwrap_or("unknown")
}

fn project_label(run: &RunSummary) -> String {
    run.path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("runs")
        .to_string()
}

fn mode_label(run: &RunSummary) -> &str {
    if run.prompt_delivery.is_some() || run.stdout_file.exists() {
        "headless"
    } else {
        "tmux"
    }
}

fn stage_label(run: &RunSummary, tick: usize) -> String {
    match run.state {
        RunState::Active => format!("{} running", SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]),
        RunState::Success => "complete".to_string(),
        RunState::Failed => "failed".to_string(),
        RunState::Unknown => "unknown".to_string(),
    }
}

fn run_duration(run: &RunSummary) -> String {
    let Some(started) = parse_time(run.started_at.as_deref()) else {
        return "?".to_string();
    };
    let end = run
        .completed_at
        .as_deref()
        .and_then(|completed| DateTime::parse_from_rfc3339(completed).ok())
        .unwrap_or_else(|| Utc::now().into());
    let millis = end.signed_duration_since(started).num_milliseconds().max(0) as u64;
    format_duration(millis)
}

fn parse_time(value: Option<&str>) -> Option<DateTime<FixedOffset>> {
    value.and_then(|value| DateTime::parse_from_rfc3339(value).ok())
}

fn short_time(value: Option<&str>) -> String {
    let Some(time) = parse_time(value) else {
        return "?".to_string();
    };
    let now = Utc::now();
    let delta = now.signed_duration_since(time.with_timezone(&Utc));
    if delta.num_seconds() < 60 {
        "now".to_string()
    } else if delta.num_minutes() < 60 {
        format!("{}m", delta.num_minutes())
    } else if delta.num_hours() < 24 {
        format!("{}h", delta.num_hours())
    } else {
        time.format("%m-%d").to_string()
    }
}

fn format_duration(millis: u64) -> String {
    let secs = millis / 1_000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3_600, (secs % 3_600) / 60)
    }
}

fn run_info_text(run: &RunSummary) -> Vec<String> {
    let mut lines = vec![
        format!("id: {}", run.id),
        format!("state: {}", state_label(run.state)),
        format!("project: {}", project_label(run)),
        format!("profile: {}", profile_label(run)),
        format!(
            "command: {}",
            run.profile_command.as_deref().unwrap_or("unknown")
        ),
        format!("args: {}", format_profile_args(&run.profile_args)),
        format!(
            "interface: {}",
            run.interface.as_deref().unwrap_or("unknown")
        ),
        format!("mode: {}", mode_label(run)),
        format!(
            "prompt delivery: {}",
            run.prompt_delivery.as_deref().unwrap_or("unknown")
        ),
        format!(
            "started: {}",
            run.started_at.as_deref().unwrap_or("unknown")
        ),
    ];

    if let Some(completed_at) = run.completed_at.as_deref() {
        lines.push(format!("completed: {completed_at}"));
    }
    lines.push(format!("duration: {}", run_duration(run)));
    if let Some(exit_code) = run.exit_code {
        lines.push(format!("exit code: {exit_code}"));
    }
    if let Some(seen) = run.completion_event_seen {
        lines.push(format!("completion event seen: {seen}"));
    }
    if let Some(failure) = run.failure.as_deref() {
        lines.push(format!("failure: {failure}"));
    }
    if let Some(error) = run.metadata_error.as_deref() {
        lines.push(format!("metadata error: {error}"));
    }
    if let Some(pid) = run.pid {
        lines.push(format!("pid: {pid}"));
    }
    if let Some(pane_id) = run.tmux_pane_id.as_deref() {
        lines.push(format!("tmux pane: {pane_id}"));
    }
    lines.push(format!("run directory: {}", run.path.display()));
    lines
}

fn prompt_text(run: &RunSummary) -> Vec<String> {
    let prompt_path = run.path.join("prompt.md");
    match fs::read_to_string(&prompt_path) {
        Ok(prompt) if !prompt.trim().is_empty() => prompt.lines().map(str::to_string).collect(),
        _ => vec!["(prompt not archived for this run)".to_string()],
    }
}

fn transcript_text(app: &MonitorApp, max_transcript_lines: usize) -> Vec<String> {
    let Some(run) = app.selected_run() else {
        return vec!["No run selected.".to_string()];
    };
    let empty_transcript = RunTranscript::default();
    let transcript = app.transcripts.get(&run.path).unwrap_or(&empty_transcript);

    let mut lines: Vec<String> = transcript
        .lines
        .iter()
        .rev()
        .take(max_transcript_lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .cloned()
        .collect();

    if lines.is_empty() {
        lines.push("(no output yet)".to_string());
    }
    if let Some(pending) = transcript.pending_raw.as_deref() {
        lines.push(format!("(partial) {pending}"));
    }
    lines
}

fn format_profile_args(args: &[String]) -> String {
    if args.is_empty() {
        "(none)".to_string()
    } else {
        args.join(" ")
    }
}

fn artifacts(run: &RunSummary) -> Vec<String> {
    let mut artifacts = Vec::new();
    let Ok(entries) = fs::read_dir(&run.path) else {
        return vec!["(run directory unavailable)".to_string()];
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|name| name.to_str())
        {
            artifacts.push(name.to_string());
        }
    }
    artifacts.sort();
    if artifacts.is_empty() {
        artifacts.push("(none)".to_string());
    }
    artifacts
}

fn detail_text(app: &MonitorApp, max_transcript_lines: usize) -> Vec<String> {
    let Some(run) = app.selected_run() else {
        return vec!["No run selected.".to_string()];
    };

    let mut lines = vec!["Run info".to_string()];
    lines.extend(
        run_info_text(run)
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    lines.push(String::new());
    lines.push("Prompt".to_string());
    lines.extend(
        prompt_display_lines(prompt_text(run), app.prompt_expanded)
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    lines.push(String::new());
    lines.push("Artifacts".to_string());
    for artifact in artifacts(run) {
        lines.push(format!("  {artifact}"));
    }
    lines.push(String::new());
    lines.push("Transcript".to_string());
    lines.extend(
        transcript_text(app, max_transcript_lines)
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    lines
}

fn section_header(title: &'static str) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("  {title}"),
        Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
    )])
}

fn render_plain_block(lines: impl IntoIterator<Item = String>, style: Style) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| Line::from(vec![Span::raw("    "), Span::styled(line, style)]))
        .collect()
}

fn prompt_display_lines(lines: Vec<String>, expanded: bool) -> Vec<String> {
    if expanded || lines.len() <= PROMPT_COLLAPSED_LINES {
        return lines;
    }
    let remaining = lines.len().saturating_sub(PROMPT_COLLAPSED_LINES);
    let mut visible = lines
        .into_iter()
        .take(PROMPT_COLLAPSED_LINES)
        .collect::<Vec<_>>();
    visible.push(format!("+{remaining} more lines"));
    visible
}

fn render_prompt_block(
    lines: Vec<String>,
    expanded: bool,
    more_hovered: bool,
) -> Vec<Line<'static>> {
    let collapsed = !expanded && lines.len() > PROMPT_COLLAPSED_LINES;
    let display_lines = prompt_display_lines(lines, expanded);
    let mut body_lines = display_lines;
    let more_line = if collapsed { body_lines.pop() } else { None };
    let mut rendered = render_prompt_markdown(&body_lines.join("\n"));

    if let Some(line) = more_line {
        let style = if more_hovered {
            Style::default()
                .fg(WHITE)
                .bg(SELECTED_BG)
                .add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(PRIMARY).add_modifier(Modifier::ITALIC)
        };
        rendered.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(line, style),
        ]));
    }

    rendered
}

#[derive(Clone, Copy, Default)]
struct PromptMarkdownAttrs {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
    heading: bool,
    quote: bool,
    link: bool,
}

#[derive(Clone, Copy)]
struct PromptList {
    next: Option<u64>,
    depth: usize,
}

struct PromptMarkdownRenderer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    attrs_stack: Vec<PromptMarkdownAttrs>,
    list_stack: Vec<PromptList>,
    quote_depth: usize,
    in_code_block: bool,
    at_line_start: bool,
}

fn render_prompt_markdown(input: &str) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, options);
    let mut renderer = PromptMarkdownRenderer::new();

    for event in parser {
        renderer.handle(event);
    }

    renderer.finish()
}

impl PromptMarkdownRenderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            spans: Vec::new(),
            attrs_stack: Vec::new(),
            list_stack: Vec::new(),
            quote_depth: 0,
            in_code_block: false,
            at_line_start: true,
        }
    }

    fn handle(&mut self, event: MarkdownEvent<'_>) {
        match event {
            MarkdownEvent::Start(tag) => self.start_tag(tag),
            MarkdownEvent::End(tag) => self.end_tag(tag),
            MarkdownEvent::Text(text) => self.push_text(&text),
            MarkdownEvent::Code(code) => self.push_code(&code),
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                self.flush_line();
                self.emit_continuation_prefix();
            }
            MarkdownEvent::Rule => {
                self.ensure_blank_line();
                self.push_styled("─".repeat(32), PromptMarkdownAttrs::default());
                self.flush_line();
            }
            MarkdownEvent::Html(html) | MarkdownEvent::InlineHtml(html) => self.push_text(&html),
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if self.should_separate_block() {
                    self.ensure_blank_line();
                }
            }
            Tag::Heading { level, .. } => {
                self.ensure_blank_line();
                self.push_styled(
                    heading_prefix(level),
                    PromptMarkdownAttrs {
                        heading: true,
                        ..PromptMarkdownAttrs::default()
                    },
                );
                self.attrs_stack.push(PromptMarkdownAttrs {
                    heading: true,
                    ..PromptMarkdownAttrs::default()
                });
            }
            Tag::Strong => self.attrs_stack.push(PromptMarkdownAttrs {
                bold: true,
                ..PromptMarkdownAttrs::default()
            }),
            Tag::Emphasis => self.attrs_stack.push(PromptMarkdownAttrs {
                italic: true,
                ..PromptMarkdownAttrs::default()
            }),
            Tag::Strikethrough => self.attrs_stack.push(PromptMarkdownAttrs {
                strikethrough: true,
                ..PromptMarkdownAttrs::default()
            }),
            Tag::CodeBlock(_) => {
                self.ensure_blank_line();
                self.in_code_block = true;
            }
            Tag::List(next) => {
                if self.should_separate_block() {
                    self.ensure_blank_line();
                }
                let depth = self.list_stack.len();
                self.list_stack.push(PromptList { next, depth });
            }
            Tag::Item => {
                self.flush_line();
                let Some(list) = self.list_stack.last_mut() else {
                    return;
                };
                let indent = "  ".repeat(list.depth);
                let bullet = match &mut list.next {
                    Some(next) => {
                        let bullet = format!("{indent}{next}. ");
                        *next += 1;
                        bullet
                    }
                    None => format!("{indent}- "),
                };
                self.push_styled(bullet, PromptMarkdownAttrs::default());
            }
            Tag::BlockQuote(_) => {
                self.ensure_blank_line();
                self.quote_depth += 1;
                self.push_styled(
                    "> ",
                    PromptMarkdownAttrs {
                        quote: true,
                        ..PromptMarkdownAttrs::default()
                    },
                );
                self.attrs_stack.push(PromptMarkdownAttrs {
                    quote: true,
                    ..PromptMarkdownAttrs::default()
                });
            }
            Tag::Link { .. } => self.attrs_stack.push(PromptMarkdownAttrs {
                link: true,
                ..PromptMarkdownAttrs::default()
            }),
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.flush_line(),
            TagEnd::Heading(_) => {
                self.flush_line();
                self.attrs_stack.pop();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link => {
                self.attrs_stack.pop();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.flush_line();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.attrs_stack.pop();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }

    fn push_code(&mut self, text: &str) {
        let attrs = PromptMarkdownAttrs {
            code: true,
            ..self.current_attrs()
        };
        self.push_styled(text.to_string(), attrs);
    }

    fn push_text(&mut self, text: &str) {
        let attrs = if self.in_code_block {
            PromptMarkdownAttrs {
                code: true,
                ..PromptMarkdownAttrs::default()
            }
        } else {
            self.current_attrs()
        };
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                self.flush_line();
            }
            self.push_styled(part.to_string(), attrs);
        }
    }

    fn push_styled(&mut self, text: impl Into<String>, attrs: PromptMarkdownAttrs) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.ensure_prompt_indent();
        self.spans
            .push(Span::styled(text, prompt_markdown_style(attrs)));
        self.at_line_start = false;
    }

    fn ensure_prompt_indent(&mut self) {
        if self.at_line_start {
            self.spans.push(Span::raw("    "));
            self.at_line_start = false;
        }
    }

    fn flush_line(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        self.at_line_start = true;
    }

    fn ensure_blank_line(&mut self) {
        self.flush_line();
        if self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn emit_continuation_prefix(&mut self) {
        if self.quote_depth > 0 {
            self.push_styled(
                "> ".repeat(self.quote_depth),
                PromptMarkdownAttrs {
                    quote: true,
                    ..PromptMarkdownAttrs::default()
                },
            );
        }
    }

    fn should_separate_block(&self) -> bool {
        !self.lines.is_empty() && !self.lines.last().is_some_and(|line| line.spans.is_empty())
    }

    fn current_attrs(&self) -> PromptMarkdownAttrs {
        let mut attrs = PromptMarkdownAttrs::default();
        for item in &self.attrs_stack {
            attrs.bold |= item.bold;
            attrs.italic |= item.italic;
            attrs.strikethrough |= item.strikethrough;
            attrs.code |= item.code;
            attrs.heading |= item.heading;
            attrs.quote |= item.quote;
            attrs.link |= item.link;
        }
        attrs
    }
}

fn heading_prefix(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "# ",
        HeadingLevel::H2 => "## ",
        HeadingLevel::H3 => "### ",
        HeadingLevel::H4 => "#### ",
        HeadingLevel::H5 => "##### ",
        HeadingLevel::H6 => "###### ",
    }
}

fn prompt_markdown_style(attrs: PromptMarkdownAttrs) -> Style {
    let mut style = Style::default().fg(DIM_WHITE);
    if attrs.heading {
        style = style.fg(PRIMARY).add_modifier(Modifier::BOLD);
    }
    if attrs.quote {
        style = style.fg(GREEN);
    }
    if attrs.link {
        style = style.fg(PRIMARY).add_modifier(Modifier::UNDERLINED);
    }
    if attrs.code {
        style = style.fg(YELLOW).bg(Color::Rgb(35, 35, 42));
    }
    if attrs.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if attrs.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if attrs.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

struct DetailRender {
    lines: Vec<Line<'static>>,
    prompt_range: Range<usize>,
    prompt_more_line: Option<usize>,
}

fn transcript_line(line: String) -> Line<'static> {
    if let Some(rest) = line.strip_prefix("[text]") {
        Line::from(vec![
            Span::styled(
                "  text ",
                Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.trim_start().to_string(), Style::default().fg(WHITE)),
        ])
    } else if let Some(rest) = line.strip_prefix("[think]") {
        Line::from(vec![
            Span::styled("  💭 ", Style::default().fg(DIM)),
            Span::styled(
                rest.trim_start().to_string(),
                Style::default()
                    .fg(DIM_WHITE)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else if let Some(rest) = line.strip_prefix("[tool→]") {
        tool_transcript_line("tool→", rest, YELLOW)
    } else if let Some(rest) = line.strip_prefix("[tool✓]") {
        tool_transcript_line("tool✓", rest, GREEN)
    } else if let Some(rest) = line.strip_prefix("[tool✗]") {
        tool_transcript_line("tool✗", rest, RED)
    } else if let Some(rest) = line.strip_prefix("[tool]") {
        tool_transcript_line("tool ", rest, YELLOW)
    } else if let Some(rest) = line.strip_prefix("[turn]") {
        Line::from(vec![
            Span::styled(
                "  turn ",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.trim_start().to_string(),
                Style::default().fg(DIM_WHITE),
            ),
        ])
    } else if let Some(rest) = line.strip_prefix("[raw]") {
        Line::from(vec![
            Span::styled(
                "  raw  ",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.trim_start().to_string(), Style::default().fg(DIM)),
        ])
    } else if let Some(rest) = line.strip_prefix("[system]") {
        Line::from(vec![
            Span::styled(
                "  sys  ",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.trim_start().to_string(), Style::default().fg(DIM)),
        ])
    } else if let Some(rest) = line.strip_prefix("[init]") {
        Line::from(vec![
            Span::styled(
                "  init ",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.trim_start().to_string(), Style::default().fg(DIM)),
        ])
    } else if line == "(no output yet)" || line.starts_with("(partial)") {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                line,
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(line, Style::default().fg(DIM_WHITE)),
        ])
    }
}

fn tool_transcript_line(label: &'static str, rest: &str, color: Color) -> Line<'static> {
    let rest = rest.trim_start();
    let (name, detail) = rest.split_once("  ").unwrap_or((rest, ""));
    let mut spans = vec![
        Span::styled("  ", Style::default().fg(DIM)),
        Span::styled("▸", Style::default().fg(color)),
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("{label:<5}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    if !name.is_empty() {
        spans.push(Span::styled(
            format!(" {name}"),
            Style::default().fg(DIM_WHITE),
        ));
    }
    if !detail.is_empty() {
        spans.push(Span::styled(
            format!("  {detail}"),
            Style::default().fg(DIM),
        ));
    }
    Line::from(spans)
}

fn detail_render(app: &MonitorApp, max_transcript_lines: usize) -> DetailRender {
    let Some(run) = app.selected_run() else {
        return DetailRender {
            lines: vec![Line::from(Span::styled(
                "No run selected.",
                Style::default().fg(DIM),
            ))],
            prompt_range: 0..0,
            prompt_more_line: None,
        };
    };

    let prompt = prompt_text(run);
    let prompt_more_line = (!app.prompt_expanded && prompt.len() > PROMPT_COLLAPSED_LINES)
        .then_some(PROMPT_COLLAPSED_LINES + 1);

    let mut lines = Vec::new();
    lines.push(section_header("Prompt"));
    let prompt_start = lines.len() - 1;
    lines.extend(render_prompt_block(
        prompt,
        app.prompt_expanded,
        app.prompt_more_hovered,
    ));
    let prompt_end = lines.len();
    lines.push(Line::default());
    lines.push(section_header("Artifacts"));
    lines.extend(render_plain_block(artifacts(run), Style::default().fg(DIM)));
    lines.push(Line::default());
    lines.push(section_header("Transcript"));
    lines.extend(
        transcript_text(app, max_transcript_lines)
            .into_iter()
            .map(transcript_line),
    );
    DetailRender {
        lines,
        prompt_range: prompt_start..prompt_end,
        prompt_more_line,
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut MonitorApp) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    match app.mode {
        AppMode::Table => draw_table(frame, app, area),
        AppMode::Detail => draw_detail(frame, app, area),
    }
    if app.show_help {
        draw_help(frame, app);
    }
}

fn draw_table(frame: &mut ratatui::Frame<'_>, app: &mut MonitorApp, area: Rect) {
    let active_height = (app.active_indices.len() as u16 + 3)
        .min(area.height.saturating_sub(5) / 2)
        .max(4);
    let filter_height = if app.filter_editing { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(active_height),
            Constraint::Min(5),
            Constraint::Length(filter_height),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(frame, app, chunks[0]);
    draw_active_table(frame, app, chunks[1]);
    draw_history_table(frame, app, chunks[2]);
    if app.filter_editing {
        draw_filter(frame, app, chunks[3]);
    }
    draw_status(frame, app, chunks[4]);
}

fn draw_header(frame: &mut ratatui::Frame<'_>, app: &MonitorApp, area: Rect) {
    let profiles = app
        .runs
        .iter()
        .filter_map(|run| run.profile_name.as_deref())
        .collect::<HashSet<_>>()
        .len();
    let completed = app
        .runs
        .iter()
        .filter(|run| matches!(run.state, RunState::Success | RunState::Failed))
        .count();
    let line = Line::from(vec![
        Span::styled(
            " sideagent-monitor",
            Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(DIM)),
        Span::styled(format!("{profiles}"), Style::default().fg(DIM_WHITE)),
        Span::styled(" profiles · ", Style::default().fg(DIM_WHITE)),
        Span::styled(
            format!("{}", app.active_indices.len()),
            Style::default().fg(if app.active_indices.is_empty() {
                DIM
            } else {
                YELLOW
            }),
        ),
        Span::styled(" active runs · ", Style::default().fg(DIM_WHITE)),
        Span::styled(format!("{completed}"), Style::default().fg(DIM_WHITE)),
        Span::styled(" completed", Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(Paragraph::new(line).style(Style::default().bg(BG)), area);
}

fn draw_active_table(frame: &mut ratatui::Frame<'_>, app: &mut MonitorApp, area: Rect) {
    let header = Row::new(vec![
        "Started",
        "Project",
        "Profile",
        "Interface",
        "Mode",
        "Stage",
        "Dur",
        "",
    ])
    .style(Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD));
    let rows = app
        .active_indices
        .iter()
        .map(|&index| {
            let run = &app.runs[index];
            Row::new(vec![
                short_time(run.started_at.as_deref()),
                project_label(run),
                profile_label(run).to_string(),
                run.interface.as_deref().unwrap_or("unknown").to_string(),
                mode_label(run).to_string(),
                stage_label(run, app.tick),
                run_duration(run),
                String::new(),
            ])
            .style(Style::default().fg(DIM_WHITE))
        })
        .collect::<Vec<_>>();

    let block = table_block(" Active ", app.focus == Focus::Active);
    let table = Table::new(
        rows,
        [
            Constraint::Length(9),
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(18),
            Constraint::Length(8),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(SELECTED_BG))
    .block(block);

    if app.focus == Focus::Active && !app.active_indices.is_empty() {
        app.active_table_state.select(Some(app.active_selected));
    } else {
        app.active_table_state.select(None);
    }
    frame.render_stateful_widget(table, area, &mut app.active_table_state);
}

fn draw_history_table(frame: &mut ratatui::Frame<'_>, app: &mut MonitorApp, area: Rect) {
    let header = Row::new(vec![
        "Time",
        "Project",
        "Profile",
        "Interface",
        "Mode",
        "Duration",
        "Exit",
        "✓",
        "",
    ])
    .style(Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD));
    let rows = app
        .filtered_history_indices
        .iter()
        .map(|&index| {
            let run = &app.runs[index];
            Row::new(vec![
                short_time(run.completed_at.as_deref().or(run.started_at.as_deref())),
                project_label(run),
                profile_label(run).to_string(),
                run.interface.as_deref().unwrap_or("unknown").to_string(),
                mode_label(run).to_string(),
                run_duration(run),
                run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                status_icon(run.state).to_string(),
                String::new(),
            ])
            .style(Style::default().fg(status_color(run.state)))
        })
        .collect::<Vec<_>>();

    let block = table_block(" History ", app.focus == Focus::History);
    let table = Table::new(
        rows,
        [
            Constraint::Length(9),
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(2),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(SELECTED_BG))
    .block(block);

    if app.focus == Focus::History && !app.filtered_history_indices.is_empty() {
        app.history_table_state.select(Some(app.history_selected));
    } else {
        app.history_table_state.select(None);
    }
    frame.render_stateful_widget(table, area, &mut app.history_table_state);
}

fn table_block(title: &'static str, focused: bool) -> Block<'static> {
    Block::default()
        .title(Line::from(Span::styled(
            title,
            Style::default().fg(if focused { PRIMARY } else { DIM_WHITE }),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SEPARATOR))
        .style(Style::default().bg(BG))
}

fn draw_filter(frame: &mut ratatui::Frame<'_>, app: &MonitorApp, area: Rect) {
    let bar = Line::from(vec![
        Span::styled(" /", Style::default().fg(PRIMARY)),
        Span::styled(&app.filter_draft, Style::default().fg(WHITE)),
        Span::styled("▎", Style::default().fg(PRIMARY)),
    ]);
    frame.render_widget(Paragraph::new(bar).style(Style::default().bg(BG)), area);
}

fn draw_status(frame: &mut ratatui::Frame<'_>, app: &MonitorApp, area: Rect) {
    let mut spans = vec![
        Span::styled(" j/k", Style::default().fg(PRIMARY)),
        Span::styled(" navigate  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Tab", Style::default().fg(PRIMARY)),
        Span::styled(" switch  ", Style::default().fg(DIM_WHITE)),
        Span::styled("/", Style::default().fg(PRIMARY)),
        Span::styled(" filter  ", Style::default().fg(DIM_WHITE)),
        Span::styled("Enter", Style::default().fg(PRIMARY)),
        Span::styled(" detail  ", Style::default().fg(DIM_WHITE)),
        Span::styled("?", Style::default().fg(PRIMARY)),
        Span::styled(" help  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(PRIMARY)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ];
    if !app.filter_text.is_empty() && !app.filter_editing {
        spans.push(Span::styled("  filter: ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            app.filter_text.clone(),
            Style::default().fg(PRIMARY),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(BG)),
        area,
    );
}

fn draw_detail(frame: &mut ratatui::Frame<'_>, app: &mut MonitorApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    draw_detail_header(frame, app, chunks[0]);

    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    let render = detail_render(app, usize::MAX);
    let max_scroll = render.lines.len().saturating_sub(inner_height) as u16;
    let is_live = app
        .selected_run()
        .is_some_and(|run| run.state == RunState::Active);
    if app.detail_auto_scroll {
        app.detail_scroll = max_scroll;
    } else {
        app.detail_scroll = app.detail_scroll.min(max_scroll);
        if is_live && app.detail_scroll >= max_scroll {
            app.detail_auto_scroll = true;
        }
    }
    app.prompt_click_area = visible_content_range(
        chunks[1],
        render.prompt_range.clone(),
        app.detail_scroll as usize,
        inner_height,
    );
    app.prompt_more_area = render.prompt_more_line.and_then(|line| {
        visible_content_range(
            chunks[1],
            line..line.saturating_add(1),
            app.detail_scroll as usize,
            inner_height,
        )
    });
    app.prompt_more_hovered = app.prompt_more_hovered && app.prompt_more_area.is_some();
    let visible_lines = render
        .lines
        .into_iter()
        .skip(app.detail_scroll as usize)
        .take(inner_height)
        .collect::<Vec<_>>();
    let detail = Paragraph::new(visible_lines)
        .block(table_block(" Run detail ", true))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, chunks[1]);
    let mut footer_spans = vec![
        Span::styled(" Esc", Style::default().fg(PRIMARY)),
        Span::styled(" table  ", Style::default().fg(DIM_WHITE)),
        Span::styled("i", Style::default().fg(PRIMARY)),
        Span::styled(" info  ", Style::default().fg(DIM_WHITE)),
        Span::styled("p", Style::default().fg(PRIMARY)),
        Span::styled(" prompt  ", Style::default().fg(DIM_WHITE)),
        Span::styled("j/k", Style::default().fg(PRIMARY)),
        Span::styled(" scroll  ", Style::default().fg(DIM_WHITE)),
        Span::styled(
            format!("{}/{}", app.detail_scroll, max_scroll),
            Style::default().fg(DIM),
        ),
    ];
    if is_live {
        footer_spans.push(Span::styled("  G", Style::default().fg(PRIMARY)));
        footer_spans.push(Span::styled(" follow  ", Style::default().fg(DIM_WHITE)));
        if app.detail_auto_scroll {
            footer_spans.push(Span::styled(
                " FOLLOW ",
                Style::default()
                    .fg(BG)
                    .bg(PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    footer_spans.extend([
        Span::styled("  ?", Style::default().fg(PRIMARY)),
        Span::styled(" help  ", Style::default().fg(DIM_WHITE)),
        Span::styled("q", Style::default().fg(PRIMARY)),
        Span::styled(" quit", Style::default().fg(DIM_WHITE)),
    ]);
    let footer = Paragraph::new(Line::from(footer_spans)).style(Style::default().bg(BG));
    frame.render_widget(footer, chunks[2]);

    if app.show_run_info {
        draw_run_info_overlay(frame, app, area);
    }
}

fn visible_content_range(
    outer: Rect,
    content_range: Range<usize>,
    scroll: usize,
    height: usize,
) -> Option<Rect> {
    let visible_start = scroll;
    let visible_end = scroll.saturating_add(height);
    let start = content_range.start.max(visible_start);
    let end = content_range.end.min(visible_end);
    if start >= end {
        return None;
    }
    Some(Rect {
        x: outer.x.saturating_add(1),
        y: outer
            .y
            .saturating_add(1)
            .saturating_add(start.saturating_sub(visible_start) as u16),
        width: outer.width.saturating_sub(2),
        height: end.saturating_sub(start) as u16,
    })
}

fn draw_detail_header(frame: &mut ratatui::Frame<'_>, app: &MonitorApp, area: Rect) {
    let Some(run) = app.selected_run() else {
        draw_header(frame, app, area);
        return;
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", run.id),
            Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::styled(profile_label(run).to_string(), Style::default().fg(WHITE)),
        Span::styled("  ", Style::default()),
        Span::styled(
            run.interface.as_deref().unwrap_or("unknown").to_string(),
            Style::default().fg(DIM_WHITE),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(mode_label(run).to_string(), Style::default().fg(DIM)),
        Span::styled("  ", Style::default()),
        Span::styled(
            stage_label(run, app.tick),
            Style::default().fg(status_color(run.state)),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(run_duration(run), Style::default().fg(DIM_WHITE)),
    ]);
    frame.render_widget(Paragraph::new(line).style(Style::default().bg(BG)), area);
}

fn draw_run_info_overlay(frame: &mut ratatui::Frame<'_>, app: &MonitorApp, area: Rect) {
    let Some(run) = app.selected_run() else {
        return;
    };
    let text = run_info_text(run);
    let width = area.width.saturating_sub(4).clamp(42, 96);
    let value_width = width.saturating_sub(31).max(8) as usize;
    let lines = run_info_lines(text, value_width);
    let height = (lines.len() as u16)
        .saturating_add(3)
        .min(area.height.saturating_sub(2).max(1));
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Run info ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .style(Style::default().bg(BG));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn run_info_lines(text: Vec<String>, value_width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default()];
    for line in text {
        let Some((key, value)) = line.split_once(": ") else {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(DIM_WHITE),
            )));
            continue;
        };
        let wrapped = wrap_field_value(value, value_width);
        for (index, chunk) in wrapped.into_iter().enumerate() {
            let key_text = if index == 0 {
                format!("{key:<22}")
            } else {
                " ".repeat(22)
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(key_text, Style::default().fg(DIM)),
                Span::styled(chunk, Style::default().fg(WHITE)),
            ]));
        }
    }
    lines
}

fn wrap_field_value(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let width = width.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if current.chars().count() == width {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn draw_help(frame: &mut ratatui::Frame<'_>, app: &MonitorApp) {
    let shortcuts = match app.mode {
        AppMode::Table => vec![
            ("j / ↓", "Move down"),
            ("k / ↑", "Move up"),
            ("Tab", "Switch Active and History focus"),
            ("Enter", "Open detail view"),
            ("/", "Filter history"),
            ("Esc / q", "Quit"),
            ("?", "Toggle this help"),
        ],
        AppMode::Detail => vec![
            ("j / ↓", "Scroll down"),
            ("k / ↑", "Scroll up"),
            ("d / PageDown", "Page down"),
            ("u / PageUp", "Page up"),
            ("g / G", "Scroll to top or bottom"),
            ("i", "Toggle run info"),
            ("p / click", "Expand or collapse prompt"),
            ("Esc", "Back to table"),
            ("q", "Quit"),
            ("?", "Toggle this help"),
        ],
    };
    let width = 44;
    let height = shortcuts.len() as u16 + 4;
    let area = frame.area();
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Shortcuts ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .style(Style::default().bg(BG));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = vec![Line::from("")];
    for (key, action) in shortcuts {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{key:<13}"), Style::default().fg(PRIMARY)),
            Span::styled(action, Style::default().fg(WHITE)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
}

fn print_snapshot(app: &MonitorApp) {
    println!(
        "sideagent-monitor · {} profiles · {} active runs · {} completed",
        app.runs
            .iter()
            .filter_map(|run| run.profile_name.as_deref())
            .collect::<HashSet<_>>()
            .len(),
        app.active_indices.len(),
        app.runs
            .iter()
            .filter(|run| matches!(run.state, RunState::Success | RunState::Failed))
            .count()
    );
    println!();
    println!("Active");
    println!("Started | Project | Profile | Interface | Mode | Stage | Dur");
    if app.active_indices.is_empty() {
        println!("(none)");
    } else {
        for &index in &app.active_indices {
            let run = &app.runs[index];
            println!(
                "{} | {} | {} | {} | {} | {} | {}",
                short_time(run.started_at.as_deref()),
                project_label(run),
                profile_label(run),
                run.interface.as_deref().unwrap_or("unknown"),
                mode_label(run),
                stage_label(run, app.tick),
                run_duration(run)
            );
        }
    }
    println!();
    println!("History");
    if !app.filter_text.is_empty() {
        println!("filter: {}", app.filter_text);
    }
    println!("Time | Project | Profile | Interface | Mode | Duration | Exit | ✓");
    if app.filtered_history_indices.is_empty() {
        println!("(none)");
    } else {
        for &index in &app.filtered_history_indices {
            let run = &app.runs[index];
            println!(
                "{} | {} | {} | {} | {} | {} | {} | {}",
                short_time(run.completed_at.as_deref().or(run.started_at.as_deref())),
                project_label(run),
                profile_label(run),
                run.interface.as_deref().unwrap_or("unknown"),
                mode_label(run),
                run_duration(run),
                run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                status_icon(run.state)
            );
        }
    }
    println!();
    println!("Detail");
    for line in detail_text(app, usize::MAX) {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_run(path: PathBuf, name: &str, started_at: &str, status: &str, text: &str) {
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("metadata.json"),
            format!(
                r#"{{"profile":{{"name":"{name}","command":"agent","args":[]}},"interface":"claude","prompt_delivery":"argument","started_at":"{started_at}","completed_at":"2026-06-09T02:00:00Z","status":"{status}","exit_code":0}}"#
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

    fn write_prompt(path: PathBuf, lines: usize) {
        let prompt = (1..=lines)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path.join("prompt.md"), prompt).unwrap();
    }

    fn write_active_run(path: PathBuf, name: &str, started_at: &str, text: &str) {
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("metadata.json"),
            format!(
                r#"{{"profile":{{"name":"{name}","command":"agent","args":[]}},"interface":"claude","prompt_delivery":"argument","started_at":"{started_at}","status":"running"}}"#
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
    fn app_poll_splits_active_and_history() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-active"),
            "demo",
            "2026-06-09T00:00:00Z",
            "hello",
        );
        write_run(
            dir.path().join("run-done"),
            "demo",
            "2026-06-09T01:00:00Z",
            "success",
            "done",
        );

        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();

        assert_eq!(app.active_indices.len(), 1);
        assert_eq!(app.filtered_history_indices.len(), 1);
        assert_eq!(
            app.transcripts[&dir.path().join("run-active")].lines,
            vec!["[text]  hello"]
        );
    }

    #[test]
    fn app_preserves_transcript_cache_when_switching_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        write_active_run(
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
        assert_eq!(line_count(&app, "  [text]  second"), 1);

        app.select_next();
        app.poll().unwrap();
        assert_eq!(line_count(&app, "  [text]  first"), 1);

        app.select_previous();
        app.poll().unwrap();
        assert_eq!(line_count(&app, "  [text]  second"), 1);
    }

    #[test]
    fn arrow_down_moves_from_active_to_history() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-active"),
            "active",
            "2026-06-09T00:00:00Z",
            "active",
        );
        write_run(
            dir.path().join("run-done"),
            "done",
            "2026-06-09T01:00:00Z",
            "success",
            "done",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.focus = Focus::Active;
        app.select_next();
        assert_eq!(app.focus, Focus::History);
    }

    #[test]
    fn arrow_up_moves_from_history_to_active() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-active"),
            "active",
            "2026-06-09T00:00:00Z",
            "active",
        );
        write_run(
            dir.path().join("run-done"),
            "done",
            "2026-06-09T01:00:00Z",
            "success",
            "done",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.focus = Focus::History;
        app.select_previous();
        assert_eq!(app.focus, Focus::Active);
    }

    #[test]
    fn app_preserves_selected_run_when_poll_resorts_runs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        write_active_run(
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
        let selected_path = app.selected_run().unwrap().path.clone();

        write_active_run(
            dir.path().join("run-c"),
            "c",
            "2026-06-09T02:00:00Z",
            "third",
        );
        app.poll().unwrap();

        assert_eq!(app.selected_run().unwrap().path, selected_path);
    }

    #[test]
    fn history_filter_matches_profile() {
        let dir = tempfile::TempDir::new().unwrap();
        write_run(
            dir.path().join("run-a"),
            "alpha",
            "2026-06-09T00:00:00Z",
            "success",
            "first",
        );
        write_run(
            dir.path().join("run-b"),
            "beta",
            "2026-06-09T01:00:00Z",
            "success",
            "second",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.filter_text = "alpha".to_string();
        app.rebuild_filter();
        assert_eq!(app.filtered_history_indices.len(), 1);
        assert_eq!(
            profile_label(&app.runs[app.filtered_history_indices[0]]),
            "alpha"
        );
    }

    #[test]
    fn run_info_lines_wrap_long_values() {
        let lines = run_info_lines(vec!["args: abcdefghij".to_string()], 4);
        let rendered = lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered[1], "  args                  abcd");
        assert_eq!(rendered[2], "                        efgh");
        assert_eq!(rendered[3], "                        ij");
    }

    #[test]
    fn detail_places_artifacts_before_transcript() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        let lines = detail_text(&app, usize::MAX);
        let artifacts = lines.iter().position(|line| line == "Artifacts").unwrap();
        let transcript = lines.iter().position(|line| line == "Transcript").unwrap();
        assert!(artifacts < transcript);
    }

    #[test]
    fn transcript_thinking_line_uses_icon() {
        let line = transcript_line("[think] I will inspect the file".to_string());
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "  💭 I will inspect the file");
        assert_eq!(line.spans[1].style.fg, Some(DIM_WHITE));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn transcript_tool_lines_split_label_name_and_detail() {
        let line = transcript_line("[tool→] Read#01  file_path=/tmp/input".to_string());
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "  ▸ tool→ Read#01  file_path=/tmp/input");
        assert_eq!(line.spans[4].style.fg, Some(DIM_WHITE));
        assert!(!line.spans[4].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[5].style.fg, Some(DIM));
    }

    #[test]
    fn detail_collapses_long_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        let run_path = dir.path().join("run-a");
        write_active_run(run_path.clone(), "a", "2026-06-09T00:00:00Z", "first");
        write_prompt(run_path, 12);
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        let lines = detail_text(&app, usize::MAX);
        assert!(lines.contains(&"  line 10".to_string()));
        assert!(lines.contains(&"  +2 more lines".to_string()));
        assert!(!lines.contains(&"  line 11".to_string()));
    }

    #[test]
    fn prompt_blank_lines_render_without_indent() {
        let lines = render_prompt_block(
            vec!["before".to_string(), String::new(), "after".to_string()],
            true,
            false,
        );
        assert!(lines[1].spans.is_empty());
    }

    #[test]
    fn prompt_more_line_has_hover_style() {
        let lines = render_prompt_block(
            (1..=12).map(|line| format!("line {line}")).collect(),
            false,
            true,
        );
        assert_eq!(lines[10].spans[1].style.bg, Some(SELECTED_BG));
        assert_eq!(lines[10].spans[1].style.fg, Some(WHITE));
    }

    #[test]
    fn prompt_markdown_renders_styled_spans() {
        let lines = render_prompt_block(
            vec![
                "# Title".to_string(),
                String::new(),
                "Use `code` and **bold**".to_string(),
            ],
            true,
            false,
        );

        let title = lines[0].spans[1].style;
        let code = lines[2].spans[2].style;
        let bold = lines[2].spans[4].style;

        assert_eq!(title.fg, Some(PRIMARY));
        assert!(title.add_modifier.contains(Modifier::BOLD));
        assert_eq!(code.fg, Some(YELLOW));
        assert!(bold.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn prompt_markdown_preserves_collapsed_logical_lines() {
        let lines = render_prompt_block(
            vec![
                "# Heading".to_string(),
                String::new(),
                "one".to_string(),
                "two".to_string(),
                "three".to_string(),
                "four".to_string(),
                "five".to_string(),
                "six".to_string(),
                "seven".to_string(),
                "eight".to_string(),
                "nine".to_string(),
            ],
            false,
            false,
        );

        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(rendered.iter().any(|line| line == "    eight"));
        assert!(rendered.iter().any(|line| line == "    +1 more lines"));
        assert!(!rendered.iter().any(|line| line == "    nine"));
    }

    #[test]
    fn detail_expands_long_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        let run_path = dir.path().join("run-a");
        write_active_run(run_path.clone(), "a", "2026-06-09T00:00:00Z", "first");
        write_prompt(run_path, 12);
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.prompt_expanded = true;
        let lines = detail_text(&app, usize::MAX);
        assert!(lines.contains(&"  line 11".to_string()));
        assert!(!lines.contains(&"  +2 more lines".to_string()));
    }

    #[test]
    fn hovering_prompt_sets_hover_state_for_more_line() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.mode = AppMode::Detail;
        app.prompt_click_area = Some(Rect {
            x: 2,
            y: 3,
            width: 10,
            height: 4,
        });
        app.prompt_more_area = Some(Rect {
            x: 2,
            y: 6,
            width: 10,
            height: 1,
        });
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: MouseEventKind::Moved,
                column: 4,
                row: 3,
                modifiers: KeyModifiers::empty(),
            },
        );
        assert!(app.prompt_more_hovered);
    }

    #[test]
    fn hovering_outside_prompt_clears_hover_state() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.mode = AppMode::Detail;
        app.prompt_more_hovered = true;
        app.prompt_click_area = Some(Rect {
            x: 2,
            y: 3,
            width: 10,
            height: 4,
        });
        app.prompt_more_area = Some(Rect {
            x: 2,
            y: 6,
            width: 10,
            height: 1,
        });
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: MouseEventKind::Moved,
                column: 20,
                row: 3,
                modifiers: KeyModifiers::empty(),
            },
        );
        assert!(!app.prompt_more_hovered);
    }

    #[test]
    fn clicking_visible_prompt_toggles_expansion() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.mode = AppMode::Detail;
        app.prompt_click_area = Some(Rect {
            x: 2,
            y: 3,
            width: 10,
            height: 2,
        });
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row: 4,
                modifiers: KeyModifiers::empty(),
            },
        );
        assert!(app.prompt_expanded);
    }

    #[test]
    fn selecting_another_run_collapses_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        write_active_run(
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
        app.prompt_expanded = true;
        app.select_next();
        assert!(!app.prompt_expanded);
    }

    #[test]
    fn entering_active_detail_enables_follow() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        handle_table_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert!(app.detail_auto_scroll);
        assert_eq!(app.detail_scroll, u16::MAX);
    }

    #[test]
    fn scrolling_up_disables_follow() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.mode = AppMode::Detail;
        app.detail_auto_scroll = true;
        handle_detail_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        assert!(!app.detail_auto_scroll);
    }

    #[test]
    fn detail_scroll_is_clamped_to_content() {
        let dir = tempfile::TempDir::new().unwrap();
        write_active_run(
            dir.path().join("run-a"),
            "a",
            "2026-06-09T00:00:00Z",
            "first",
        );
        let mut app = MonitorApp::new(
            MonitorCore::new(dir.path().to_path_buf()),
            Duration::from_millis(50),
        );
        app.poll().unwrap();
        app.detail_scroll = u16::MAX;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| draw_detail(frame, &mut app, frame.area()))
            .unwrap();
        assert!(app.detail_scroll < u16::MAX);
    }

    #[test]
    fn cleanup_terminal_startup_leaves_alternate_screen_after_enter() {
        let mut output = Vec::new();
        cleanup_terminal_startup(&mut output, true, false);
        assert_eq!(output, b"\x1b[?1049l");
    }

    #[test]
    fn cleanup_terminal_startup_does_not_leave_alternate_screen_before_enter() {
        let mut output = Vec::new();
        cleanup_terminal_startup(&mut output, false, false);
        assert!(output.is_empty());
    }

    #[test]
    fn cleanup_terminal_startup_disables_mouse_capture_after_enter() {
        let mut output = Vec::new();
        cleanup_terminal_startup(&mut output, true, true);
        assert_eq!(
            output,
            b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1049l"
        );
    }
}
