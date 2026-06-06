use std::path::Path;
use std::process::Command;

fn config_yaml(profile: &str) -> String {
    format!("default_profile: {profile}\nprofiles:\n  {profile}:\n    command: /bin/true\n")
}

#[test]
fn test_bare_cli_prompts_help() {
    // Running with --help should succeed and mention the app
    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("--help")
        .output()
        .expect("failed to run agent-offload --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("agent-offload"));
    assert!(stdout.contains("profiles"));
    assert!(stdout.contains("install-skill"));
    assert!(stdout.contains("prompt"));
}

#[test]
fn test_install_skill_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("install-skill")
        .arg("--help")
        .output()
        .expect("failed to run agent-offload install-skill --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Install the bundled skill"));
}

#[test]
fn test_install_skill_provider_flag_writes_target() {
    let home = tempfile::tempdir().unwrap();
    let home_dir = home.path();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("install-skill")
        .arg("--provider")
        .arg("claude")
        .env("HOME", home_dir)
        .output()
        .expect("failed to run agent-offload install-skill --provider claude");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = Path::new(home_dir).join(".claude/skills/agent-offload/SKILL.md");
    assert!(expected.exists());
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        include_str!("../skills/agent-offload/SKILL.md")
    );
}

#[test]
fn test_profiles_requires_config() {
    // Without a config file, profiles should fail with a clear error
    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .arg("--config")
        .arg("/nonexistent/config.yaml")
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_run_requires_config() {
    // Without a config file, run should fail with a clear error
    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("run")
        .arg("--config")
        .arg("/nonexistent/config.yaml")
        .arg("test prompt")
        .output()
        .expect("failed to run agent-offload run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_profiles_discovers_nearest_project_config() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("repo");
    let nested = root.join("packages").join("one");
    let expected = root.join("packages").join(".agent-offload.yaml");
    std::fs::create_dir_all(&nested).unwrap();

    std::fs::write(root.join(".agent-offload.yaml"), config_yaml("root")).unwrap();
    std::fs::write(&expected, config_yaml("package")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&nested)
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_path = expected.canonicalize().unwrap_or(expected);
    assert!(stdout.contains(&format!("config: {}", expected_path.display())));
    assert!(stdout.contains("package default"));
    assert!(!stdout.contains("root default"));
}

#[test]
fn test_project_config_replaces_user_config_completely() {
    let home = tempfile::tempdir().unwrap();
    let user_dir = home.path().join(".config").join("agent-offload");
    let project = home.path().join("repo");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    std::fs::write(user_dir.join("config.yaml"), config_yaml("user")).unwrap();
    std::fs::write(project.join(".agent-offload.yaml"), config_yaml("project")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("project default"));
    assert!(!stdout.contains("user default"));
}

#[test]
fn test_explicit_config_overrides_project_discovery() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("repo");
    let explicit = home.path().join("explicit.yaml");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(".agent-offload.yaml"), config_yaml("project")).unwrap();
    std::fs::write(&explicit, config_yaml("explicit")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .arg("--config")
        .arg(&explicit)
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("explicit default"));
    assert!(!stdout.contains("project default"));
}

#[test]
fn test_profiles_falls_back_to_user_config() {
    let home = tempfile::tempdir().unwrap();
    let user_config = home.path().join(".config/agent-offload/config.yaml");
    let project = home.path().join("repo");
    std::fs::create_dir_all(user_config.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&user_config, config_yaml("user")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("user default"));
}

#[test]
fn test_invalid_project_config_does_not_fallback_to_user_config() {
    let home = tempfile::tempdir().unwrap();
    let user_config = home.path().join(".config/agent-offload/config.yaml");
    let project = home.path().join("repo");
    std::fs::create_dir_all(user_config.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&user_config, config_yaml("user")).unwrap();
    std::fs::write(project.join(".agent-offload.yaml"), "profiles: []\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_agent-offload"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run agent-offload profiles");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not parse config file") || stderr.contains("default_profile"));
}
