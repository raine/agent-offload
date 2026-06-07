# Agent Profile Benchmark

## Summary

`composer-2.5-fast` remains the winner by the host rubric after adding `gpt-5.4-mini`. It had the best median host score, the fastest median time, and the most consistent host-reviewed results. `gpt-5.4-mini` passed `just check` in all three trials, but the host acceptance script recorded critical failures in every trial and the model was the slowest result in this report. Its consult scores were also highly polarized, with DeepSeek much more favorable than `gpt-5.5` and Gemini.

| Model                   | Median elapsed time | Success | Median host score | Consult range | Notes                                              |
| ----------------------- | ------------------: | ------: | ----------------: | ------------: | -------------------------------------------------- |
| `composer-2.5-fast`     |            2m 1.16s |     3/3 |                88 |         68-90 | Best median quality, fastest, most consistent      |
| `gpt-5.3-codex-spark`   |           2m 58.32s |     3/3 |                82 |         64-95 | Strong peak result, higher variance                |
| `deepseek-v4-flash[1m]` |           6m 29.23s |     3/3 |                76 |         61-88 | Reliable completion, slower, lower median quality  |
| `gpt-5.4-mini`          |           7m 49.72s |     0/3 |                62 |         42-96 | Passed checks, failed acceptance, widest spread    |

## Setup

- Base commit: `5e7f57fdda7fd00a8f458e65d578fff939245870`
- Config: `/Users/raine/.config/sideagent/config.yaml`
- Task: implement `/Users/raine/code/sideagent/history/2026-06-06-plan-run-archive-events.md`
- Date: 2026-06-07
- Method: manual host-run benchmark with fresh temporary clones and detached per-trial runners
- Benchmarked models:
  - `gpt-5.3-codex-spark`, profile `codex-spark`
  - `deepseek-v4-flash[1m]`, profile `claude-deepseek-flash`
  - `composer-2.5-fast`, profile `cursor-composer-fast`
  - `gpt-5.4-mini`, profile `codex-mini`
- Trials per model: 3
- Isolation: unique clone, `TMPDIR`, and `CARGO_TARGET_DIR` per trial. Codex and Claude trials also used unique `HOME`; Cursor trials used the real user `HOME` for authentication.
- Warmup: `cargo fetch` and `cargo build --all` before each timed agent run
- `consult-llm` review models: `gemini-3.1-pro-preview` and `gpt-5.5`; the `gpt-5.4-mini` addendum also used `deepseek-v4-pro`

## Task

Implement run archives for `sideagent`: metadata, JSONL event capture, raw stdout and stderr logs, tmux pane capture, headless summaries, JSON stream parsing for Cursor and Codex, and `runs` / `show` commands with tests and README coverage.

## Trial Results

| Model                   | Profile               | Trial |      Time | just check | Acceptance | Host score | Consult range | Branch                                |
| ----------------------- | --------------------- | ----: | --------: | ---------- | ---------- | ---------: | ------------: | ------------------------------------- |
| `gpt-5.3-codex-spark`   | codex-spark           |     1 | 2m 47.88s | pass       | pass       |         74 |         72-74 | `bench/codex-spark-trial-1`           |
| `gpt-5.3-codex-spark`   | codex-spark           |     2 | 2m 58.32s | pass       | pass       |         82 |         70-80 | `bench/codex-spark-trial-2`           |
| `gpt-5.3-codex-spark`   | codex-spark           |     3 | 3m 16.14s | pass       | pass       |         95 |         64-95 | `bench/codex-spark-trial-3`           |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     1 | 6m 29.23s | pass       | pass       |         76 |         76-78 | `bench/claude-deepseek-flash-trial-1` |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     2 | 5m 18.78s | pass       | pass       |         75 |         63-75 | `bench/claude-deepseek-flash-trial-2` |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     3 | 6m 45.24s | pass       | pass       |         88 |         61-88 | `bench/claude-deepseek-flash-trial-3` |
| `composer-2.5-fast`     | cursor-composer-fast  |     1 | 1m 28.23s | pass       | pass       |         78 |         76-78 | `bench/cursor-composer-fast-trial-1`  |
| `composer-2.5-fast`     | cursor-composer-fast  |     2 |  2m 1.16s | pass       | pass       |         90 |         72-90 | `bench/cursor-composer-fast-trial-2`  |
| `composer-2.5-fast`     | cursor-composer-fast  |     3 |  2m 5.61s | pass       | pass       |         88 |         68-88 | `bench/cursor-composer-fast-trial-3`  |
| `gpt-5.4-mini`          | codex-mini            |     1 | 7m 49.72s | pass       | fail       |         62 |         42-60 | `bench/codex-mini-trial-1`            |
| `gpt-5.4-mini`          | codex-mini            |     2 | 8m 44.44s | pass       | fail       |         78 |         58-96 | `bench/codex-mini-trial-2`            |
| `gpt-5.4-mini`          | codex-mini            |     3 | 7m 21.64s | pass       | fail       |         45 |         58-92 | `bench/codex-mini-trial-3`            |

