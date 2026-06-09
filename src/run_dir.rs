use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RunDir {
    pub path: PathBuf,
    pub prompt_file: PathBuf,
    pub done_file: PathBuf,
    pub launcher_file: PathBuf,
    pub metadata_file: PathBuf,
    pub stdout_file: PathBuf,
}

impl RunDir {
    pub fn at(path: PathBuf) -> Self {
        Self {
            prompt_file: path.join("prompt.md"),
            done_file: path.join("done.md"),
            launcher_file: path.join("launch.sh"),
            metadata_file: path.join("metadata.json"),
            stdout_file: path.join("stdout.jsonl"),
            path,
        }
    }
}

pub fn create() -> Result<RunDir> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let runs_dir = home
        .join(".local")
        .join("state")
        .join("sideagent")
        .join("runs");
    fs::create_dir_all(&runs_dir).context("could not create run directory root")?;
    make_private(&runs_dir)?;

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_millis();
    let path = runs_dir.join(format!("{}-{}", millis, std::process::id()));
    fs::create_dir(&path).context("could not create run directory")?;
    make_private(&path)?;

    Ok(RunDir::at(path))
}

#[cfg(unix)]
fn make_private(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("could not stat {}", path.display()))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("could not set permissions on {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_private(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_run_dir_creates_files_in_temp_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test-run");
        let run_dir = RunDir::at(path);

        fs::create_dir_all(&run_dir.path).unwrap();
        assert!(run_dir.path.exists());
        assert_eq!(run_dir.prompt_file.file_name().unwrap(), "prompt.md");
        assert_eq!(run_dir.done_file.file_name().unwrap(), "done.md");
        assert_eq!(run_dir.launcher_file.file_name().unwrap(), "launch.sh");
        assert_eq!(run_dir.metadata_file.file_name().unwrap(), "metadata.json");
        assert_eq!(run_dir.stdout_file.file_name().unwrap(), "stdout.jsonl");
    }

    #[cfg(unix)]
    #[test]
    fn test_run_directory_is_private() {
        let run_dir = create().unwrap();
        let mode = fs::metadata(&run_dir.path).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o700);
    }
}
