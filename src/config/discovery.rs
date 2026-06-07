use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub const PROJECT_CONFIG_FILENAME: &str = ".sideagent.yaml";

pub fn find_project_config(cwd: &Path, home: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut dir = Some(cwd);

    while let Some(current) = dir {
        let candidate = current.join(PROJECT_CONFIG_FILENAME);
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(candidate)),
            Ok(_) => bail!("project config path is not a file: {}", candidate.display()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("could not inspect {}", candidate.display()));
            }
        }

        if home.is_some_and(|home| current == home) {
            break;
        }

        dir = current.parent();
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_config_in_parent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let cwd = root.join("crates").join("app");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(root.join(PROJECT_CONFIG_FILENAME), "").unwrap();

        assert_eq!(
            find_project_config(&cwd, Some(temp.path())).unwrap(),
            Some(root.join(PROJECT_CONFIG_FILENAME))
        );
    }

    #[test]
    fn nearest_config_wins() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let package = root.join("packages").join("one");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(root.join(PROJECT_CONFIG_FILENAME), "").unwrap();
        std::fs::write(root.join("packages").join(PROJECT_CONFIG_FILENAME), "").unwrap();

        assert_eq!(
            find_project_config(&package, Some(temp.path())).unwrap(),
            Some(root.join("packages").join(PROJECT_CONFIG_FILENAME))
        );
    }

    #[test]
    fn checks_home_before_stopping() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let cwd = home.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(home.join(PROJECT_CONFIG_FILENAME), "").unwrap();

        assert_eq!(
            find_project_config(&cwd, Some(&home)).unwrap(),
            Some(home.join(PROJECT_CONFIG_FILENAME))
        );
    }

    #[test]
    fn stops_at_home_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let cwd = home.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(temp.path().join(PROJECT_CONFIG_FILENAME), "").unwrap();

        assert_eq!(find_project_config(&cwd, Some(&home)).unwrap(), None);
    }

    #[test]
    fn errors_when_candidate_is_not_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("repo");
        std::fs::create_dir_all(cwd.join(PROJECT_CONFIG_FILENAME)).unwrap();

        let err = find_project_config(&cwd, Some(temp.path())).unwrap_err();
        assert!(err.to_string().contains("not a file"));
    }
}
