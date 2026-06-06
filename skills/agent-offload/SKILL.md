# agent-offload

Use `agent-offload` when implementation work can be delegated to a configured
coding agent running in a nearby tmux pane.

## Check profiles

Run:

```sh
agent-offload profiles
```

Use the default profile unless the user asks for a specific one or the task
clearly needs a named profile.

## Delegate work

For short prompts:

```sh
agent-offload run --profile <name> "implement the requested change"
```

For long prompts or markdown plans:

```sh
cat path/to/plan.md | agent-offload run --profile <name>
```

The command blocks until the delegated agent writes its done file. When it
returns, read the short summary printed by `agent-offload`, inspect the working
tree, and verify the result before reporting success.

## Profiles

A profile can point at a custom binary and declare the agent interface it
follows. For example, a `claude-deepseek` binary can use `interface: claude`.

## Prompt guidance

Include:

- The goal
- Exact files or plan path when known
- Constraints from `CLAUDE.md`
- Expected checks
- A request for a short final summary

Do not ask the delegated agent to commit unless the user explicitly asked for
that behavior.
