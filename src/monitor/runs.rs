use anyhow::Result;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::run_dir::RunDir;
use crate::tmux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunState {
    Active,
    Success,
    Failed,
    Unknown,
}

impl RunState {
    fn from_status(status: Option<&str>) -> Self {
        match status {
            Some("running") => Self::Active,
            Some("success") => Self::Success,
            Some("failed") => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RunSummary {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) stdout_file: PathBuf,
    pub(crate) state: RunState,
    pub(crate) profile_name: Option<String>,
    pub(crate) profile_command: Option<String>,
    pub(crate) profile_args: Vec<String>,
    pub(crate) interface: Option<String>,
    pub(crate) prompt_delivery: Option<String>,
    pub(crate) pid: Option<u32>,
    pub(crate) tmux_pane_id: Option<String>,
    pub(crate) started_at: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) completion_event_seen: Option<bool>,
    pub(crate) failure: Option<String>,
    pub(crate) metadata_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunMetadata {
    profile: Option<RunProfileMetadata>,
    interface: Option<String>,
    prompt_delivery: Option<String>,
    pid: Option<u32>,
    tmux_pane_id: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    status: Option<String>,
    exit_code: Option<i32>,
    completion_event_seen: Option<bool>,
    failure: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunProfileMetadata {
    name: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
}

pub(crate) fn poll_runs(runs_root: &Path) -> Result<Vec<RunSummary>> {
    if !runs_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut runs = Vec::new();
    for entry in fs::read_dir(runs_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let run_dir = RunDir::at(path);
        runs.push(load_run_summary(id, run_dir));
    }

    runs.sort_by(compare_runs);
    Ok(runs)
}

fn compare_runs(a: &RunSummary, b: &RunSummary) -> std::cmp::Ordering {
    let a_ts = parse_started_at(a.started_at.as_deref());
    let b_ts = parse_started_at(b.started_at.as_deref());
    match (a_ts, b_ts) {
        (Some(a_ts), Some(b_ts)) => b_ts.cmp(&a_ts).then_with(|| b.id.cmp(&a.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.id.cmp(&a.id),
    }
}

fn parse_started_at(value: Option<&str>) -> Option<DateTime<FixedOffset>> {
    value.and_then(|value| DateTime::parse_from_rfc3339(value).ok())
}

fn reconcile_run_state(metadata: &RunMetadata, run_id: &str) -> RunState {
    let state = RunState::from_status(metadata.status.as_deref());
    if state != RunState::Active {
        return state;
    }
    match run_liveness(metadata, run_id) {
        Liveness::Alive | Liveness::Unknown => RunState::Active,
        Liveness::Dead(_) => RunState::Failed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Liveness {
    Alive,
    Dead(DeadReason),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeadReason {
    TmuxPane,
    Process,
    RunOwner,
}

fn run_liveness(metadata: &RunMetadata, run_id: &str) -> Liveness {
    if let Some(pane_id) = metadata.tmux_pane_id.as_deref() {
        return match tmux::pane_status(pane_id) {
            Ok(tmux::PaneStatus::Alive) => Liveness::Alive,
            Ok(tmux::PaneStatus::Dead | tmux::PaneStatus::Missing) => {
                Liveness::Dead(DeadReason::TmuxPane)
            }
            Err(_) => Liveness::Unknown,
        };
    }
    if let Some(pid) = metadata.pid {
        return process_liveness(pid, DeadReason::Process);
    }
    if let Some(pid) = run_owner_pid(run_id) {
        return process_liveness(pid, DeadReason::RunOwner);
    }
    Liveness::Unknown
}

fn process_liveness(pid: u32, dead_reason: DeadReason) -> Liveness {
    match process_status(pid) {
        ProcessStatus::Alive => Liveness::Alive,
        ProcessStatus::Dead => Liveness::Dead(dead_reason),
        ProcessStatus::Unknown => Liveness::Unknown,
    }
}

fn run_owner_pid(run_id: &str) -> Option<u32> {
    let (_, pid) = run_id.rsplit_once('-')?;
    pid.parse().ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessStatus {
    Alive,
    Dead,
    Unknown,
}

fn process_status(pid: u32) -> ProcessStatus {
    if pid == 0 {
        return ProcessStatus::Dead;
    }

    let current = std::process::id();
    if pid == current {
        return ProcessStatus::Alive;
    }

    #[cfg(unix)]
    {
        unix_process_status(pid)
    }
    #[cfg(not(unix))]
    {
        ProcessStatus::Unknown
    }
}

#[cfg(unix)]
fn unix_process_status(pid: u32) -> ProcessStatus {
    use std::process::Command;

    let output = Command::new("kill").arg("-0").arg(pid.to_string()).output();
    match output {
        Ok(output) if output.status.success() => ProcessStatus::Alive,
        Ok(_) => ProcessStatus::Dead,
        Err(_) => ProcessStatus::Unknown,
    }
}

fn reconcile_completed_at(metadata: &RunMetadata, run_id: &str) -> Option<String> {
    if metadata.completed_at.is_some() {
        return metadata.completed_at.clone();
    }
    if RunState::from_status(metadata.status.as_deref()) != RunState::Active {
        return None;
    }
    match run_liveness(metadata, run_id) {
        Liveness::Dead(_) => stale_completed_at(metadata),
        Liveness::Alive | Liveness::Unknown => None,
    }
}

fn stale_completed_at(metadata: &RunMetadata) -> Option<String> {
    metadata
        .started_at
        .as_deref()
        .and_then(|started| parse_started_at(Some(started)))
        .map(|started| started.to_rfc3339())
        .or_else(|| Some(Utc::now().to_rfc3339()))
}

fn stale_failure_message(metadata: &RunMetadata, run_id: &str) -> Option<String> {
    if metadata.status.as_deref() != Some("running") {
        return metadata.failure.clone();
    }
    if metadata.failure.is_some() {
        return metadata.failure.clone();
    }
    match run_liveness(metadata, run_id) {
        Liveness::Dead(DeadReason::TmuxPane) => {
            Some("recorded tmux pane is no longer alive".to_string())
        }
        Liveness::Dead(DeadReason::Process) => {
            Some("recorded process is no longer alive".to_string())
        }
        Liveness::Dead(DeadReason::RunOwner) => {
            Some("run owner process from archive id is no longer alive".to_string())
        }
        Liveness::Alive | Liveness::Unknown => None,
    }
}

fn load_run_summary(id: String, run_dir: RunDir) -> RunSummary {
    let metadata_path = run_dir.metadata_file.clone();
    let stdout_file = run_dir.stdout_file.clone();
    let path = run_dir.path.clone();

    let metadata = match fs::read_to_string(&metadata_path) {
        Ok(contents) => match serde_json::from_str::<RunMetadata>(&contents) {
            Ok(metadata) => metadata,
            Err(error) => {
                return RunSummary {
                    id,
                    path,
                    stdout_file,
                    state: RunState::Unknown,
                    profile_name: None,
                    profile_command: None,
                    profile_args: Vec::new(),
                    interface: None,
                    prompt_delivery: None,
                    pid: None,
                    tmux_pane_id: None,
                    started_at: None,
                    completed_at: None,
                    exit_code: None,
                    completion_event_seen: None,
                    failure: None,
                    metadata_error: Some(error.to_string()),
                };
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RunSummary {
                id,
                path,
                stdout_file,
                state: RunState::Unknown,
                profile_name: None,
                profile_command: None,
                profile_args: Vec::new(),
                interface: None,
                prompt_delivery: None,
                pid: None,
                tmux_pane_id: None,
                started_at: None,
                completed_at: None,
                exit_code: None,
                completion_event_seen: None,
                failure: None,
                metadata_error: None,
            };
        }
        Err(error) => {
            return RunSummary {
                id,
                path,
                stdout_file,
                state: RunState::Unknown,
                profile_name: None,
                profile_command: None,
                profile_args: Vec::new(),
                interface: None,
                prompt_delivery: None,
                pid: None,
                tmux_pane_id: None,
                started_at: None,
                completed_at: None,
                exit_code: None,
                completion_event_seen: None,
                failure: None,
                metadata_error: Some(error.to_string()),
            };
        }
    };

    let state = reconcile_run_state(&metadata, &id);
    let completed_at = reconcile_completed_at(&metadata, &id);
    let failure = stale_failure_message(&metadata, &id);

    RunSummary {
        id,
        path,
        stdout_file,
        state,
        profile_name: metadata.profile.as_ref().and_then(|p| p.name.clone()),
        profile_command: metadata.profile.as_ref().and_then(|p| p.command.clone()),
        profile_args: metadata
            .profile
            .as_ref()
            .and_then(|p| p.args.clone())
            .unwrap_or_default(),
        interface: metadata.interface,
        prompt_delivery: metadata.prompt_delivery,
        pid: metadata.pid,
        tmux_pane_id: metadata.tmux_pane_id,
        started_at: metadata.started_at,
        completed_at,
        exit_code: metadata.exit_code,
        completion_event_seen: metadata.completion_event_seen,
        failure,
        metadata_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_metadata(path: PathBuf, status: &str, started_at: &str) {
        write_metadata_fields(path, status, started_at, "");
    }

    fn write_metadata_fields(path: PathBuf, status: &str, started_at: &str, fields: &str) {
        fs::create_dir_all(&path).unwrap();
        let metadata = format!(
            r#"{{
  "profile": {{
    "name": "test",
    "command": "agent",
    "args": []
  }},
  "interface": "claude",
  "prompt_delivery": "argument",
  "started_at": "{started_at}",
  "status": "{status}"{fields}
}}"#
        );
        fs::write(path.join("metadata.json"), metadata).unwrap();
    }

    #[test]
    fn poll_runs_returns_sorted_summaries_and_states() {
        let dir = tempfile::TempDir::new().unwrap();
        write_metadata(dir.path().join("old"), "running", "2026-06-09T00:00:00Z");
        write_metadata(dir.path().join("new"), "success", "2026-06-09T01:00:00Z");
        write_metadata(dir.path().join("failed"), "failed", "2026-06-09T00:30:00Z");
        fs::create_dir_all(dir.path().join("unknown")).unwrap();

        let runs = poll_runs(dir.path()).unwrap();

        assert_eq!(
            runs.iter().map(|run| run.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "failed", "old", "unknown"]
        );
        assert_eq!(runs[0].state, RunState::Success);
        assert_eq!(runs[1].state, RunState::Failed);
        assert_eq!(runs[2].state, RunState::Active);
        assert_eq!(runs[3].state, RunState::Unknown);
    }

    #[test]
    fn poll_runs_marks_dead_pid_run_as_failed_history() {
        let dir = tempfile::TempDir::new().unwrap();
        write_metadata_fields(
            dir.path().join("dead"),
            "running",
            "2026-06-09T00:00:00Z",
            r#",
  "pid": 99999999,
  "completed_at": null,
  "exit_code": null,
  "completion_event_seen": null"#,
        );

        let runs = poll_runs(dir.path()).unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, RunState::Failed);
        assert_eq!(
            runs[0].completed_at.as_deref(),
            Some("2026-06-09T00:00:00+00:00")
        );
        assert_eq!(
            runs[0].failure.as_deref(),
            Some("recorded process is no longer alive")
        );
    }

    #[test]
    fn poll_runs_keeps_current_pid_active() {
        let dir = tempfile::TempDir::new().unwrap();
        write_metadata_fields(
            dir.path().join("live"),
            "running",
            "2026-06-09T00:00:00Z",
            &format!(
                r#",
  "pid": {}"#,
                std::process::id()
            ),
        );

        let runs = poll_runs(dir.path()).unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, RunState::Active);
        assert!(runs[0].failure.is_none());
    }

    #[test]
    fn poll_runs_marks_running_run_with_dead_owner_pid_as_failed_history() {
        let dir = tempfile::TempDir::new().unwrap();
        write_metadata(
            dir.path().join("1781071956577-99999999"),
            "running",
            "2026-06-09T00:00:00Z",
        );

        let runs = poll_runs(dir.path()).unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, RunState::Failed);
        assert_eq!(
            runs[0].completed_at.as_deref(),
            Some("2026-06-09T00:00:00+00:00")
        );
        assert_eq!(
            runs[0].failure.as_deref(),
            Some("run owner process from archive id is no longer alive")
        );
    }

    #[test]
    fn poll_runs_missing_root_returns_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("missing");
        assert!(poll_runs(&missing).unwrap().is_empty());
    }

    #[test]
    fn poll_runs_malformed_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("metadata.json"), "{not json").unwrap();

        let runs = poll_runs(dir.path()).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, RunState::Unknown);
        assert!(runs[0].metadata_error.is_some());
    }
}
