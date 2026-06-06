<h1 align="center">agent-offload</h1>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands">Commands</a>
</p>

`agent-offload` launches another coding agent and blocks until that agent
finishes. Use it when your main agent wants to delegate implementation work
while keeping the current conversation in control of review, verification, and
final reporting.

## Why?

Sometimes you already have a thorough Markdown plan, and implementing it with a
heavy, slow model is overkill. The host agent should be able to hand that work
to a cheaper and faster model directly, without user manually starting a new
Codex, OpenCode, or other agent session.

Claude Code can delegate to subagents, but that keeps delegation inside Claude's
own harness and model choices. If you want to use another model, another
provider, or another agent CLI, there is no smooth handoff.

`agent-offload` bridges that gap with tmux or headless subprocess execution.
The host agent runs one blocking command, the delegated task opens in a new tmux
pane or runs headlessly, and the host agent waits until the task finishes. Then
the host agent can inspect the diff, run checks, and continue the review in the
original conversation.

## What it does

- Launches configured agent profiles in tmux or in headless mode
- Sends prompts as an argument, stdin, or a prompt-file argument
- Adds completion-file instructions to tmux delegated prompts
- Waits until the delegated agent exits or publishes `done.md`
- Detects if the tmux pane exits before completion
- Kills the delegated pane after completion
- Supports per-profile environment variables, including forwarding from the host
  environment
- Installs its Claude Code skill with `agent-offload install-skill`

## Quick start

### 1. Install

```bash
# Shell script, macOS/Linux
curl -fsSL https://raw.githubusercontent.com/raine/agent-offload/main/scripts/install.sh | bash

# Homebrew
brew install raine/agent-offload/agent-offload

# Cargo from source
cargo install --git https://github.com/raine/agent-offload --locked
```

### 2. Create a config

`agent-offload` reads:

```text
~/.config/agent-offload/config.yaml
```

