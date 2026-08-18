# Memory: Worktree plugin tmux pane grid layout

## Project Findings
- The OCX worktree plugin (`kdco/worktree`) installs readable (un-minified) TypeScript at `~/.opencode/plugins/worktree.ts` and `~/.opencode/plugins/worktree/terminal.ts`. Terminal spawning lives in `terminal.ts`; tool registration and config schema live in `worktree.ts`.
- Terminal type is auto-detected by `detectTerminalType()` in `terminal.ts`: tmux wins if `process.env.TMUX` is set, then cmux, then platform terminal. Not configurable via `worktree.jsonc` (the schema only has `worktreePath`, `sync`, `hooks`).
- The only tmux spawn path is `openTmuxWindow()` in `terminal.ts`. The sole runtime caller is `openTerminalByType()` (around `terminal.ts:1323`), which passes `windowName`, `cwd`, `argv` and never `sessionName`. The `sessionName` branch is dead code today.
- `worktree_create` and `worktree_delete` are plugin-registered tools (via `@opencode-ai/plugin` `tool()`), not opencode core tools.
- `ocx update kdco/worktree` overwrites the plugin source; the pane-mode edit is not durable across updates.

## Key Decisions
- Worktrees spawn as tmux panes (not windows) in the current tmux window, gated by env var `OPENCODE_WORKTREE_TMUX_TARGET=pane`. Any other value or absence preserves the original `tmux new-window` behavior byte-for-byte. Rationale: the user wants to view all orchestrators simultaneously; env-gating keeps other projects and environments on default behavior without config-schema changes.
- Pane layout is a count-driven capped grid, not a fixed split direction. Before each spawn, `openTmuxWindow` queries `tmux display-message -p "#{window_panes}"` (inside `tmuxMutex` to avoid count/split races). Decision rule by current pane count: 0,1,2,3 -> `split-window -h` (fill row 1 left to right); 4 -> `split-window -v` (start row 2); 5,6,7 -> `split-window -h` (fill row 2); >=8 -> `new-window` (overflow to a fresh window, cycle restarts). Rationale: the user wants up to 4 panes per row, 2 rows (8 total), then a new window. Matches a 4x2 grid then wrap.
- Post-split layout is `even-horizontal` when total panes <= 4 (single balanced row) and `tiled` when >= 5 (grid; reaches exactly 4+4 at 8 panes). Intermediate counts (5,6,7) produce 3+2, 3+3, 4+3 under `tiled` — accepted approximation; exact 4+N intermediates would need custom tmux layout strings (fragile, rejected).
- The pane title is set to the branch name via `tmux select-pane -t <paneId> -T <windowName>` so panes are identifiable. `~/.tmux.conf` has `set -g pane-border-status top` to render the titles in the top border. New-window overflow does not set a pane title (it uses the window name via `-n`).
- `useSplit` (not `usePane`) gates all split-specific behavior: session targeting (`${sessionName}:`), error text ("pane" vs "window"), and post-split title/layout work. Rationale: pane mode can now produce `new-window` on overflow, so code keyed on `usePane` would mislabel the overflow case. `usePane` remains the mode switch only.

## Assumptions
- One active worktree/feature at a time is the common case. Tiled layout remains usable up to roughly four concurrent panes. Beyond that, windows are more practical (not enforced).
- The env var is set globally in `~/.zshrc` (line 180), so it applies to all opencode-in-tmux sessions. Acceptable because the user works on orange_yeoman as the primary project.

## Trade-offs
- Chose env-gated global plugin edit over (a) a project-local plugin fork (tool-override collision behavior is unverified) and (b) adding a `worktree.jsonc` schema field (would require threading config through `openTerminal` -> `openTerminalByType` -> `openTmuxWindow`, more invasive). Env-gating is minimal and reversible; the cost is non-durability across `ocx update`.
- Chose `tiled` for the 5-7 pane intermediates over custom tmux layout strings. Cost: intermediates are 3+2, 3+3, 4+3 rather than exactly 4+1, 4+2, 4+3. Benefit: simple, robust, reaches the exact 4+4 target at 8 panes.
- Chose isolated worktree caches in `.opencode/worktree.jsonc` (`symlinkDirs: []`, `postCreate: ["pnpm install"]`) over shared or symlinked caches. Cost: each worktree rebuilds `src-tauri/target` from scratch (expensive for Rust). Benefit: zero cache contention between concurrent worktrees. pnpm hardlinks from its global store, so the JS install cost is low.

