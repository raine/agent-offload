<h1 align="center">agent-offload</h1>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands">Commands</a>
</p>

`agent-offload` launches another coding agent and blocks until that agent
completes. Use it when your main agent wants to delegate implementation work
while keeping the current conversation in control of review, verification, and
final reporting.

## Why?

When you have a thorough Markdown plan, implementing it with a heavy, slow model
is overkill. The host agent should be able to hand that work to a cheaper and
faster model directly, without user manually starting a new Codex, OpenCode, or
other agent session.

Claude Code can delegate to subagents, but that keeps delegation inside Claude's
own harness and model choices. Other agent harnesses have their own delegation
mechanisms, assumptions, and model support. For example, you might be inside
Claude Code with a plan ready to implement, but want Codex Spark or a DeepSeek V4
backed agent to do the edit. The host agent should be able to start that run,
pass the plan, wait for completion, and review the result without asking the user
to open and manage a separate session.

`agent-offload` makes offloading harness agnostic by using process execution as
the boundary. The host agent runs one blocking command, the delegated task opens
in a new tmux pane or runs headlessly, and the host agent waits until the task
completes. Then the host agent can inspect the diff, run checks, and continue
the review in the original conversation.

## What it does

- Launches configured agent profiles in tmux or headless mode
- Passes prompts using the format each agent CLI expects
- Waits for completion and returns the delegated agent's summary
- Supports per-profile arguments and environment variables
- Installs a provider-agnostic `/agent-offload` skill bundle

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

`agent-offload` selects a config in this order:

1. `--config <path>`
2. The nearest `.agent-offload.yaml` in the current directory or an ancestor up to
   your home directory
3. `~/.config/agent-offload/config.yaml`

A discovered project config replaces the user config completely. Configs are not
merged.

Project configs can contain commands and environment variables. Use `from_env`
for secrets instead of committing literal secret values.

