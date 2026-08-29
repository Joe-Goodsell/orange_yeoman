# Memory: Worktree plugin tmux pane mode

## Project Findings
- The OCX worktree plugin (`kdco/worktree`) installs readable (un-minified) TypeScript at `~/.opencode/plugins/worktree.ts` and `~/.opencode/plugins/worktree/terminal.ts`. Terminal spawning lives in `terminal.ts`; tool registration and config schema live in `worktree.ts`.
- Terminal type is auto-detected by `detectTerminalType()` in `terminal.ts`: tmux wins if `process.env.TMUX` is set, then cmux, then platform terminal. Not configurable via `worktree.jsonc` (the schema only has `worktreePath`, `sync`, `hooks`).
- The only tmux spawn path is `openTmuxWindow()` in `terminal.ts`. The sole runtime caller is `openTerminalByType()` (around `terminal.ts:1323`), which passes `windowName`, `cwd`, `argv` and never `sessionName`. The `sessionName` branch is dead code today.
- `worktree_create` and `worktree_delete` are plugin-registered tools (via `@opencode-ai/plugin` `tool()`), not opencode core tools.
- tmux sets `TMUX_PANE` in the environment of every process started in a pane. The opencode process therefore carries the pane id of the orchestrator pane, used to target splits and keep the orchestrator as the main (left) pane.
- `ocx update kdco/worktree` overwrites the plugin source; the pane-mode edit is not durable across updates.

## Key Decisions
- Worktrees spawn as tmux panes (not windows) in the current tmux window, gated by env var `OPENCODE_WORKTREE_TMUX_TARGET=pane`. Any other value or absence preserves the original `tmux new-window` behavior byte-for-byte. Rationale: the user wants to view all orchestrators simultaneously; env-gating keeps other projects and environments on default behavior without config-schema changes.
- The split uses `tmux split-window -h -d` (horizontal split, non-focus-stealing) so the new pane appears to the right of the orchestrator. The split targets `process.env.TMUX_PANE` explicitly so the new pane splits to the right of the orchestrator pane even if the user has focused another pane.
- After each split, `tmux select-pane -t <TMUX_PANE>` ensures the orchestrator pane is active, then `tmux select-layout main-vertical` arranges the layout: the orchestrator pane stays on the left as the main pane, and all other panes stack vertically to its right. Both run inside the existing `tmuxMutex` to avoid same-process races. Rationale: the user requires the main orchestrator to remain on the left with new panes to the right. Accepted trade-off: the main pane width follows tmux's `main-pane-width` (default ~80 cols); the right column gets shorter as panes are added.
- The new pane's title is set to the branch name via `tmux select-pane -t <paneId> -T <windowName>` so panes are identifiable. `~/.tmux.conf` has `set -g pane-border-status top` to render the titles in the top border.

## Assumptions
- One active worktree/feature at a time is the common case. The right column remains usable up to roughly four concurrent panes. Beyond that, windows are more practical (not enforced).
- The env var is set globally in `~/.zshrc` (line 180), so it applies to all opencode-in-tmux sessions. Acceptable because the user works on orange_yeoman as the primary project.
- `TMUX_PANE` is present whenever `TMUX` is present (both set by tmux for processes in a pane), so the explicit target is available in every pane-mode invocation.

## Trade-offs
- Chose env-gated global plugin edit over (a) a project-local plugin fork (tool-override collision behavior is unverified) and (b) adding a `worktree.jsonc` schema field (would require threading config through `openTerminal` -> `openTerminalByType` -> `openTmuxWindow`, more invasive). Env-gating is minimal and reversible; the cost is non-durability across `ocx update`.
- Chose `main-vertical` layout (main pane left, others stacked vertically right) over `tiled` (grid) so the orchestrator stays prominent on the left. Trade-off: right-column panes get shorter as more are added; beyond ~4 the right column becomes cramped. The main pane width is governed by tmux's `main-pane-width` option (tunable in `~/.tmux.conf`).
- Chose isolated worktree caches in `.opencode/worktree.jsonc` (`symlinkDirs: []`, `postCreate: ["pnpm install"]`) over shared or symlinked caches. Cost: each worktree rebuilds `src-tauri/target` from scratch (expensive for Rust). Benefit: zero cache contention between concurrent worktrees. pnpm hardlinks from its global store, so the JS install cost is low.