## User Feedback
- The user uses tmux (not cmux) and wanted panes specifically to view all orchestrators at once.
- The user requested the env var name reflect that it is opencode-specific; renamed from `WORKTREE_TMUX_TARGET` to `OPENCODE_WORKTREE_TMUX_TARGET`.
- The user chose the isolated cache strategy for `.opencode/worktree.jsonc`.

## Implementation Blockers
- `ocx update kdco/worktree` overwrites `~/.opencode/plugins/worktree/terminal.ts` and erases the pane-mode edit. Re-apply by editing `openTmuxWindow()` per the re-apply steps below, or pin the plugin version (`~/.ocx/receipt.jsonc` records `kdco/worktree@sha256:b5306aa8166234f186cde908f519f4f4667f70161fc2dbdb5055a8f79d9ecb30`).
- The memory artifact did not exist before this task; it was created at `memories/session/memory.md` (repo-relative).

## Re-apply steps (after `ocx update kdco/worktree` overwrites the plugin)
File: `~/.opencode/plugins/worktree/terminal.ts`, function `openTmuxWindow` (tab-indented).
1. After the `const command = buildBashCommandFromArgv(argv)` line, keep `const usePane = process.env.OPENCODE_WORKTREE_TMUX_TARGET === "pane"`.
2. Inside the `tmuxMutex.runExclusive(async () => { try {` block, replace the static `tmuxArgs` ternary with a count-driven decision block: declare `let useSplit = false` and `let splitVertical = false`; if `usePane`, run `Bun.spawnSync(["tmux", "display-message", "-p", "#{window_panes}"])`, parse to int as `paneCount` (fallback 0); if `paneCount >= 8` leave `useSplit` false (overflow -> new-window); else if `paneCount === 4` set `useSplit = true; splitVertical = true`; else set `useSplit = true; splitVertical = false`.
3. Build `tmuxArgs`: if `useSplit` then `["split-window", splitVertical ? "-v" : "-h", "-d", "-c", cwd, "-P", "-F", "#{pane_id}"]`, else `["new-window", "-n", windowName, "-c", cwd, "-P", "-F", "#{pane_id}"]`.
4. In the `sessionName` branch, target `useSplit ? `${sessionName}:` : sessionName` (split-window targets a window within a session).
5. The error message uses `useSplit ? "pane" : "window"`.
6. After the `Bun.sleep(STABILIZATION_DELAY_MS)` line, gate on `if (useSplit)`: read `createResult.stdout.toString().trim()` as `paneId`; if non-empty, run `Bun.spawnSync(["tmux", "select-pane", "-t", paneId, "-T", windowName])`; re-query `tmux display-message -p "#{window_panes}"` as `totalPanes` (fallback 1); run `Bun.spawnSync(["tmux", "select-layout", totalPanes <= 4 ? "even-horizontal" : "tiled"])`.
7. The default path (env var absent or not "pane") must remain byte-for-byte identical to the original: `useSplit` and `splitVertical` stay false, `tmuxArgs` is the `new-window` array, no post-split title/layout work.
8. Tab indentation must be preserved (the file uses tabs, not spaces).

# Memory: Slash command detection wiring (fix/slash-command-detection)

## Project Findings
- The slash command pipeline existed end-to-end in Rust but was never wired to the frontend. `submit_block` (renamed from `submit_auto_task`) parses a block, runs `parse_slash_command_in_block` (src-tauri/src/pipeline.rs:358), emits a `slash_command` debug event (src-tauri/src/tasks.rs ~484-498) BEFORE `route_block`, then routes. Only `/fact-check` dispatches an LLM task (Trigger::Inline); `/research` and `/ignore` return Ok(None) (roadmap). The frontend DebugConsole already rendered the event via the same path the watcher uses.
- There is NO deferral layer in Rust. `dispatch_task` (src-tauri/src/tasks.rs:341-446) calls `async_runtime::spawn` (line 354) with zero delay; the provider call is line 386. The dedup gate at tasks.rs:252 applies ONLY to Trigger::Automatic, NOT Trigger::Inline, so repeated `submit_block` calls for the same `/fact-check` block spawn repeated LLM calls. Frontend dedup is therefore mandatory.
- The editor is CodeMirror 6 (src/lib/Editor.svelte). Its `updateListener` only tracks dirty + selection. The autocomplete extension is `@codemirror/autocomplete`; `completionStatus(view.state)` returns null | "active" | "pending".
- The watcher (src-tauri/src/watcher.rs) does NOT call submit_block despite commit b68edac's message claiming it wires inline /fact-check dispatch. Watcher-side dispatch is a separate open follow-up.