See [Configuration](#configuration) for an example.

### 3. Install the skill

```bash
agent-offload install-skill
```

This writes:

```text
~/.claude/skills/agent-offload/SKILL.md
```

Then use `/agent-offload` from Claude Code, or call the CLI directly.

### 4. Delegate work

From Claude Code, invoke the installed skill:

```text
/agent-offload implement the change in history/plan.md
```

The host agent loads the skill, chooses a configured profile, runs
`agent-offload`, waits for the delegated agent's summary, then reviews and
reports the result in the current conversation.

## How it works

Each run gets a directory under:

```text
~/.local/state/agent-offload/runs/
```

The run directory contains:

| File        | Purpose                                               |
| ----------- | ----------------------------------------------------- |
| `prompt.md` | The augmented prompt sent to the delegated agent      |
| `launch.sh` | The generated executable launcher script              |
| `done.md`   | The completion summary written by the delegated agent |

In tmux mode, `agent-offload` appends instructions to the prompt telling the
delegated agent to write a concise summary to `done.md.tmp`, then atomically
rename it to `done.md`. The parent process waits for `done.md` to exist. If the
tmux pane closes first, the run fails instead of hanging silently.

## Configuration

A config has a default profile and one or more named profiles. This example
uses Codex Spark and a DeepSeek-backed Claude Code profile:

```yaml
default_profile: claude-deepseek-flash

profiles:
  codex-spark:
    command: /opt/homebrew/bin/codex
    interface: codex
    args:
      - --yolo
      - --model
      - gpt-5.3-codex-spark
      - -c
      - model_reasoning_effort="high"
    env: {}
    prompt: argument

  claude-deepseek-flash:
    command: /Users/raine/.local/bin/claude
    interface: claude
    args:
      - --dangerously-skip-permissions
    env:
      ANTHROPIC_BASE_URL: https://api.deepseek.com/anthropic
      ANTHROPIC_AUTH_TOKEN:
        from_env: DEEPSEEK_API_KEY
      ANTHROPIC_MODEL: 'deepseek-v4-flash[1m]'
      ANTHROPIC_SMALL_FAST_MODEL: 'deepseek-v4-flash[1m]'
      ANTHROPIC_DEFAULT_OPUS_MODEL: deepseek-v4-flash
      ANTHROPIC_DEFAULT_SONNET_MODEL: deepseek-v4-flash
      ANTHROPIC_DEFAULT_HAIKU_MODEL: deepseek-v4-flash
      CLAUDE_CODE_SUBAGENT_MODEL: deepseek-v4-pro
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1'
      CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK: '1'
      CLAUDE_CODE_EFFORT_LEVEL: max
      CLAUDE_CODE_ENABLE_TELEMETRY: '0'
    prompt: argument
```

### Profile fields

| Field       | Description                                  |
| ----------- | -------------------------------------------- |
| `command`   | Binary or script to execute                  |
| `interface` | Argument shape for the agent CLI             |
| `args`      | Extra arguments passed before the prompt     |
| `env`       | Environment variables exported before launch |
| `prompt`    | How the augmented prompt is delivered        |
| `headless`  | Whether to run this profile without tmux      |

### Interfaces

| Interface  | Prompt argument form         |
| ---------- | ---------------------------- |
| `generic`  | `-- "$PROMPT_CONTENT"`       |
| `claude`   | `-- "$PROMPT_CONTENT"`       |
| `codex`    | `-- "$PROMPT_CONTENT"`       |
| `opencode` | `--prompt "$PROMPT_CONTENT"` |

### Prompt delivery

| Value             | Behavior                                                     |
| ----------------- | ------------------------------------------------------------ |
| `argument`        | Pass the prompt using the selected interface's argument form |
| `stdin`           | Redirect the prompt file to stdin                            |
| `prompt-file-arg` | Replace `{prompt_file}` in `args` with the prompt file path  |

Use `prompt-file-arg` only when at least one arg contains `{prompt_file}`.

### Environment variables

Literal values are written directly:

```yaml
env:
  CLAUDE_CODE_ENABLE_TELEMETRY: '0'
```

Values can also be forwarded from the host environment:

```yaml
env:
  API_KEY:
    from_env: MY_SECRET_KEY
```

Environment variable names must be valid shell identifiers.

## Commands

### `run`

Launch a profile with a prompt and wait for completion.

```bash
agent-offload run --profile claude "fix the failing tests"
cat history/plan.md | agent-offload run --profile codex-spark
```

The `run` subcommand is optional, so this is equivalent:

```bash
agent-offload --profile claude "fix the failing tests"
```

Options:

| Option                 | Description                            |
| ---------------------- | -------------------------------------- |
| `-p, --profile <name>` | Profile name from the config           |
| `--config <path>`      | Override the config file path          |
| `-H, --headless`       | Run this invocation in headless mode   |

### `profiles`

List configured profiles and mark the default.

```bash
agent-offload profiles
agent-offload profiles --config ./agent-offload.yaml
```

### `install-skill`

Install the bundled Claude Code skill.

```bash
agent-offload install-skill
```

The command is idempotent. It prints `up-to-date` when the installed skill
matches the bundled copy.

## Agent workflow

A typical host-agent workflow looks like this:

```text
> /agent-offload implement the plan in history/2026-06-06-plan-feature.md with the spark profile

The host agent:
1. Loads the `agent-offload` skill
2. Pipes the plan into `agent-offload run --profile codex-spark`
3. Waits for the delegated agent's completion summary
4. Inspects the diff and runs the requested checks
5. Reports the reviewed result
```

The delegated agent should do the implementation. The host agent should still
inspect the diff, verify behavior, and decide what to report or commit.

## Requirements

- Rust, for building from source
- tmux
- At least one configured agent CLI

## Development

```bash
git clone https://github.com/raine/agent-offload.git
cd agent-offload
just check
```

`just check` runs formatting, clippy fixes, build, tests, and clippy.

Useful commands:

```bash
just run --help
just run profiles --config history/2026-06-06-sample-config.yaml
```

## Related projects

- [consult-llm](https://github.com/raine/consult-llm) - ask other LLMs for planning, review, and debugging help
- [git-surgeon](https://github.com/raine/git-surgeon) - non-interactive hunk staging and commit surgery for agents
- [workmux](https://github.com/raine/workmux) - git worktrees and tmux windows for parallel agent workflows
