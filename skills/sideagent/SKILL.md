# sideagent

Use `sideagent` when implementation work can be delegated to a configured
coding agent running in tmux or headless mode.

## Delegate work

Omit `--profile` by default. That uses the default profile from the config.
If the user asks for a specific profile, run `sideagent profiles` to list
available profiles, then pass `--profile <name>`.

For short prompts:

```sh
sideagent "implement the requested change"
```

For long prompts or markdown plans:

```sh
cat path/to/plan.md | sideagent
```

The command blocks until the delegated run completes. Tmux profiles signal
completion with a done file; headless profiles use the configured CLI's
machine-readable output or process exit. Headless known-interface output is a
compact transcript tail with a path to the full raw log. When it returns,
inspect the working tree and verify the result before reporting success.

## Prompt guidance

Include:

- The goal
- Exact files or plan path when known
- Constraints from `CLAUDE.md`
- Expected checks
- A request for a short final summary

Do not ask the delegated agent to commit unless the user explicitly asked for
that behavior.