## Consult Scores

| Model                   | Profile               | Trial | Host score | `gemini-3.1-pro-preview` | `gpt-5.5` | `deepseek-v4-pro` |
| ----------------------- | --------------------- | ----: | ---------: | -----------------------: | --------: | ----------------: |
| `gpt-5.3-codex-spark`   | codex-spark           |     1 |         74 |                       74 |        72 |               n/a |
| `gpt-5.3-codex-spark`   | codex-spark           |     2 |         82 |                       80 |        70 |               n/a |
| `gpt-5.3-codex-spark`   | codex-spark           |     3 |         95 |                       95 |        64 |               n/a |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     1 |         76 |                       76 |        78 |               n/a |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     2 |         75 |                       75 |        63 |               n/a |
| `deepseek-v4-flash[1m]` | claude-deepseek-flash |     3 |         88 |                       88 |        61 |               n/a |
| `composer-2.5-fast`     | cursor-composer-fast  |     1 |         78 |                       78 |        76 |               n/a |
| `composer-2.5-fast`     | cursor-composer-fast  |     2 |         90 |                       90 |        72 |               n/a |
| `composer-2.5-fast`     | cursor-composer-fast  |     3 |         88 |                       88 |        68 |               n/a |
| `gpt-5.4-mini`          | codex-mini            |     1 |         62 |                       60 |        42 |                52 |
| `gpt-5.4-mini`          | codex-mini            |     2 |         78 |                       80 |        58 |                92 |
| `gpt-5.4-mini`          | codex-mini            |     3 |         45 |                       62 |        58 |                96 |

## Results

### `gpt-5.3-codex-spark`

`gpt-5.3-codex-spark` was solid by host review and improved across trials, but the consult reviewers disagreed sharply about trial 3. Gemini rated trial 3 as the strongest single result in the benchmark, while `gpt-5.5` rated it lowest in its trial group because of bounded-read, parser fidelity, documentation, and lifecycle concerns. That makes this model the clearest high-variance result: strong peak host score, but less consult consensus than the headline score implies.

- Profile: `codex-spark`
- Median time: 2m 58.32s
- Success count: 3/3
- Median host score: 82
- Branches:
  - `bench/codex-spark-trial-1`
  - `bench/codex-spark-trial-2`
  - `bench/codex-spark-trial-3`
- Diffs:
  - `/tmp/sideagent-bench-codex-spark-trial-1.diff`
  - `/tmp/sideagent-bench-codex-spark-trial-2.diff`
  - `/tmp/sideagent-bench-codex-spark-trial-3.diff`
- Check outputs:
  - `/tmp/sideagent-bench-codex-spark-trial-1-check.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-2-check.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-3-check.txt`
- Acceptance outputs:
  - `/tmp/sideagent-bench-codex-spark-trial-1-acceptance.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-2-acceptance.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-3-acceptance.txt`
- Timing outputs:
  - `/tmp/sideagent-bench-codex-spark-trial-1-time.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-2-time.txt`
  - `/tmp/sideagent-bench-codex-spark-trial-3-time.txt`
- `consult-llm` outputs:
  - `/tmp/sideagent-bench-trial-1-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult-gpt-rerun.txt`
  - `/tmp/sideagent-bench-trial-3-consult.txt`
  - `/tmp/sideagent-bench-trial-3-consult-gpt-rerun.txt`
