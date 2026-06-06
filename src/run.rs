use crate::config;
use crate::launcher;
use crate::prompt;
use crate::run_dir;
use crate::tmux;
use crate::{ConfigArgs, RunArgs};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

pub fn run(args: RunArgs) -> Result<()> {
    let (config, config_path) = config::load_config(args.config.as_deref())?;
    let (profile_name, profile) = config.resolve_profile(args.profile.as_deref())?;
    let prompt = prompt::load_prompt(&args.prompt)?;

    let run_dir = run_dir::create()?;
    let augmented_prompt = prompt::augment_prompt(&prompt, &run_dir.done_file);
    fs::write(&run_dir.prompt_file, &augmented_prompt).context("could not write prompt file")?;
    launcher::write_launcher(profile, &run_dir.prompt_file, &run_dir.launcher_file)?;

    let cwd = std::env::current_dir().context("could not read current directory")?;
    let pane_id = tmux::split_window(&run_dir.launcher_file, &cwd)?;

    eprintln!("profile: {profile_name}");
    eprintln!("config: {}", config_path.display());
    eprintln!("pane: {pane_id}");
    eprintln!("run dir: {}", run_dir.path.display());
    eprintln!("waiting for: {}", run_dir.done_file.display());

    wait_for_done(&run_dir.done_file, &pane_id)?;

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