## Key Decisions
- Detection and model-API dispatch are strictly separated. Detection is continuous and cheap (frontend only, no IPC). The model API fires ONLY when the user presses Enter (newline), even for slash commands. This OVERRIDES research/concept-extraction-and-prompting.md:54, which says explicit /fact-check and /research commands should override the active-line grace period. The user explicitly chose stricter behavior for now; revisit consciously if immediate firing is ever wanted.
- `submit_auto_task` was renamed to `submit_block` (trigger-neutral) so the single IPC entry serves both the slash-command caller (now) and the future automatic-everything caller. `submitBlock` is the TS wrapper in src/lib/tauri.ts.
- The frontend re-implements the slash token bounding rule in src/lib/slashCommands.ts `detectSlashCommandInLine`, mirroring Rust `parse_slash_command_in_line` (pipeline.rs:288-313). The two registries (SLASH_COMMANDS / COMMAND_REGISTRY) MUST stay in sync. A known minor divergence: the TS whitespace set includes `\v` (0x0B), which Rust `u8::is_ascii_whitespace` does not; unreachable in editor content.
- The autocomplete popup closes once the command word is complete and bounded (slashCommandSource returns null for a complete command), so Enter never has to choose between accepting a completion and dispatching. Enter dispatch is a side effect that returns false so the default newline still inserts.
- `submit_block` selects the first non-excluded block that CONTAINS a slash command, falling back to the first non-excluded block (auto path). This prevents silently dropping a command that follows a heading/list in the same blank-line-bounded region.
- `paragraphAround` (slashCommands.ts) returns an empty slice when the cursor sits on an empty line. Rationale: a trailing-newline document made the post-Enter paragraph hash differ from the dispatched paragraph, causing a second Enter on the fresh empty line to re-dispatch. Offsets are UTF-16 JS indices, consistent with CodeMirror; offsets are never sent to Rust.
- Dispatch dedup key is `${commandName}:${hashText(paragraphText)}` (FNV-1a). The Set clears on file switch. Only the first command in a paragraph dispatches (mirrors Rust parse_slash_command_in_block).

## Trade-offs
- Chose the neutral rename over a dedicated `submit_inline_command` IPC split. Cost: one shared function handles two trigger types. Benefit: small, safe, no duplicated dispatch logic. A hard split is a viable follow-up if the inline path grows apart from the auto path.
- Chose to send only the paragraph (blank-line-bounded) as blockText, not the full file. Cost: inline /fact-check loses heading-chain context (Rust gets an empty chain). Benefit: minimal payload, matches the cursor's local context.
- Chose CodeMirror autocompletion over a custom Svelte overlay. Cost: a new JS dep (@codemirror/autocomplete). Benefit: cursor anchoring, prefix filtering, and keyboard nav for free.

## Implementation Blockers / Open Follow-ups
- No TS unit tests for the slashCommands helpers (R6 from review). The Rust side has tests for the non-match cases. Add a frontend test harness or mirror tests to guard against registry/bounding drift.
- The full active-line grace period for the automatic path (Enter / cursor-leaves-line / focus-loss / idle) is NOT implemented. Only slash commands dispatch, and only on Enter. Design intent lives in research/concept-extraction-and-prompting.md:43-56.
- Watcher-side slash command dispatch is not wired (watcher emits events only).
- AgentPane (src/lib/AgentPane.svelte) still uses mock feedback; onTaskUpdated / onResultReady are never subscribed.
- /research and /ignore dispatch is roadmap (Rust returns Ok(None)).
- Markdown rendering and syntax highlighting remain roadmap.

## User Feedback
- The user flagged that calling `submitAutoTask` for a slash command was misnamed ("not auto"); drove the trigger-neutral rename to `submitBlock`.
- The user required detection vs model-API call be separated, with the model API deferred until newline (Enter), and pointed to the /research docs. This shaped the dispatch-on-Enter design and the recorded override of the doc's grace-period-bypass clause.
- The user requested a minimalist autocomplete dropdown at the cursor showing all supported commands, refined as the user types, with a roadmap entry for better contextual ranking.