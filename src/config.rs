use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub default_profile: String,
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub command: String,
    #[serde(default)]
    pub interface: AgentInterface,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, EnvValue>,
    #[serde(default)]
    pub prompt: PromptDelivery,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    Literal(String),
    FromEnv(FromEnvValue),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FromEnvValue {
    pub from_env: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentInterface {
    #[default]
    Generic,
    Claude,
    Codex,
    Opencode,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptDelivery {
    Stdin,
    #[default]
    Argument,
    PromptFileArg,
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not find home directory")?;
    Ok(home
        .join(".config")
        .join("agent-offload")
        .join("config.yaml"))
}

pub fn load_config(path: Option<&Path>) -> Result<(Config, PathBuf)> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => default_config_path()?,
    };

    if !path.exists() {
        bail!("config file not found at {}", path.display());
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("could not read config file {}", path.display()))?;
    let config: Config = serde_yaml::from_str(&contents)
        .with_context(|| format!("could not parse config file {}", path.display()))?;

    config.validate()?;
    Ok((config, path))
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.profiles.is_empty() {
            bail!("config must define at least one profile");
        }

        if !self.profiles.contains_key(&self.default_profile) {
            bail!("default profile {:?} is not defined", self.default_profile);
        }

        for (name, profile) in &self.profiles {
            if profile.command.trim().is_empty() {
                bail!("profile {name:?} must define a command");
            }

            for key in profile.env.keys() {
                if !is_env_key(key) {
                    bail!("profile {name:?} has invalid env key {key:?}");
                }
            }

            for value in profile.env.values() {
                if let EnvValue::FromEnv(from_env) = value
                    && !is_env_key(&from_env.from_env) {
                        bail!(
                            "profile {name:?} has invalid from_env name {:?}",
                            from_env.from_env
                        );
                    }
            }

            if matches!(profile.prompt, PromptDelivery::PromptFileArg)
                && !profile.args.iter().any(|arg| arg.contains("{prompt_file}"))
            {
                bail!("profile {name:?} uses prompt-file-arg but no arg contains {{prompt_file}}");
            }
        }

        Ok(())
    }

    pub fn resolve_profile<'a>(
        &'a self,
        requested: Option<&'a str>,
    ) -> Result<(&'a str, &'a Profile)> {
        let name = requested.unwrap_or(&self.default_profile);
        let profile = self
            .profiles
            .get(name)
            .with_context(|| format!("profile {name:?} is not defined"))?;

        Ok((name, profile))
    }
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_profile_resolution() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: claude
            "#,
        )
        .unwrap();
        assert!(config.validate().is_ok());

        let (name, _) = config.resolve_profile(None).unwrap();
        assert_eq!(name, "default");
    }

    #[test]
    fn test_requested_profile_resolution() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: claude
              fast:
                command: claude
                args: ["--fast"]
            "#,
        )
        .unwrap();

        let (name, _) = config.resolve_profile(Some("fast")).unwrap();
        assert_eq!(name, "fast");
    }

    #[test]
    fn test_invalid_env_key() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: claude
                env:
                  "123invalid": "value"
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_prompt_file_arg_requires_placeholder() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: agent
                prompt: prompt-file-arg
                args: ["--file", "/fixed/path"]
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_prompt_file_arg_with_placeholder() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: agent
                prompt: prompt-file-arg
                args: ["--file", "{prompt_file}"]
            "#,
        )
        .unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_missing_default_profile() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: missing
            profiles:
              default:
                command: claude
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_empty_profiles() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles: {}
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_from_env_value() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: claude
                env:
                  API_KEY:
                    from_env: MY_SECRET_KEY
            "#,
        )
        .unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_from_env_name() {
        let config: Config = serde_yaml::from_str(
            r#"
            default_profile: default
            profiles:
              default:
                command: claude
                env:
                  API_KEY:
                    from_env: "123invalid"
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }
}