- Strengths: good trial 3 robustness, complete core feature coverage, passing checks and acceptance in all trials
- Issues: consult reviewers found recurring whole-file reads in default `show`, parser fidelity gaps, incomplete lifecycle cleanup, and uneven test or documentation coverage
- Safe and maintainable diff: useful architecture overall, but the `gpt-5.5` rerun did not consider trial 3 merge-ready without cleanup, parser, and bounded-read fixes

Per-trial diff stats:

```text
trial 1:
Cargo.lock      | 253 +++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  63 ++++++++++++-
src/config.rs   |   4 +-
src/headless.rs | 288 +++++++++++++++++++++++++++++++++++++++++++++++---------
src/main.rs     |  36 +++++++
src/run.rs      | 141 +++++++++++++++++++++++++--
src/run_dir.rs  | 122 +++++++++++++++++++-----
src/tmux.rs     |  22 +++++
tests/cli.rs    | 144 ++++++++++++++++++++++++++++
10 files changed, 996 insertions(+), 78 deletions(-)

trial 2:
Cargo.lock      | 253 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  47 +++++++++--
src/config.rs   |   4 +-
src/headless.rs | 237 ++++++++++++++++++++++++++++++++++++++++++----------
src/main.rs     |  31 +++++++
src/run.rs      | 138 ++++++++++++++++++++++++++++---
src/run_dir.rs  | 110 ++++++++++++++++++++----
src/tmux.rs     |  21 +++++
tests/cli.rs    | 224 +++++++++++++++++++++++++++++++++++++++++++++++++
10 files changed, 987 insertions(+), 79 deletions(-)

trial 3:
Cargo.lock      | 253 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  42 ++++++++--
src/config.rs   |   4 +-
src/headless.rs | 218 +++++++++++++++++++++++++++++++++++++-----------
src/main.rs     |  28 +++++++
src/run.rs      | 128 +++++++++++++++++++++++++++-
src/run_dir.rs  | 122 ++++++++++++++++++++++-----
src/tmux.rs     |  21 +++++
tests/cli.rs    | 217 ++++++++++++++++++++++++++++++++++++++++++++++++
10 files changed, 957 insertions(+), 77 deletions(-)
```

`consult-llm` analysis:

- `gemini-3.1-pro-preview`: scored trials 74, 80, and 95. It penalized trial 1 for whole-file `show` reads, process cleanup risks, and missing fake headless artifact tests. It rated trial 3 strongest because it handled cleanup and immediate text flushing better.
- `gpt-5.5`: scored trials 72, 70, and 64. It was much more skeptical of trial 3, citing weak Cursor and Codex parser fidelity, whole-file event reads, delayed text flushing, unsafe headless cleanup, and weaker README warnings. This disagreement is why the model has the widest consult range.

### `deepseek-v4-flash[1m]`

`deepseek-v4-flash[1m]` completed all trials and passed verification, but it was the slowest model and had the lowest median host score. The full consult set was also less favorable than the original Gemini-only view: `gpt-5.5` rated trial 2 and trial 3 substantially lower because it saw parser fidelity gaps, missing runtime acceptance evidence, and unsafe process or tmux cleanup. The model was reliable at finishing, but not the strongest final-quality option.

- Profile: `claude-deepseek-flash`
- Median time: 6m 29.23s
- Success count: 3/3
- Median host score: 76
- Branches:
  - `bench/claude-deepseek-flash-trial-1`
  - `bench/claude-deepseek-flash-trial-2`
  - `bench/claude-deepseek-flash-trial-3`
- Diffs:
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-1.diff`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-2.diff`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-3.diff`
- Check outputs:
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-1-check.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-2-check.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-3-check.txt`
- Acceptance outputs:
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-1-acceptance.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-2-acceptance.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-3-acceptance.txt`
- Timing outputs:
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-1-time.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-2-time.txt`
  - `/tmp/sideagent-bench-claude-deepseek-flash-trial-3-time.txt`
- `consult-llm` outputs:
  - `/tmp/sideagent-bench-trial-1-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult-gpt-rerun.txt`
  - `/tmp/sideagent-bench-trial-3-consult.txt`
  - `/tmp/sideagent-bench-trial-3-consult-gpt-rerun.txt`
