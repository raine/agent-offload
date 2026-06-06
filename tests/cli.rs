use std::path::Path;
use std::process::Command;

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
