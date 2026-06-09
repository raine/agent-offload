use anyhow::Result;
use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::run_dir::RunDir;

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
                started_at: None,
                completed_at: None,
                exit_code: None,
                completion_event_seen: None,
                failure: None,
                metadata_error: Some(error.to_string()),
            };
        }
    };

    RunSummary {
        id,
        path,
        stdout_file,
        state: RunState::from_status(metadata.status.as_deref()),
        profile_name: metadata.profile.as_ref().and_then(|p| p.name.clone()),
        profile_command: metadata.profile.as_ref().and_then(|p| p.command.clone()),
        profile_args: metadata
            .profile
            .as_ref()
            .and_then(|p| p.args.clone())
            .unwrap_or_default(),
        interface: metadata.interface,
        prompt_delivery: metadata.prompt_delivery,
        started_at: metadata.started_at,
        completed_at: metadata.completed_at,
        exit_code: metadata.exit_code,
        completion_event_seen: metadata.completion_event_seen,
        failure: metadata.failure,
        metadata_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_metadata(path: PathBuf, status: &str, started_at: &str) {
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
  "status": "{status}"
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