- Strengths: completed every run, passed `just check`, broad module coverage, decent trial 3 quality
- Issues: slowest median time, lower median host score, whole-file reads, parser fidelity concerns, missing runtime acceptance confidence, and cleanup issues across failure paths
- Safe and maintainable diff: mostly maintainable structurally, but `gpt-5.5` did not consider the trial 2 or trial 3 diffs safe to merge without lifecycle and event-tail refactors

Per-trial diff stats:

```text
trial 1:
Cargo.lock              | 253 +++++++++++++++++++++++++++++
Cargo.toml              |   1 +
README.md               |  45 +++++-
src/config.rs           |   4 +-
src/events.rs           | 192 ++++++++++++++++++++++
src/headless.rs         | 211 ++++++++++++++++++------
src/main.rs             |  32 ++++
src/parsers/codex.rs    | 175 ++++++++++++++++++++
src/parsers/cursor.rs   | 278 +++++++++++++++++++++++++++++++
src/parsers/mod.rs      |  22 +++
src/parsers/opencode.rs |   5 +
src/run.rs              | 126 ++++++++++++++-
src/run_archive.rs      | 423 ++++++++++++++++++++++++++++++++++++++++++++++++
src/run_dir.rs          | 152 +++++++++++++----
src/tmux.rs             |  21 +++
tests/cli.rs            | 216 ++++++++++++++++++++++++-
16 files changed, 2068 insertions(+), 88 deletions(-)

trial 2:
Cargo.lock              | 253 +++++++++++++++++++++++++++++++
Cargo.toml              |   1 +
README.md               |  46 +++++-
src/config.rs           |   4 +-
src/events.rs           | 171 +++++++++++++++++++++
src/headless.rs         | 209 ++++++++++++++++++++------
src/main.rs             |  28 ++++
src/parsers/codex.rs    |  43 ++++++
src/parsers/cursor.rs   |  66 ++++++++
src/parsers/mod.rs      |  40 +++++
src/parsers/opencode.rs |   5 +
src/run.rs              | 132 +++++++++++++++-
src/run_archive.rs      | 392 ++++++++++++++++++++++++++++++++++++++++++++++++
src/run_dir.rs          | 139 ++++++++++++++---
src/tmux.rs             |  21 +++
tests/cli.rs            | 218 +++++++++++++++++++++++++++
16 files changed, 1687 insertions(+), 81 deletions(-)

trial 3:
Cargo.lock              | 253 +++++++++++++++++++++++
Cargo.toml              |   1 +
README.md               |  38 +++-
src/config.rs           |   4 +-
src/events.rs           | 219 ++++++++++++++++++++
src/headless.rs         | 224 ++++++++++++++++-----
src/main.rs             |  35 ++++
src/parsers/codex.rs    | 176 ++++++++++++++++
src/parsers/cursor.rs   | 303 ++++++++++++++++++++++++++++
src/parsers/mod.rs      |  22 ++
src/parsers/opencode.rs | 130 ++++++++++++
src/run.rs              | 126 +++++++++++-
src/run_archive.rs      | 525 ++++++++++++++++++++++++++++++++++++++++++++++++
src/run_dir.rs          | 139 +++++++++++--
src/tmux.rs             |  21 ++
tests/cli.rs            | 196 ++++++++++++++++++
16 files changed, 2332 insertions(+), 80 deletions(-)
```

`consult-llm` analysis:

- `gemini-3.1-pro-preview`: scored trials 76, 75, and 88. It identified tmux pane leaks on failure, whole-file event reads in default `show`, and missing or weaker headless artifact testing in some trials.
- `gpt-5.5`: scored trials 78, 63, and 61. It praised broad coverage in trial 1 but was harsh on trial 2 and trial 3, flagging parser fidelity, large-log reads, missing runtime acceptance evidence, missing README warnings, and unsafe process or tmux cleanup. Its review makes the model look less consistent than the host scores alone suggest.

### `composer-2.5-fast`

`composer-2.5-fast` was still the most compelling model by the benchmark rubric. It was fastest by a wide margin and had the best median host score. The `gpt-5.5` rerun was more conservative than Gemini and pulled the consult ranges down, especially for trial 3, but it still scored this model above the other trial 3 results and did not change the host-ranked winner.

