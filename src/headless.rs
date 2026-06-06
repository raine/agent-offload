use crate::config::{AgentInterface, Profile, PromptDelivery};
use crate::run_dir;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

pub fn run_headless(profile: &Profile, prompt: &str) -> Result<i32> {
    let mut cmd = Command::new(&profile.command);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    for (key, value) in &profile.env {
        match value {
            crate::config::EnvValue::Literal(value) => {
                cmd.env(key, value);
            }
            crate::config::EnvValue::FromEnv(from_env) => {
                let resolved = std::env::var(&from_env.from_env).with_context(|| {
                    format!("{} is not set in the environment", from_env.from_env)
                })?;
                cmd.env(key, resolved);
            }
        }
    }

    let mut args: Vec<String> = interface_headless_flags(profile.interface)
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    args.extend(profile.args.iter().cloned());

    if matches!(profile.prompt, PromptDelivery::PromptFileArg) {
        let run_dir = run_dir::create()?;
        fs::write(&run_dir.prompt_file, prompt).context("could not write prompt file")?;
        let prompt_file = run_dir.prompt_file.to_string_lossy().to_string();
        for arg in args.iter_mut() {
            if arg.contains("{prompt_file}") {
                *arg = arg.replace("{prompt_file}", &prompt_file);
            }
        }
    }

    cmd.args(&args);

    let status = match profile.prompt {
        PromptDelivery::Stdin => {
            let mut child = cmd
                .stdin(Stdio::piped())
                .spawn()
                .context("could not spawn headless agent")?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .context("could not write prompt to agent stdin")?;
            }

            child.wait().context("could not wait for headless agent")?
        }
        PromptDelivery::Argument => cmd
            .stdin(Stdio::null())
            .arg(prompt)
            .spawn()
            .context("could not spawn headless agent")?
            .wait()
            .context("could not wait for headless agent")?,
        PromptDelivery::PromptFileArg => cmd
            .stdin(Stdio::null())
            .spawn()
            .context("could not spawn headless agent")?
            .wait()
            .context("could not wait for headless agent")?,
    };

    Ok(status.code().unwrap_or(1))
}

fn interface_headless_flags(interface: AgentInterface) -> &'static [&'static str] {
    match interface {
        AgentInterface::Claude => &["-p"],
        AgentInterface::Codex => &["exec"],
        AgentInterface::Opencode => &["run"],
        AgentInterface::Generic => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_headless_flags() {
        assert_eq!(interface_headless_flags(AgentInterface::Claude), &["-p"]);
    }

    #[test]
    fn test_codex_headless_flags() {
        assert_eq!(interface_headless_flags(AgentInterface::Codex), &["exec"]);
    }

    #[test]
    fn test_opencode_headless_flags() {
        assert_eq!(interface_headless_flags(AgentInterface::Opencode), &["run"]);
    }

    #[test]
    fn test_generic_headless_flags() {
        assert!(interface_headless_flags(AgentInterface::Generic).is_empty());
    }
}
