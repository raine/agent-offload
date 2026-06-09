use anyhow::Result;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use super::render::{CompactRenderer, RendererKind};

pub(crate) struct TailUpdate {
    pub(crate) lines: Vec<String>,
    pub(crate) pending_raw: Option<String>,
    pub(crate) offset: u64,
}

pub(crate) struct JsonlTailer {
    offset: u64,
    pending: Vec<u8>,
    renderer: CompactRenderer,
    kind: RendererKind,
}

impl JsonlTailer {
    pub(crate) fn new(kind: RendererKind) -> Self {
        Self {
            offset: 0,
            pending: Vec::new(),
            renderer: CompactRenderer::new(kind),
            kind,
        }
    }

    pub(crate) fn kind(&self) -> RendererKind {
        self.kind
    }

    pub(crate) fn poll_path(&mut self, path: &Path) -> Result<TailUpdate> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if self.offset > 0 || !self.pending.is_empty() {
                    self.reset();
                }
                return Ok(TailUpdate {
                    lines: Vec::new(),
                    pending_raw: None,
                    offset: self.offset,
                });
            }
            Err(error) => return Err(error.into()),
        };
        let len = file.metadata()?.len();
        if len < self.offset {
            self.reset();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut new_bytes = Vec::new();
        file.read_to_end(&mut new_bytes)?;
        self.offset += new_bytes.len() as u64;
        Ok(self.update(new_bytes))
    }

    fn update(&mut self, new_bytes: Vec<u8>) -> TailUpdate {
        self.pending.extend_from_slice(&new_bytes);
        let mut lines = Vec::new();

        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let record = self.pending.drain(..=newline).collect::<Vec<_>>();
            let line = String::from_utf8_lossy(&record[..record.len().saturating_sub(1)]);
            if line.is_empty() {
                continue;
            }
            lines.extend(self.renderer.render_jsonl_line(&line).lines);
        }

        let pending_raw = if self.pending.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&self.pending).into_owned())
        };

        TailUpdate {
            lines,
            pending_raw,
            offset: self.offset,
        }
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.renderer = CompactRenderer::new(self.kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    #[test]
    fn tail_reads_only_new_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("stdout.jsonl");
        fs::write(
            &path,
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"first\"}]}}\n",
        )
        .unwrap();
        let mut tailer = JsonlTailer::new(RendererKind::Claude);

        assert_eq!(
            tailer.poll_path(&path).unwrap().lines,
            vec!["[text]  first"]
        );

        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(
            file,
            "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"second\"}}]}}}}"
        )
        .unwrap();

        assert_eq!(
            tailer.poll_path(&path).unwrap().lines,
            vec!["[text]  second"]
        );
    }

    #[test]
    fn tail_partial_jsonl_is_buffered() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("stdout.jsonl");
        fs::write(
            &path,
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"later\"}]",
        )
        .unwrap();
        let mut tailer = JsonlTailer::new(RendererKind::Claude);

        let first = tailer.poll_path(&path).unwrap();
        assert!(first.lines.is_empty());
        assert!(first.pending_raw.is_some());

        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "}}}}").unwrap();

        assert_eq!(
            tailer.poll_path(&path).unwrap().lines,
            vec!["[text]  later"]
        );
    }

    #[test]
    fn tail_malformed_jsonl_renders_raw() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("stdout.jsonl");
        fs::write(&path, b"not json\n").unwrap();
        let mut tailer = JsonlTailer::new(RendererKind::Claude);

        assert_eq!(
            tailer.poll_path(&path).unwrap().lines,
            vec!["[raw]   not json"]
        );
    }

    #[test]
    fn tail_missing_file_returns_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("stdout.jsonl");
        let mut tailer = JsonlTailer::new(RendererKind::Claude);

        let update = tailer.poll_path(&path).unwrap();
        assert!(update.lines.is_empty());
        assert!(update.pending_raw.is_none());
    }

    #[test]
    fn tail_truncation_resets_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("stdout.jsonl");
        fs::write(
            &path,
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"first with enough bytes to exceed the rewrite\"}]}}\n",
        )
        .unwrap();
        let mut tailer = JsonlTailer::new(RendererKind::Claude);
        tailer.poll_path(&path).unwrap();

        fs::write(
            &path,
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"restarted\"}]}}\n",
        )
        .unwrap();

        assert_eq!(
            tailer.poll_path(&path).unwrap().lines,
            vec!["[text]  restarted"]
        );
    }
}
