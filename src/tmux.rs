use anyhow::{Context, Result, bail};
use std::env;
use std::path::Path;
use std::process::Command;

pub fn split_window(launcher_file: &Path, cwd: &Path) -> Result<String> {
    let pane_id = agent_pane()?;

    let output = Command::new("tmux")
        .arg("split-window")
        .arg("-h")
        .arg("-t")
        .arg(&pane_id)
        .arg("-c")
        .arg(cwd)
        .arg("-P")
        .arg("-F")
        .arg("#{pane_id}")
        .arg("sh")
        .arg(launcher_file)
        .output()
        .context("could not run tmux split-window")?;

    if !output.status.success() {
        return tmux_error("split-window", output.stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneStatus {
    Alive,
    Dead,
    Missing,
}

pub fn pane_status(pane_id: &str) -> Result<PaneStatus> {
    let output = Command::new("tmux")
        .arg("display-message")
        .arg("-t")
        .arg(pane_id)
        .arg("-p")
        .arg("#{pane_dead}")
        .output()
        .context("could not check tmux pane")?;

    if !output.status.success() {
        return Ok(PaneStatus::Missing);
    }

    Ok(parse_pane_status(&output.stdout))
}

fn parse_pane_status(stdout: &[u8]) -> PaneStatus {
    if String::from_utf8_lossy(stdout).trim() == "1" {
        PaneStatus::Dead
    } else {
        PaneStatus::Alive
    }
}

pub fn kill_pane(pane_id: &str) -> Result<()> {
    let output = Command::new("tmux")
        .arg("kill-pane")
        .arg("-t")
        .arg(pane_id)
        .output()
        .context("could not run tmux kill-pane")?;

    if !output.status.success() {
        return tmux_error("kill-pane", output.stderr);
    }

    Ok(())
}

fn agent_pane() -> Result<String> {
    let pane_id = env::var("TMUX_PANE").context("could not read agent tmux pane")?;
    let pane_id = pane_id.trim().to_string();
    if pane_id.is_empty() {
        bail!("TMUX_PANE is empty");
    }

    Ok(pane_id)
}

fn tmux_error<T>(command: &str, stderr: Vec<u8>) -> Result<T> {
    let stderr = String::from_utf8_lossy(&stderr);
    bail!("tmux {command} failed: {stderr}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pane_status_alive() {
        assert_eq!(parse_pane_status(b"0\n"), PaneStatus::Alive);
    }

    #[test]
    fn test_parse_pane_status_dead() {
        assert_eq!(parse_pane_status(b"1\n"), PaneStatus::Dead);
    }
}