- Profile: `cursor-composer-fast`
- Median time: 2m 1.16s
- Success count: 3/3
- Median host score: 88
- Branches:
  - `bench/cursor-composer-fast-trial-1`
  - `bench/cursor-composer-fast-trial-2`
  - `bench/cursor-composer-fast-trial-3`
- Diffs:
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-1.diff`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-2.diff`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-3.diff`
- Check outputs:
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-1-check.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-2-check.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-3-check.txt`
- Acceptance outputs:
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-1-acceptance.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-2-acceptance.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-3-acceptance.txt`
- Timing outputs:
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-1-time.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-2-time.txt`
  - `/tmp/sideagent-bench-cursor-composer-fast-trial-3-time.txt`
- `consult-llm` outputs:
  - `/tmp/sideagent-bench-trial-1-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult.txt`
  - `/tmp/sideagent-bench-trial-2-consult-gpt-rerun.txt`
  - `/tmp/sideagent-bench-trial-3-consult.txt`
  - `/tmp/sideagent-bench-trial-3-consult-gpt-rerun.txt`
- Strengths: fastest, best median score, broad tests, good README coverage, strong trial 2 cleanup story
- Issues: trial 1 and trial 3 still had cleanup risks, and `gpt-5.5` flagged whole-file event reads, lossy Cursor parsing, unsafe lifecycle cleanup, and tee-thread exit-code masking
- Safe and maintainable diff: trial 2 remained the best balanced result, but the full consult set still called for bounded-read and lifecycle cleanup fixes before merging

Per-trial diff stats:

```text
trial 1:
Cargo.lock      | 253 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  45 ++++++++--
src/config.rs   |   4 +-
src/headless.rs | 210 +++++++++++++++++++++++++++++++++++-----------
src/main.rs     |  33 ++++++++
src/run.rs      | 125 +++++++++++++++++++++++++++-
src/run_dir.rs  | 113 ++++++++++++++++++++-----
src/tmux.rs     |  21 +++++
tests/cli.rs    | 221 ++++++++++++++++++++++++++++++++++++++++++++++++-
10 files changed, 946 insertions(+), 80 deletions(-)

trial 2:
Cargo.lock      | 253 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  44 ++++++++--
src/config.rs   |   4 +-
src/headless.rs | 212 ++++++++++++++++++++++++++++++++++++-----------
src/main.rs     |  28 +++++++
src/run.rs      | 127 +++++++++++++++++++++++++++-
src/run_dir.rs  | 113 ++++++++++++++++++++-----
src/tmux.rs     |  21 +++++
tests/cli.rs    | 214 ++++++++++++++++++++++++++++++++++++++++++++++-
10 files changed, 936 insertions(+), 81 deletions(-)

trial 3:
Cargo.lock      | 253 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
Cargo.toml      |   1 +
README.md       |  45 ++++++++--
src/config.rs   |   4 +-
src/headless.rs | 210 +++++++++++++++++++++++++++++++++++-----------
src/main.rs     |  28 +++++++
src/run.rs      | 125 +++++++++++++++++++++++++++-
src/run_dir.rs  | 113 ++++++++++++++++++++-----
src/tmux.rs     |  21 +++++
tests/cli.rs    | 213 +++++++++++++++++++++++++++++++++++++++++++++++
10 files changed, 934 insertions(+), 79 deletions(-)
```

`consult-llm` analysis:

- `gemini-3.1-pro-preview`: scored trials 78, 90, and 88. It rated trial 2 as the strongest implementation in that group because it handled error paths and included comprehensive tests, while still noting whole-file reads in default `show`.
- `gpt-5.5`: scored trials 76, 72, and 68. It liked the fake headless artifact tests and overall coverage, but flagged whole-file event reads, delayed text-event flushing, lossy Cursor parsing, unsafe lifecycle cleanup, and tee-thread failures that could be masked by a zero child exit code. It still rated this model best in trial 3.

### `gpt-5.4-mini`

