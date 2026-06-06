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

pub fn pane_exists(pane_id: &str) -> Result<bool> {
    let output = Command::new("tmux")
        .arg("display-message")
        .arg("-t")
        .arg(pane_id)
        .arg("-p")
        .arg("#{pane_id}")
        .output()
        .context("could not check tmux pane")?;

    Ok(output.status.success())
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
