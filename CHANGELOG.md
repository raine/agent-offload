# Changelog

## v0.1.9 (2026-06-13)

- Added `sideagent monitor`, a terminal UI for watching active and completed headless runs.
- Headless runs for known streaming interfaces now save run metadata, prompts, and stdout logs in a private run archive.
- The monitor now shows live transcript output, expandable prompts, run metadata, history filtering, and keyboard or mouse navigation.
- Stale active runs are now detected and moved to history as failed instead of staying active forever.

## v0.1.8 (2026-06-08)

- Headless Claude, Codex, Cursor, and OpenCode runs now use each CLI's structured output to detect when delegated work finishes.
- Headless runs for known interfaces now print a compact transcript tail and save the full JSONL log for review.

## v0.1.7 (2026-06-07)

- Delegated tmux panes now choose the neighboring pane when it better preserves the current layout.

## v0.1.6 (2026-06-07)

- Tmux runs now fail promptly if the delegated pane closes before reporting completion instead of waiting forever.

## v0.1.5 (2026-06-07)

- Project configs are now discovered from `.sideagent.yaml`, and the default user config lives under `~/.config/sideagent`.

## v0.1.4 (2026-06-06)

- Added project config discovery with `.sideagent.yaml`, so repositories can provide their own profiles without passing `--config`.
- Delegated tmux panes now open next to the pane running `sideagent` without changing the rest of the window layout.

## v0.1.3 (2026-06-06)

- Added Cursor Agent as a supported interface.
- Cursor Agent workspaces are marked trusted before launch, so delegated runs do not stop on workspace trust prompts.

## v0.1.2 (2026-06-06)

- Added provider-aware skill installation for Claude Code, OpenCode, Codex, and Pi
- Added Cursor Agent as a supported interface.

## v0.1.1 (2026-06-06)

- Added headless mode so delegated agents can run as non-interactive subprocesses without tmux.

## v0.1.0 (2026-06-06)

Initial release.
