use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

const MIN_RIGHT_SPLIT_WIDTH: u16 = 160;

pub fn split_window(launcher_file: &Path, cwd: &Path) -> Result<String> {
    let active = active_pane()?;
    let split_flag = if active.width >= MIN_RIGHT_SPLIT_WIDTH {
        "-h"
    } else {
        "-v"
    };

    let output = Command::new("tmux")
        .arg("split-window")
        .arg(split_flag)
        .arg("-t")
        .arg(&active.id)
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
    let status = Command::new("tmux")
        .arg("display-message")
        .arg("-t")
        .arg(pane_id)
        .arg("-p")
        .arg("#{pane_id}")
        .status()
        .context("could not check tmux pane")?;

    Ok(status.success())
}

struct ActivePane {
    id: String,
    width: u16,
}

fn active_pane() -> Result<ActivePane> {
    let output = Command::new("tmux")
        .arg("display-message")
        .arg("-p")
        .arg("#{pane_id} #{pane_width}")
        .output()
        .context("could not read active tmux pane")?;

    if !output.status.success() {
        return tmux_error("display-message", output.stderr);
    }

    let output = String::from_utf8_lossy(&output.stdout);
    let mut parts = output.split_whitespace();
    let id = parts
        .next()
        .context("tmux did not return pane id")?
        .to_string();
    let width = parts
        .next()
        .context("tmux did not return pane width")?
        .parse()
        .context("tmux returned an invalid pane width")?;

    Ok(ActivePane { id, width })
}

fn tmux_error<T>(command: &str, stderr: Vec<u8>) -> Result<T> {
    let stderr = String::from_utf8_lossy(&stderr);
    bail!("tmux {command} failed: {stderr}")
}
