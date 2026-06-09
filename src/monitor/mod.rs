#![allow(dead_code)]

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

mod render;
mod runs;
mod tail;

pub(crate) use render::{CompactRenderer, RenderedTail, RendererKind};
pub(crate) use runs::{RunSummary, poll_runs};
pub(crate) use tail::{JsonlTailer, TailUpdate};

pub(crate) struct MonitorCore {
    runs_root: PathBuf,
    tailers: HashMap<PathBuf, JsonlTailer>,
}

impl MonitorCore {
    pub(crate) fn new(runs_root: PathBuf) -> Self {
        Self {
            runs_root,
            tailers: HashMap::new(),
        }
    }

    pub(crate) fn default_root() -> Result<PathBuf> {
        crate::run_dir::runs_root()
    }

    pub(crate) fn default() -> Result<Self> {
        Ok(Self::new(Self::default_root()?))
    }

    pub(crate) fn poll_runs(&self) -> Result<Vec<RunSummary>> {
        poll_runs(&self.runs_root)
    }

    pub(crate) fn poll_stdout(&mut self, run: &RunSummary) -> Result<TailUpdate> {
        let kind = RendererKind::from_interface(run.interface.as_deref().unwrap_or(""));
        let needs_new_tailer = self
            .tailers
            .get(&run.stdout_file)
            .map(|tailer| tailer.kind() != kind)
            .unwrap_or(false);
        if needs_new_tailer {
            self.tailers.remove(&run.stdout_file);
        }
        self.tailers
            .entry(run.stdout_file.clone())
            .or_insert_with(|| JsonlTailer::new(kind))
            .poll_path(&run.stdout_file)
    }
}

#[cfg(test)]
mod tests {
    use super::runs::RunState;
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn monitor_core_polls_runs_and_stdout() {
        let dir = tempfile::TempDir::new().unwrap();
        let run_path = dir.path().join("123-test");
        fs::create_dir_all(&run_path).unwrap();
        fs::write(
            run_path.join("metadata.json"),
            r#"{
  "profile": {"name": "test", "command": "agent", "args": []},
  "interface": "claude",
  "prompt_delivery": "argument",
  "started_at": "2026-06-09T00:00:00Z",
  "status": "running"
}"#,
        )
        .unwrap();
        fs::write(
            run_path.join("stdout.jsonl"),
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
        )
        .unwrap();

        let mut core = MonitorCore::new(dir.path().to_path_buf());
        let runs = core.poll_runs().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, RunState::Active);

        let first = core.poll_stdout(&runs[0]).unwrap();
        assert_eq!(first.lines, vec!["[text]  hello"]);

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(run_path.join("stdout.jsonl"))
            .unwrap();
        writeln!(
            file,
            "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"world\"}}]}}}}"
        )
        .unwrap();

        let second = core.poll_stdout(&runs[0]).unwrap();
        assert_eq!(second.lines, vec!["[text]  world"]);
    }
}
