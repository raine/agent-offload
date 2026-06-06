use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const SKILL_ID: &str = "agent-offload";
const SKILL_CONTENT: &str = include_str!("../skills/agent-offload/SKILL.md");

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Provider {
    #[value(name = "claude")]
    Claude,
    #[value(name = "opencode")]
    Opencode,
    #[value(name = "codex")]
    Codex,
    #[value(name = "pi")]
    Pi,
}

impl Provider {
    fn all() -> &'static [Self] {
        use self::Provider::*;
        &[Claude, Opencode, Codex, Pi]
    }

    fn label(&self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Opencode => "OpenCode",
            Provider::Codex => "Codex",
            Provider::Pi => "Pi",
        }
    }

    fn parent_dir(
        &self,
        home: &Path,
        claude_config_dir: Option<&Path>,
        pi_coding_agent_dir: Option<&Path>,
    ) -> PathBuf {
        match self {
            Provider::Claude => {
                claude_config_dir.map_or_else(|| home.join(".claude"), |path| path.to_path_buf())
            }
            Provider::Opencode => home.join(".config").join("opencode"),
            Provider::Codex => home.join(".codex"),
            Provider::Pi => pi_coding_agent_dir
                .map_or_else(|| home.join(".pi").join("agent"), |path| path.to_path_buf()),
        }
    }

    fn skill_dir(
        &self,
        home: &Path,
        claude_config_dir: Option<&Path>,
        pi_coding_agent_dir: Option<&Path>,
    ) -> PathBuf {
        self.parent_dir(home, claude_config_dir, pi_coding_agent_dir)
            .join("skills")
            .join(SKILL_ID)
    }
}

pub fn run(provider: Option<Provider>) -> Result<()> {
    let home = dirs::home_dir().context("could not find home directory")?;
    let claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok().map(PathBuf::from);
    let pi_coding_agent_dir = std::env::var("PI_CODING_AGENT_DIR").ok().map(PathBuf::from);

    let targets = collect_targets(
        &home,
        provider,
        claude_config_dir.as_deref(),
        pi_coding_agent_dir.as_deref(),
    );

    if targets.is_empty() {
        return Err(no_provider_error(
            &home,
            claude_config_dir.as_deref(),
            pi_coding_agent_dir.as_deref(),
        ));
    }

    let color = use_color();

    for (provider, skill_dir) in targets {
        println!("{}:", provider.label());
        let dest = skill_dir.join("SKILL.md");

        let up_to_date = fs::read(&dest).is_ok_and(|bytes| bytes == SKILL_CONTENT.as_bytes());

        if up_to_date {
            print_line("up-to-date", &dest, color, None, &home);
            continue;
        }

        fs::create_dir_all(&skill_dir)
            .with_context(|| format!("could not create {}", skill_dir.display()))?;
        fs::write(&dest, SKILL_CONTENT.as_bytes())
            .with_context(|| format!("could not write {}", dest.display()))?;

        print_line("written", &dest, color, Some(32), &home);
    }

    Ok(())
}

fn collect_targets(
    home: &Path,
    provider: Option<Provider>,
    claude_config_dir: Option<&Path>,
    pi_coding_agent_dir: Option<&Path>,
) -> Vec<(Provider, PathBuf)> {
    match provider {
        Some(provider) => vec![(
            provider,
            provider.skill_dir(home, claude_config_dir, pi_coding_agent_dir),
        )],
        None => Provider::all()
            .iter()
            .filter_map(|provider| {
                let parent_dir = provider.parent_dir(home, claude_config_dir, pi_coding_agent_dir);
                if parent_dir.exists() {
                    Some((
                        *provider,
                        provider.skill_dir(home, claude_config_dir, pi_coding_agent_dir),
                    ))
                } else {
                    None
                }
            })
            .collect(),
    }
}

fn no_provider_error(
    home: &Path,
    claude_config_dir: Option<&Path>,
    pi_coding_agent_dir: Option<&Path>,
) -> anyhow::Error {
    let mut detail = String::from("No provider configuration directory was detected.\n");
    detail.push_str("Checked:\n");
    for provider in Provider::all() {
        let dir = provider.parent_dir(home, claude_config_dir, pi_coding_agent_dir);
        detail.push_str(&format!("  {} => {}\n", provider.label(), dir.display()));
    }
    detail.push_str("Run with --provider to install for a specific provider.");
    anyhow!(detail)
}

fn use_color() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var("NO_COLOR")
            .map(|v| v.is_empty())
            .unwrap_or(true)
}

fn print_line(status: &str, path: &Path, color: bool, ansi_color: Option<u8>, home: &Path) {
    let display = shrink_path(path, home);
    if color {
        if let Some(code) = ansi_color {
            println!("\x1b[{code}m{status:<12}\x1b[0m {display}");
        } else {
            println!("\x1b[2m{status:<12}\x1b[0m {display}");
        }
    } else {
        println!("{status:<12} {display}");
    }
}

fn shrink_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|rel| format!("~/{}", rel.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_shrink_path_under_home() {
        let home = PathBuf::from("/tmp/home");
        let path = home.join(".claude/skills/agent-offload/SKILL.md");
        assert_eq!(
            shrink_path(&path, &home),
            "~/.claude/skills/agent-offload/SKILL.md"
        );
    }

    #[test]
    fn test_shrink_path_outside_home() {
        let home = PathBuf::from("/tmp/home");
        let path = PathBuf::from("/var/tmp/SKILL.md");
        assert_eq!(shrink_path(&path, &home), "/var/tmp/SKILL.md");
    }

    #[test]
    fn test_provider_path_resolves_env_overrides() {
        let home = PathBuf::from("/tmp/home");
        let claude_override = PathBuf::from("/tmp/claude-config");
        let pi_override = PathBuf::from("/tmp/pi-agent");

        assert_eq!(
            Provider::Claude.skill_dir(&home, Some(&claude_override), Some(&pi_override),),
            PathBuf::from("/tmp/claude-config/skills/agent-offload"),
        );
        assert_eq!(
            Provider::Pi.skill_dir(&home, Some(&claude_override), Some(&pi_override)),
            PathBuf::from("/tmp/pi-agent/skills/agent-offload"),
        );
        assert_eq!(
            Provider::Opencode.skill_dir(&home, Some(&claude_override), Some(&pi_override),),
            PathBuf::from("/tmp/home/.config/opencode/skills/agent-offload"),
        );
    }

    #[test]
    fn test_collect_targets_detects_existing_providers() {
        let home = tempdir().unwrap();
        let home = home.path();

        let claude_dir = home.join(".claude");
        let codex_dir = home.join(".codex");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::create_dir_all(&codex_dir).unwrap();

        let targets = super::collect_targets(home, None, None, None);
        assert_eq!(
            targets,
            vec![
                (
                    Provider::Claude,
                    claude_dir.join("skills").join("agent-offload")
                ),
                (
                    Provider::Codex,
                    codex_dir.join("skills").join("agent-offload")
                ),
            ]
        );
    }

    #[test]
    fn test_collect_targets_uses_provider_override() {
        let home = tempdir().unwrap();
        let home = home.path();

        let pi_override = home.join(".custom-pi");
        fs::create_dir_all(&pi_override).unwrap();
        let targets = super::collect_targets(
            home,
            Some(Provider::Pi),
            Some(&home.join(".other")),
            Some(&pi_override),
        );
        assert_eq!(
            targets,
            vec![(
                Provider::Pi,
                pi_override.join("skills").join("agent-offload")
            )]
        );
    }
}
