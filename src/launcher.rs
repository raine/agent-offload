use crate::config::{AgentInterface, Profile, PromptDelivery};
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn write_launcher(profile: &Profile, prompt_file: &Path, launcher_file: &Path) -> Result<()> {
    let mut script = String::from("#!/bin/sh\nset -eu\n\n");
    script.push_str(&format!(
        "PROMPT_FILE={}\n",
        sh_quote(&prompt_file.display().to_string())
    ));
    script.push_str("PROMPT_CONTENT=$(cat \"$PROMPT_FILE\")\n\n");

    for (key, value) in &profile.env {
        script.push_str(&format!("export {key}={}\n", sh_quote(value)));
    }

    script.push_str("\nexec ");
    script.push_str(&sh_quote(&profile.command));

    for arg in &profile.args {
        script.push(' ');
        script.push_str(&sh_quote(
            &arg.replace("{prompt_file}", &prompt_file.display().to_string()),
        ));
    }

    match profile.prompt {
        PromptDelivery::Stdin => {
            script.push_str(" < \"$PROMPT_FILE\"");
        }
        PromptDelivery::Argument => {
            for arg in interface_prompt_args(profile.interface) {
                script.push(' ');
                script.push_str(arg);
            }
        }
        PromptDelivery::PromptFileArg => {}
    }

    script.push('\n');
    fs::write(launcher_file, script).context("could not write launcher script")?;

    let mut permissions = fs::metadata(launcher_file)
        .context("could not stat launcher script")?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(launcher_file, permissions).context("could not chmod launcher script")?;

    Ok(())
}

fn interface_prompt_args(interface: AgentInterface) -> &'static [&'static str] {
    match interface {
        AgentInterface::Generic | AgentInterface::Claude | AgentInterface::Codex => {
            &["--", "\"$PROMPT_CONTENT\""]
        }
        AgentInterface::Opencode => &["--prompt", "\"$PROMPT_CONTENT\""],
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PromptDelivery;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn test_profile(command: &str, interface: AgentInterface) -> Profile {
        Profile {
            command: command.to_string(),
            interface,
            args: vec![],
            env: BTreeMap::new(),
            prompt: PromptDelivery::Argument,
        }
    }

    #[test]
    fn test_launcher_claude_interface() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let profile = test_profile("claude", AgentInterface::Claude);
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let script = fs::read_to_string(&launcher_file).unwrap();
        assert!(script.contains("PROMPT_CONTENT=$(cat \"$PROMPT_FILE\")"));
        assert!(script.contains("exec 'claude'"));
        assert!(script.contains("-- \"$PROMPT_CONTENT\""));
    }

    #[test]
    fn test_launcher_opencode_interface() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let profile = test_profile("opencode", AgentInterface::Opencode);
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let script = fs::read_to_string(&launcher_file).unwrap();
        assert!(script.contains("exec 'opencode'"));
        assert!(script.contains("--prompt \"$PROMPT_CONTENT\""));
    }

    #[test]
    fn test_launcher_env_vars() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let mut env = BTreeMap::new();
        env.insert("API_KEY".to_string(), "secret".to_string());
        let profile = Profile {
            command: "tool".to_string(),
            interface: AgentInterface::Generic,
            args: vec![],
            env,
            prompt: PromptDelivery::Argument,
        };
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let script = fs::read_to_string(&launcher_file).unwrap();
        assert!(script.contains("export API_KEY='secret'"));
    }

    #[test]
    fn test_launcher_is_executable() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let profile = test_profile("tool", AgentInterface::Generic);
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let metadata = fs::metadata(&launcher_file).unwrap();
        assert!(metadata.permissions().mode() & 0o111 != 0);
    }

    #[test]
    fn test_launcher_stdin_delivery() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let profile = Profile {
            command: "agent".to_string(),
            interface: AgentInterface::Generic,
            args: vec![],
            env: BTreeMap::new(),
            prompt: PromptDelivery::Stdin,
        };
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let script = fs::read_to_string(&launcher_file).unwrap();
        assert!(script.contains(" < \"$PROMPT_FILE\""));
    }

    #[test]
    fn test_launcher_prompt_file_delivery() {
        let dir = TempDir::new().unwrap();
        let prompt_file = dir.path().join("prompt.md");
        let launcher_file = dir.path().join("launch.sh");

        let profile = Profile {
            command: "agent".to_string(),
            interface: AgentInterface::Generic,
            args: vec!["--file".to_string(), "{prompt_file}".to_string()],
            env: BTreeMap::new(),
            prompt: PromptDelivery::PromptFileArg,
        };
        write_launcher(&profile, &prompt_file, &launcher_file).unwrap();

        let script = fs::read_to_string(&launcher_file).unwrap();
        let expected = format!("'{}'", prompt_file.display());
        assert!(script.contains(&expected));
    }

    #[test]
    fn test_sh_quote_simple() {
        assert_eq!(sh_quote("hello"), "'hello'");
    }

    #[test]
    fn test_sh_quote_with_single_quote() {
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
    }
}