`gpt-5.4-mini` completed all three headless trials and passed host `just check`, but it did not clear the host acceptance checklist in any trial. Trial 2 was the best final state because it included the expected `runs --json` and `show` variants plus README and integration test coverage. Trial 3 regressed on the public CLI and documentation surface, while trial 1 had a thinner accepted behavior story. The consult panel was the most polarized in the report: `gpt-5.5` and Gemini emphasized failed acceptance, unbounded reads, XDG/state-directory issues, and lifecycle risks, while DeepSeek rated trial 2 and trial 3 much more generously.

- Profile: `codex-mini`
- Median time: 7m 49.72s
- Success count: 0/3
- Median host score: 62
- Branches:
  - `bench/codex-mini-trial-1`
  - `bench/codex-mini-trial-2`
  - `bench/codex-mini-trial-3`
- Diffs:
  - `/tmp/sideagent-bench-codex-mini-trial-1.diff`
  - `/tmp/sideagent-bench-codex-mini-trial-2.diff`
  - `/tmp/sideagent-bench-codex-mini-trial-3.diff`
- Check outputs:
  - `/tmp/sideagent-bench-codex-mini-trial-1-check.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-2-check.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-3-check.txt`
- Acceptance outputs:
  - `/tmp/sideagent-bench-codex-mini-trial-1-acceptance.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-2-acceptance.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-3-acceptance.txt`
- Timing outputs:
  - `/tmp/sideagent-bench-codex-mini-trial-1-time.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-2-time.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-3-time.txt`
- `consult-llm` outputs:
  - `/tmp/sideagent-bench-codex-mini-trial-1-consult.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-2-consult.txt`
  - `/tmp/sideagent-bench-codex-mini-trial-3-consult.txt`
- Strengths: all trials compiled and passed `just check`, trial 2 added the broadest CLI, README, parser, archive, and integration-test coverage, and the module boundaries were generally sensible
- Issues: every acceptance output recorded fake Cursor artifact and `show` failures, trial 3 omitted README and CLI flag coverage, reviewers flagged whole-file reads, XDG/state-directory behavior, parser narrowness, and child or tmux cleanup risks
- Safe and maintainable diff: trial 2 was the most maintainable candidate, but none of the three diffs looked merge-ready without acceptance, bounded-read, and lifecycle fixes

Per-trial diff stats:

```text
trial 1:
 Cargo.lock              | 253 +++++++++++++++++++++++++++++
 Cargo.toml              |   1 +
 README.md               |  46 +++++-
 src/config.rs           |   4 +-
 src/events.rs           | 165 +++++++++++++++++++
 src/headless.rs         | 218 +++++++++++++++++++------
 src/main.rs             |  32 ++++
 src/parsers/codex.rs    | 160 ++++++++++++++++++
 src/parsers/cursor.rs   | 202 +++++++++++++++++++++++
 src/parsers/mod.rs      |  22 +++
 src/parsers/opencode.rs |   5 +
 src/run.rs              | 153 +++++++++++++++---
 src/run_archive.rs      | 420 ++++++++++++++++++++++++++++++++++++++++++++++++
 src/run_dir.rs          | 132 +++++++++++----
 src/tmux.rs             |  21 +++
 tests/cli.rs            | 143 +++++++++++++++++
 16 files changed, 1867 insertions(+), 110 deletions(-)

trial 2:
 Cargo.lock              | 253 +++++++++++++++++++++++++++++
 Cargo.toml              |   1 +
 README.md               |  37 +++++
 src/config.rs           |   4 +-
 src/events.rs           | 128 +++++++++++++++
 src/headless.rs         | 237 +++++++++++++++++++++------
 src/main.rs             |  35 ++++
 src/parsers/codex.rs    | 173 ++++++++++++++++++++
 src/parsers/cursor.rs   | 249 +++++++++++++++++++++++++++++
 src/parsers/mod.rs      |  22 +++
 src/parsers/opencode.rs |   5 +
 src/run.rs              | 146 +++++++++++++++--
 src/run_archive.rs      | 413 ++++++++++++++++++++++++++++++++++++++++++++++++
 src/run_dir.rs          | 126 +++++++++++----
 src/tmux.rs             |  21 +++
 tests/cli.rs            | 204 +++++++++++++++++++++++-
 16 files changed, 1965 insertions(+), 89 deletions(-)

trial 3:
 Cargo.lock              | 253 ++++++++++++++++++++++++++
 Cargo.toml              |   1 +
 src/config.rs           |   4 +-
 src/events.rs           | 128 +++++++++++++
 src/headless.rs         | 276 +++++++++++++++++++++-------
 src/main.rs             |  17 ++
 src/parsers/codex.rs    | 136 ++++++++++++++
 src/parsers/cursor.rs   | 195 ++++++++++++++++++++
 src/parsers/mod.rs      |  22 +++
 src/parsers/opencode.rs |   5 +
 src/run.rs              |  74 +++++++-
 src/run_archive.rs      | 464 ++++++++++++++++++++++++++++++++++++++++++++++++
 src/run_dir.rs          | 126 ++++++++++---
 src/tmux.rs             |  16 ++
 14 files changed, 1628 insertions(+), 89 deletions(-)
```