## User Feedback
- The user uses tmux (not cmux) and wanted panes specifically to view all orchestrators at once.
- The user requested the env var name reflect that it is opencode-specific; renamed from `WORKTREE_TMUX_TARGET` to `OPENCODE_WORKTREE_TMUX_TARGET`.
- The user chose the isolated cache strategy for `.opencode/worktree.jsonc`.
- The user required new panes to open to the right of the main orchestrator, with the orchestrator remaining on the left; led to the switch from `tiled` to `main-vertical` and explicit `TMUX_PANE` targeting.

## Implementation Blockers
- `ocx update kdco/worktree` overwrites `~/.opencode/plugins/worktree/terminal.ts` and erases the pane-mode edit. Re-apply by editing `openTmuxWindow()` per the re-apply steps below, or pin the plugin version (`~/.ocx/receipt.jsonc` records `kdco/worktree@sha256:b5306aa8166234f186cde908f519f4f4667f70161fc2dbdb5055a8f79d9ecb30`).
- The memory artifact lives at `memories/session/memory.md` (repo-relative).

## Re-apply steps (after `ocx update kdco/worktree` overwrites the plugin)
File: `~/.opencode/plugins/worktree/terminal.ts`, function `openTmuxWindow`.
1. After the `const command = buildBashCommandFromArgv(argv)` line, add:
   `const usePane = process.env.OPENCODE_WORKTREE_TMUX_TARGET === "pane"`
   `const sourcePaneId = process.env.TMUX_PANE`
2. Build `tmuxArgs` with an if/else. Pane mode: `["split-window", "-h", "-d", "-c", cwd, "-P", "-F", "#{pane_id}"]`; if `sourcePaneId` is set, splice `-t <sourcePaneId>` after `split-window`, else if `sessionName` is set, splice `-t ${sessionName}:`. Default mode: `["new-window", "-n", windowName, "-c", cwd, "-P", "-F", "#{pane_id}"]`; if `sessionName` is set, splice `-t sessionName`.
3. The error message uses `usePane ? "pane" : "window"`.
4. After the `Bun.sleep(STABILIZATION_DELAY_MS)` line, in pane mode only: read `createResult.stdout.toString().trim()` as `paneId`; if non-empty, run `Bun.spawnSync(["tmux", "select-pane", "-t", paneId, "-T", windowName])`. Then if `sourcePaneId` is set, run `Bun.spawnSync(["tmux", "select-pane", "-t", sourcePaneId])` to make the orchestrator active. Then run `Bun.spawnSync(["tmux", "select-layout", "main-vertical"])`.
5. The default path (env var absent or not "pane") must remain byte-for-byte identical to the original.
6. Tab indentation must be preserved (the file uses tabs, not spaces).

# Memory: Markdown syntax highlighting (feature/markdown-syntax-highlighting)

## Project Findings
- The editor is CodeMirror 6 (src/lib/Editor.svelte). The extensions array is built in onMount; markdownHighlight() sits right after EditorView.lineWrapping and before feedbackField. Feedback decorations are background overlays and coexist with text-color highlight marks on the same spans; extension order is not load-bearing for that coexistence.
- @codemirror/lang-markdown's markdown() factory uses the commonmark grammar by default (base parser is commonmarkLanguage), NOT GFM. GFM (strikethrough, tables, task lists) and YAML frontmatter require extensions: [GFM] and a direct @lezer/markdown dep. Verified against the installed package source.
- @codemirror/language@6.12.4 does NOT re-export tags from @lezer/highlight.