See [Configuration](#configuration) for an example.

### 3. Install the skill

```bash
agent-offload install-skill
```

Without `--provider`, this installs to every detected provider config
directory. A provider is detected when its config directory already exists. Use
`--provider` to install for one provider even if its config directory has not
been created yet.

Default install paths are:

```text
~/.claude/skills/agent-offload/SKILL.md
~/.config/opencode/skills/agent-offload/SKILL.md
~/.codex/skills/agent-offload/SKILL.md
~/.pi/agent/skills/agent-offload/SKILL.md
```

Then use `/agent-offload` from your host agent UI, or call the CLI directly.

### 4. Delegate work

From Claude Code, invoke the installed skill:

```text
/agent-offload implement the change in history/plan.md
```

The host agent loads the skill, chooses a configured profile, runs
`agent-offload`, waits for the delegated agent's summary, then reviews and
reports the result in the current conversation.

## How it works

Tmux runs get a directory under:

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
rename it to `done.md`. The parent process waits for `done.md` to exist, then
kills the delegated pane. If the tmux pane closes first, the run fails instead
of hanging silently.

Headless runs execute the configured command in the current terminal and return
its exit status. Headless `prompt-file-arg` runs create a run directory for the
prompt file; headless `argument` and `stdin` runs do not.

The delegated pane opens to the right of the tmux pane that runs
`agent-offload`, even if another tmux client is viewing a different window.
Other panes in the window keep their existing layout scope.

## One-off benchmark

A small real-world benchmark helped choose a model for implementation work. Each
model got three isolated attempts at the same
[run archive implementation plan](https://gist.github.com/raine/2c94e39be2e491663346c759c87a05e6):
adding metadata, JSONL events, raw logs, tmux pane capture, summaries, and
`runs` / `show` commands.

| Model                   | Median time | What happened                                                                                |
| ----------------------- | ----------: | -------------------------------------------------------------------------------------------- |
| `composer-2.5-fast`     |    2m 1.16s | Fastest typical run, best typical host score, and safest default choice from this benchmark. |
| `gpt-5.3-codex-spark`   |   2m 58.32s | Produced the best single attempt, but reviewers disagreed strongly about that result.        |
| `deepseek-v4-flash[1m]` |   6m 29.23s | Completed every trial, but was slower and had the lowest typical host score.                 |

The main failures were subtle implementation issues, not obvious missing
features: parsing real streamed output, avoiding large log reads, and cleaning up
child processes or tmux panes after errors.

This is one benchmark on one implementation task, not a universal model ranking.
For this project, `composer-2.5-fast` looked like the safest default starting
point. See the
[full benchmark report](https://gist.github.com/raine/85ceb6d752baca6e03b9a7e0b4eb5e0d)
for methodology, timings, reviews, and limitations.

## Configuration

`agent-offload` selects a config in this order:

1. `--config <path>`
2. The nearest `.agent-offload.yaml` in the current directory or an ancestor up to
   your home directory
3. `~/.config/agent-offload/config.yaml`

A discovered project config replaces the user config completely. Configs are not
merged. Project discovery stops after checking your home directory.

Project configs can contain commands and environment variables. Do not commit
secrets in `.agent-offload.yaml`. Use `from_env` instead.

A config has a default profile and one or more named profiles. Top-level
`headless: true` runs every profile without tmux. This example uses Codex Spark
and a DeepSeek-backed Claude Code profile:

```yaml
default_profile: claude-deepseek-flash
headless: false

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

  cursor-composer:
    command: /Users/raine/.local/bin/cursor-agent
    interface: cursor
    args:
      - --force
      - --model
      - composer-2.5-fast
    env: {}
```

### Config fields

| Field             | Description                              | Default  |
| ----------------- | ---------------------------------------- | -------- |
| `default_profile` | Profile used when `--profile` is omitted | Required |
| `headless`        | Run every profile without tmux           | `false`  |
| `profiles`        | Named profile map                        | Required |

Headless mode is enabled when `--headless`, top-level `headless`, or
profile-level `headless` is true.

### Profile fields

| Field       | Description                                  | Default    |
| ----------- | -------------------------------------------- | ---------- |
| `command`   | Binary or script to execute                  | Required   |
| `interface` | Argument shape for the agent CLI             | `generic`  |
| `args`      | Extra arguments passed before the prompt     | `[]`       |
| `env`       | Environment variables exported before launch | `{}`       |
| `prompt`    | How the augmented prompt is delivered        | `argument` |
| `headless`  | Whether to run this profile without tmux     | `false`    |

### Interfaces

| Interface  | Prompt argument form         |
| ---------- | ---------------------------- |
| `generic`  | `-- "$PROMPT_CONTENT"`       |
| `claude`   | `-- "$PROMPT_CONTENT"`       |
| `codex`    | `-- "$PROMPT_CONTENT"`       |
| `cursor`   | `-- "$PROMPT_CONTENT"`       |
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
agent-offload run --profile claude-deepseek-flash "fix the failing tests"
cat history/plan.md | agent-offload run --profile codex-spark
```

The `run` subcommand is optional, so this is equivalent:

```bash
agent-offload --profile claude-deepseek-flash "fix the failing tests"
```

Options:

| Option                 | Description                               |
| ---------------------- | ----------------------------------------- |
| `-p, --profile <name>` | Profile name from the selected config     |
| `--config <path>`      | Use this config file instead of discovery |
| `-H, --headless`       | Run this invocation in headless mode      |

### `profiles`

List configured profiles and mark the default.

```bash
agent-offload profiles
agent-offload profiles --config ./.agent-offload.yaml
```

### `install-skill`

Install the bundled skill for one provider or all detected providers.

```bash
agent-offload install-skill
```

```bash
agent-offload install-skill --provider claude
```

Without `--provider`, the command installs to detected provider config
directories. A provider is detected when its config directory already exists. If
no provider config directories are found, the command exits with an error and
lists the checked paths.

With `--provider`, the command installs only for that provider and creates the
skill directory as needed.

The default Claude Code install root is `~/.claude`, or `CLAUDE_CONFIG_DIR` when
that variable is set. The default Pi install root is `~/.pi/agent`, or
`PI_CODING_AGENT_DIR` when that variable is set.

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
- At least one configured agent CLI
- tmux, for non-headless runs

Non-headless runs must be started from inside an existing tmux pane. Headless
runs do not require tmux.

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