`consult-llm` analysis:

- `gpt-5.5`: scored trials 42, 58, and 58. It treated the failed acceptance workflow as the key cap, and repeatedly flagged fake Cursor archive failures, whole-file `show` reads, parser narrowness, child cleanup, and tee or recorder error handling.
- `gemini-3.1-pro-preview`: scored trials 60, 80, and 62. It liked the architecture more than `gpt-5.5`, especially trial 2, but still called out missing or failing CLI behavior, XDG/state-directory handling, unbounded `show` reads, and tmux pane capture risks.
- `deepseek-v4-pro`: scored trials 52, 92, and 96. It was much more forgiving of the acceptance failures, treating some as likely host-script or environment issues, and emphasized the modular architecture and passing tests. The host score does not follow that optimism because the recorded acceptance outputs had critical failures.

## Methodology limitations

- This was a manual benchmark, not an automated harness. The host followed the same procedure for each trial, but manual scripting can still introduce incidental differences.
- Trial clones were retained until consult review so reviewers could inspect final source context. They can be removed after this report is no longer needed.
- The external acceptance checklist was implemented as a host script. It produced pass evidence and `accept_rc=0`, but it was not a dedicated, versioned test harness. The rerun `gpt-5.5` reviews treated some acceptance evidence as lower-confidence than the host score did.
- The first `gpt-5.5` consult attempt succeeded for trial 1 only. After Max Mode was enabled, the missing trial 2 and trial 3 `gpt-5.5` reviews were rerun successfully and added to this report.
- The `gpt-5.4-mini` addendum was run later from the renamed repository path, `/Users/raine/code/agent-offload`, at base commit `5501a3098907f4c9f167b539eff7566e7e6846cd`. The earlier rows used base commit `5e7f57fdda7fd00a8f458e65d578fff939245870`.
- Consult scores judge final result quality only. Host scores are used for ranking. The full consult set is notably more skeptical than the host scores, mostly because it weighs parser fidelity, bounded reads, cleanup paths, and missing runtime evidence more heavily.
- Timing includes only the delegated implementation run, not host verification, consult review, artifact capture, or reporting.
- The models were asked to implement a detailed existing plan, so this benchmark measures execution of a specific task shape more than open-ended architecture skill.

## Winner

`composer-2.5-fast` wins by the host scoring rubric. It had the highest median host score, all three trials succeeded, and it had the fastest median elapsed time. Adding `gpt-5.4-mini` did not change the winner because `gpt-5.4-mini` had the slowest median time, a lower median host score, and zero acceptance successes. The full consult set still makes the race look less clean than host scores alone, especially because DeepSeek was much more optimistic about `gpt-5.4-mini` trial 2 and trial 3 than the host acceptance evidence supports.

## Takeaways

- `composer-2.5-fast`: fast and steady, and still the best host-ranked choice after the harsher consult rerun.
- `gpt-5.3-codex-spark`: capable of a high-scoring host result, but consult disagreement says to review its best-looking diffs carefully.
- `deepseek-v4-flash[1m]`: dependable completion, but bring snacks and expect more cleanup review.
- `gpt-5.4-mini`: it can build a lot of scaffolding, but this run needed more end-to-end discipline before the CLI contract was trustworthy.
- All successful agents found the broad shape of the feature. The hard part was not adding artifacts, it was cleaning up failure paths, parsing real stream formats, and avoiding sneaky large-file reads.
