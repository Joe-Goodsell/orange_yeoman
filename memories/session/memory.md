# Memory: Worktree plugin tmux pane mode

## Project Findings
- The OCX worktree plugin (`kdco/worktree`) installs readable (un-minified) TypeScript at `~/.opencode/plugins/worktree.ts` and `~/.opencode/plugins/worktree/terminal.ts`. Terminal spawning lives in `terminal.ts`; tool registration and config schema live in `worktree.ts`.
- Terminal type is auto-detected by `detectTerminalType()` in `terminal.ts`: tmux wins if `process.env.TMUX` is set, then cmux, then platform terminal. Not configurable via `worktree.jsonc` (the schema only has `worktreePath`, `sync`, `hooks`).
- The only tmux spawn path is `openTmuxWindow()` in `terminal.ts`. The sole runtime caller is `openTerminalByType()` (around `terminal.ts:1323`), which passes `windowName`, `cwd`, `argv` and never `sessionName`. The `sessionName` branch is dead code today.
- `worktree_create` and `worktree_delete` are plugin-registered tools (via `@opencode-ai/plugin` `tool()`), not opencode core tools.
- `ocx update kdco/worktree` overwrites the plugin source; the pane-mode edit is not durable across updates.

## Key Decisions
- Worktrees spawn as tmux panes (not windows) in the current tmux window, gated by env var `OPENCODE_WORKTREE_TMUX_TARGET=pane`. Any other value or absence preserves the original `tmux new-window` behavior byte-for-byte. Rationale: the user wants to view all orchestrators simultaneously; env-gating keeps other projects and environments on default behavior without config-schema changes.
- Layout `tiled` is applied after each pane split via `tmux select-layout tiled`, inside the existing `tmuxMutex` to avoid same-process races. Rationale: keeps pane sizes balanced as orchestrators are added. Accepted trade-off: it resizes all panes in the window on every create.
- The pane title is set to the branch name via `tmux select-pane -t <paneId> -T <windowName>` so panes are identifiable. `~/.tmux.conf` has `set -g pane-border-status top` to render the titles in the top border.

## Assumptions
- One active worktree/feature at a time is the common case. Tiled layout remains usable up to roughly four concurrent panes. Beyond that, windows are more practical (not enforced).
- The env var is set globally in `~/.zshrc` (line 180), so it applies to all opencode-in-tmux sessions. Acceptable because the user works on orange_yeoman as the primary project.

## Trade-offs
- Chose env-gated global plugin edit over (a) a project-local plugin fork (tool-override collision behavior is unverified) and (b) adding a `worktree.jsonc` schema field (would require threading config through `openTerminal` -> `openTerminalByType` -> `openTmuxWindow`, more invasive). Env-gating is minimal and reversible; the cost is non-durability across `ocx update`.
- Chose isolated worktree caches in `.opencode/worktree.jsonc` (`symlinkDirs: []`, `postCreate: ["pnpm install"]`) over shared or symlinked caches. Cost: each worktree rebuilds `src-tauri/target` from scratch (expensive for Rust). Benefit: zero cache contention between concurrent worktrees. pnpm hardlinks from its global store, so the JS install cost is low.

## User Feedback
- The user uses tmux (not cmux) and wanted panes specifically to view all orchestrators at once.
- The user requested the env var name reflect that it is opencode-specific; renamed from `WORKTREE_TMUX_TARGET` to `OPENCODE_WORKTREE_TMUX_TARGET`.
- The user chose the isolated cache strategy for `.opencode/worktree.jsonc`.

## Implementation Blockers
- `ocx update kdco/worktree` overwrites `~/.opencode/plugins/worktree/terminal.ts` and erases the pane-mode edit. Re-apply by editing `openTmuxWindow()` per the re-apply steps below, or pin the plugin version (`~/.ocx/receipt.jsonc` records `kdco/worktree@sha256:b5306aa8166234f186cde908f519f4f4667f70161fc2dbdb5055a8f79d9ecb30`).
- The memory artifact did not exist before this task; it was created at `memories/session/memory.md` (repo-relative).

## Re-apply steps (after `ocx update kdco/worktree` overwrites the plugin)
File: `~/.opencode/plugins/worktree/terminal.ts`, function `openTmuxWindow`.
1. After the `const command = buildBashCommandFromArgv(argv)` line, add: `const usePane = process.env.OPENCODE_WORKTREE_TMUX_TARGET === "pane"`
2. Replace the `tmuxArgs` initialization with a ternary: `["split-window", "-d", "-c", cwd, "-P", "-F", "#{pane_id}"]` for pane mode, else the original `["new-window", "-n", windowName, "-c", cwd, "-P", "-F", "#{pane_id}"]`.
3. In the `sessionName` branch, target `${sessionName}:` for pane mode (split-window targets a window within a session, not a bare session).
4. The error message uses `usePane ? "pane" : "window"`.
5. After the `Bun.sleep(STABILIZATION_DELAY_MS)` line, in pane mode only: read `createResult.stdout.toString().trim()` as `paneId`; if non-empty, run `Bun.spawnSync(["tmux", "select-pane", "-t", paneId, "-T", windowName])`; then run `Bun.spawnSync(["tmux", "select-layout", "tiled"])`.
6. The default path (env var absent or not "pane") must remain byte-for-byte identical to the original.
7. Tab indentation must be preserved (the file uses tabs, not spaces).