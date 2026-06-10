use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn config_yaml(profile: &str) -> String {
    format!("default_profile: {profile}\nprofiles:\n  {profile}:\n    command: /bin/true\n")
}

#[test]
fn test_bare_cli_prompts_help() {
    // Running with --help should succeed and mention the app
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("--help")
        .output()
        .expect("failed to run sideagent --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sideagent"));
    assert!(stdout.contains("profiles"));
    assert!(stdout.contains("install-skill"));
    assert!(stdout.contains("prompt"));
    assert!(stdout.contains("monitor"));
}

#[test]
fn test_install_skill_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("install-skill")
        .arg("--help")
        .output()
        .expect("failed to run sideagent install-skill --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Install the bundled skill"));
}

#[test]
fn test_install_skill_provider_flag_writes_target() {
    let home = tempfile::tempdir().unwrap();
    let home_dir = home.path();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("install-skill")
        .arg("--provider")
        .arg("claude")
        .env("HOME", home_dir)
        .output()
        .expect("failed to run sideagent install-skill --provider claude");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = Path::new(home_dir).join(".claude/skills/sideagent/SKILL.md");
    assert!(expected.exists());
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        include_str!("../skills/sideagent/SKILL.md")
    );
}

#[test]
fn test_profiles_requires_config() {
    // Without a config file, profiles should fail with a clear error
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .arg("--config")
        .arg("/nonexistent/config.yaml")
        .output()
        .expect("failed to run sideagent profiles");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_run_requires_config() {
    // Without a config file, run should fail with a clear error
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("run")
        .arg("--config")
        .arg("/nonexistent/config.yaml")
        .arg("test prompt")
        .output()
        .expect("failed to run sideagent run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_profiles_discovers_nearest_project_config() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("repo");
    let nested = root.join("packages").join("one");
    let expected = root.join("packages").join(".sideagent.yaml");
    std::fs::create_dir_all(&nested).unwrap();

    std::fs::write(root.join(".sideagent.yaml"), config_yaml("root")).unwrap();
    std::fs::write(&expected, config_yaml("package")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&nested)
        .output()
        .expect("failed to run sideagent profiles");

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
    let user_dir = home.path().join(".config").join("sideagent");
    let project = home.path().join("repo");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    std::fs::write(user_dir.join("config.yaml"), config_yaml("user")).unwrap();
    std::fs::write(project.join(".sideagent.yaml"), config_yaml("project")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run sideagent profiles");

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
    std::fs::write(project.join(".sideagent.yaml"), config_yaml("project")).unwrap();
    std::fs::write(&explicit, config_yaml("explicit")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .arg("--config")
        .arg(&explicit)
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run sideagent profiles");

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
    let user_config = home.path().join(".config/sideagent/config.yaml");
    let project = home.path().join("repo");
    std::fs::create_dir_all(user_config.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&user_config, config_yaml("user")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run sideagent profiles");

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
    let user_config = home.path().join(".config/sideagent/config.yaml");
    let project = home.path().join("repo");
    std::fs::create_dir_all(user_config.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&user_config, config_yaml("user")).unwrap();
    std::fs::write(project.join(".sideagent.yaml"), "profiles: []\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("profiles")
        .env("HOME", home.path())
        .current_dir(&project)
        .output()
        .expect("failed to run sideagent profiles");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not parse config file") || stderr.contains("default_profile"));
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn headless_runs_dir(home: &Path) -> PathBuf {
    home.join(".local/state/sideagent/runs")
}

#[cfg(unix)]
#[test]
fn test_headless_known_interface_writes_metadata_and_stdout_jsonl() {
    let home = tempfile::tempdir().unwrap();
    let fake_agent = home.path().join("fake-claude.sh");
    write_executable(
        &fake_agent,
        r#"#!/bin/sh
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","num_turns":0,"duration_ms":0,"total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0}}'
"#,
    );

    let config = home.path().join("config.yaml");
    fs::write(
        &config,
        format!(
            "default_profile: fake-claude\nheadless: true\nprofiles:\n  fake-claude:\n    command: {}\n    interface: claude\n",
            fake_agent.display()
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("run")
        .arg("--config")
        .arg(&config)
        .arg("test prompt")
        .env("HOME", home.path())
        .output()
        .expect("failed to run sideagent headless");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[text]  hello"));
    assert!(stdout.contains("[turn]  ok"));
    assert!(stdout.contains("Full log:"));

    let runs_dir = headless_runs_dir(home.path());
    let run_dirs: Vec<_> = fs::read_dir(&runs_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(run_dirs.len(), 1);

    let run_dir = run_dirs[0].path();
    let metadata = fs::read_to_string(run_dir.join("metadata.json")).unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["profile"]["name"], "fake-claude");
    assert_eq!(metadata["interface"], "claude");
    assert_eq!(metadata["status"], "success");
    assert_eq!(metadata["exit_code"], 0);
    assert_eq!(metadata["completion_event_seen"], true);
    assert!(metadata["started_at"].is_string());
    assert!(metadata["completed_at"].is_string());

    assert_eq!(
        fs::read_to_string(run_dir.join("prompt.md")).unwrap(),
        "test prompt"
    );

    let stdout_log = fs::read_to_string(run_dir.join("stdout.jsonl")).unwrap();
    assert!(stdout_log.contains(r#""type":"assistant""#));
    assert!(stdout_log.contains(r#""type":"result""#));
}

#[cfg(unix)]
#[test]
fn test_headless_generic_prompt_file_arg_writes_prompt_without_stdout_log() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config.yaml");
    fs::write(
        &config,
        "default_profile: generic-prompt-file\nheadless: true\nprofiles:\n  generic-prompt-file:\n    command: /bin/cat\n    interface: generic\n    prompt: prompt-file-arg\n    args:\n      - '{prompt_file}'\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("run")
        .arg("--config")
        .arg(&config)
        .arg("prompt body")
        .env("HOME", home.path())
        .output()
        .expect("failed to run sideagent headless generic prompt-file-arg");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("prompt body"));
    assert!(!stdout.contains("Full log:"));

    let runs_dir = headless_runs_dir(home.path());
    let run_dirs: Vec<_> = fs::read_dir(&runs_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(run_dirs.len(), 1);

    let run_dir = run_dirs[0].path();
    assert!(run_dir.join("prompt.md").exists());
    assert!(!run_dir.join("stdout.jsonl").exists());
    assert!(!run_dir.join("metadata.json").exists());
}

#[test]
fn test_monitor_help_documents_tui_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("monitor")
        .arg("--help")
        .output()
        .expect("failed to run sideagent monitor --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Monitor headless run archives"));
    assert!(stdout.contains("--runs-root"));
    assert!(stdout.contains("--poll-interval-ms"));
}

#[test]
fn test_monitor_once_renders_runs_and_detail() {
    let dir = tempfile::tempdir().unwrap();
    let run_path = dir.path().join("run-1");
    fs::create_dir_all(&run_path).unwrap();
    fs::write(
        run_path.join("metadata.json"),
        r#"{"profile":{"name":"demo","command":"agent","args":[]},"interface":"claude","started_at":"2026-06-09T00:00:00Z","status":"running"}"#,
    )
    .unwrap();
    fs::write(
        run_path.join("stdout.jsonl"),
        b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("monitor")
        .arg("--runs-root")
        .arg(dir.path())
        .arg("--once")
        .output()
        .expect("failed to run sideagent monitor --once");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sideagent-monitor"));
    assert!(stdout.contains("Active"));
    assert!(stdout.contains("Started | Project | Profile | Interface | Mode | Stage | Dur"));
    assert!(stdout.contains("demo"));
    assert!(stdout.contains("[text]  hello"));
}

#[test]
fn test_monitor_once_empty_runs_root() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sideagent"))
        .arg("monitor")
        .arg("--runs-root")
        .arg(dir.path())
        .arg("--once")
        .output()
        .expect("failed to run sideagent monitor --once");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sideagent-monitor"));
    assert!(stdout.contains("Active"));
    assert!(stdout.contains("(none)"));
}
