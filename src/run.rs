use crate::config::{self, AgentInterface};
use crate::headless;
use crate::launcher;
use crate::prompt;
use crate::run_dir;
use crate::tmux;
use crate::{ConfigArgs, RunArgs};
use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

pub fn run(args: RunArgs) -> Result<()> {
    let (config, config_path) = config::load_config(args.config.as_deref())?;
    let (profile_name, profile) = config.resolve_profile(args.profile.as_deref())?;
    let prompt = prompt::load_prompt(&args.prompt)?;
    let headless_mode = args.headless || config.headless || profile.headless;

    if headless_mode {
        eprintln!("profile: {profile_name} (headless)");
        eprintln!("config: {}", config_path.display());
        let exit_code = headless::run_headless(profile, &prompt)?;
        std::process::exit(exit_code);
    }

    let run_dir = run_dir::create()?;
    let augmented_prompt = prompt::augment_prompt(&prompt, &run_dir.done_file);
    fs::write(&run_dir.prompt_file, &augmented_prompt).context("could not write prompt file")?;
    launcher::write_launcher(profile, &run_dir.prompt_file, &run_dir.launcher_file)?;

    let cwd = std::env::current_dir().context("could not read current directory")?;
    if matches!(profile.interface, AgentInterface::Cursor) {
        trust_cursor_workspace(&cwd)?;
    }
    let pane_id = tmux::split_window(&run_dir.launcher_file, &cwd)?;

    eprintln!("profile: {profile_name}");
    eprintln!("config: {}", config_path.display());
    eprintln!("pane: {pane_id}");
    eprintln!("run dir: {}", run_dir.path.display());
    eprintln!("waiting for: {}", run_dir.done_file.display());

    wait_for_done(&run_dir.done_file, &pane_id)?;

    // Kill the delegated agent's pane -- it stays running after writing done.md
    let _ = tmux::kill_pane(&pane_id);

    let summary = fs::read_to_string(&run_dir.done_file).unwrap_or_default();
    if summary.trim().is_empty() {
        println!("done: {}", run_dir.done_file.display());
    } else {
        println!("{}", summary.trim());
    }

    Ok(())
}

pub fn profiles(args: ConfigArgs) -> Result<()> {
    let (config, config_path) = config::load_config(args.config.as_deref())?;
    println!("config: {}", config_path.display());

    for name in config.profiles.keys() {
        let marker = if name == &config.default_profile {
            " default"
        } else {
            ""
        };
        println!("{name}{marker}");
    }

    Ok(())
}

fn wait_for_done(done_file: &Path, pane_id: &str) -> Result<()> {
    loop {
        if done_file.exists() {
            return Ok(());
        }

        if !tmux::pane_exists(pane_id)? {
            bail!(
                "tmux pane {pane_id} closed before writing {}",
                done_file.display()
            );
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn trust_cursor_workspace(workspace: &Path) -> Result<()> {
    let data_dir = cursor_data_dir()?;
    let marker = cursor_trust_marker_path(&data_dir, workspace);

    let parent = marker
        .parent()
        .with_context(|| format!("could not resolve parent for {}", marker.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;

    let content = serde_json::json!({
        "trustedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        "workspacePath": workspace.display().to_string(),
        "trustMethod": "cli-flag",
    });
    let data =
        serde_json::to_vec_pretty(&content).context("could not encode cursor trust marker")?;
    fs::write(&marker, data).with_context(|| format!("could not write {}", marker.display()))?;

    Ok(())
}

fn cursor_data_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CURSOR_DATA_DIR")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }

    Ok(dirs::home_dir()
        .context("could not find home directory")?
        .join(".cursor"))
}

fn cursor_trust_marker_path(data_dir: &Path, workspace: &Path) -> PathBuf {
    data_dir
        .join("projects")
        .join(slug_workspace_path(workspace))
        .join(".workspace-trusted")
}

fn slug_workspace_path(path: &Path) -> String {
    path.display()
        .to_string()
        .trim_start_matches(['/', '\\'])
        .chars()
        .map(|c| match c {
            '/' | '\\' => '-',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_cursor_trust_marker_path_uses_cursor_project_slug() {
        let data_dir = PathBuf::from("/tmp/home/.cursor");
        let workspace = PathBuf::from("/Users/raine/code/sideagent__worktrees/project-configs");

        assert_eq!(
            cursor_trust_marker_path(&data_dir, &workspace),
            PathBuf::from(
                "/tmp/home/.cursor/projects/Users-raine-code-sideagent__worktrees-project-configs/.workspace-trusted"
            )
        );
    }
}