## Key Decisions
- Syntax highlighting is token coloring only (Lezer markdown parser + HighlightStyle via syntaxHighlighting). Rendered markdown preview is NOT implemented and remains roadmap per outline.md. This clears the prior "syntax highlighting remain roadmap" blocker; Vim keybindings and markdown rendering still remain roadmap.
- @lezer/highlight is a direct dependency because pnpm strict mode does not hoist transitive deps, and importing tags for HighlightStyle.define requires it. General rule for this repo: any @lezer/* or @codemirror/* package imported from src/ must be declared as a direct dependency.
- Single muted HighlightStyle palette reads on both light (#ffffff) and dark (#1e1e1e) backgrounds, rather than two prefers-color-scheme-keyed stylesheets. Contrast ranges roughly 3:1 to 5:1. Heading color (#5b6e8c) is the weakest on dark (3.22:1) but is compensated by fontWeight 600.
- HighlightStyle rule order: the heading rule is declared last so the # markers (which carry both heading and markup tags) adopt the heading color via CSS precedence; list/code/link markup markers stay quiet gray.

## Trade-offs
- Chose commonmark over GFM. Cost: no strikethrough, table, or task-list token colors. Benefit: smaller dep surface; GFM is a well-scoped follow-up (the HighlightStyle already covers tags.contentSeparator and tags.processingInstruction that frontmatter delimiters would use).
- Chose a single dual-theme palette over two stylesheets. Cost: heading contrast is the weakest point on dark. Benefit: simpler, one source of truth.

## Implementation Blockers / Open Follow-ups
- No JS test infrastructure in the repo, so the HighlightStyle tag mapping has no automated regression test. A runtime simulation validated it against installed package versions.
- GFM and YAML-frontmatter highlighting are follow-ups (need extensions: [GFM] and a direct @lezer/markdown dep).

# Memory: Mock model feedback wiring (feature/mock-feedback-wiring)

## Project Findings
- The feedback UI is fully built but was unfed: `AgentPane.svelte` renders cards with linked/focused/expand/re-run states; `Editor.svelte` has a `feedbackField` StateField with `cm-feedback` decorations, click-to-focus, and a selection-driven `linkedFeedbackIds` derived in the store. The missing piece was a transport driving the store from slash command dispatch.
- `onTaskUpdated` / `onResultReady` (src/lib/tauri.ts) wrap `agent://task-updated` and `agent://result-ready` Rust events but are STILL never subscribed. Rust returns `Ok(None)` for `/research` and `/ignore` (roadmap), so wiring those events today would surface nothing for two of three commands. Only `/fact-check` routes to the mock provider.
- `AgentPane.svelte` renders feedback for ALL files; it does NOT filter by the open file. The editor hides foreign-file decorations via `f.range.file === file` in `buildFeedbackDecorations`, but the pane shows every card. This is pre-existing, not introduced here.
- The app has no save action. Local edits set `project.dirty` only; `project.openFileContent` is the open-time snapshot and is NOT updated on local edits. The watcher does not see local edits until another tool writes the file.

## Key Decisions
- A frontend mock dispatcher (`project.dispatchMockFeedback(command, range)`) is the placeholder transport, driven by `onEnterDispatch` in Editor.svelte alongside the existing `submitBlock` IPC call (which is preserved for the Rust debug event and the future real path). Rationale: reliably demonstrates linking for all three commands; the store comments already anticipate a swap to the real transport.
- Local-edit range stability is achieved by mapping store ranges through CodeMirror `ChangeSet.mapPos` on every local edit. The editor's `updateListener` calls `project.mapFeedbackRanges(u.changes.mapPos.bind(u.changes), project.openFilePath)` on `u.docChanged && !isRemote`. `from` maps with assoc -1, `to` with assoc +1 (canonical range-stability convention). The store stays CodeMirror-free: the `mapPos` callable is passed in.
- The decoration rebuild `$effect` bound changed from `Math.min(v.state.doc.length, content.length)` to `v.state.doc.length`, and the `const content = project.openFileContent` read was dropped from that effect. Rationale: `content` is the stale open-time snapshot; after local typing, `content.length < docLen`, and `Math.min` clipped or dropped the freshly dispatched range. With range mapping on local edits and stale-marking on external edits (`pushChange`), the live doc length is the correct bound. The effect still re-runs on `openFilePath` (file reopen) and `feedback` changes.

## Trade-offs
- Frontend mock duplicates the mock concept (the Rust core also has a deterministic mock provider). Cost: two mock sources to retire later. Benefit: all three commands produce visible feedback today; no dependency on unverified Rust event emission.
- `mapFeedbackRanges` reassigns `this.feedback` on every keystroke, re-running `linkedFeedbackIds` and the rebuild `$effect` (one effects-only dispatch per keystroke). Acceptable at mock scale; revisit when the real transport produces many items.
- Degenerate ranges (full deletion of a paragraph) store mapped `from >= to` values; the `buildFeedbackDecorations` guard skips them and the card remains in the pane. No stale marking on full deletion; accepted for now.

## Implementation Blockers / Open Follow-ups
- `onTaskUpdated` / `onResultReady` remain unsubscribed. Wiring them is the path to retire the frontend mock, but requires Rust to route `/research` and `/ignore` (currently `Ok(None)`) and confirmed event emission end-to-end.
- `AgentPane.svelte` does not filter feedback by the open file. A future change may scope the pane to the current file or group by file.
- No save action exists; local edits never reach the watcher. A save/persist path is a separate follow-up.
