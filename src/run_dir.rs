use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RunDir {
    pub path: PathBuf,
    pub prompt_file: PathBuf,
    pub done_file: PathBuf,
    pub launcher_file: PathBuf,
}

pub fn create() -> Result<RunDir> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let runs_dir = home
        .join(".local")
        .join("state")
        .join("agent-offload")
        .join("runs");
    fs::create_dir_all(&runs_dir).context("could not create run directory root")?;

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_millis();
    let path = runs_dir.join(format!("{}-{}", millis, std::process::id()));
    fs::create_dir(&path).context("could not create run directory")?;

    Ok(RunDir {
        prompt_file: path.join("prompt.md"),
        done_file: path.join("done.md"),
        launcher_file: path.join("launch.sh"),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_run_dir_creates_files_in_temp_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test-run");

        let run_dir = RunDir {
            prompt_file: path.join("prompt.md"),
            done_file: path.join("done.md"),
            launcher_file: path.join("launch.sh"),
            path,
        };

        fs::create_dir_all(&run_dir.path).unwrap();
        assert!(run_dir.path.exists());
        assert_eq!(run_dir.prompt_file.file_name().unwrap(), "prompt.md");
        assert_eq!(run_dir.done_file.file_name().unwrap(), "done.md");
        assert_eq!(run_dir.launcher_file.file_name().unwrap(), "launch.sh");
    }
}
