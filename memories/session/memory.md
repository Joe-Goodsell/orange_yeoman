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

# Memory: Replace custom markdown parser with markdown-rs (mdast adapter)

## Project Findings
- The custom line-based markdown parser in src-tauri/src/pipeline.rs was too ambitious for the CommonMark/GFM spec. It was replaced with `markdown::to_mdast` (markdown-rs 1.0.0, MIT; pulls `unicode-id`).
- mdast `unist::Point.offset` is a BYTE offset into the source string, not a character index. `input[start..end]` slicing is safe on multi-byte UTF-8. Verified by a spike and locked in by the `multibyte_offsets_are_bytes` test in pipeline/tests.rs.
- mdast API shape: the `Node` enum exposes `position()` and `children()` as methods, but the inner node structs (`ListItem`, `Yaml`, `Code`, `Heading`, etc.) carry `position: Option<Position>` as a public FIELD. Accessing a struct's position uses the field, not a method.
- Canonical parse config: `markdown::ParseOptions { constructs: markdown::Constructs { frontmatter: true, ..markdown::Constructs::gfm() }, ..markdown::ParseOptions::default() }`. This enables GFM tables and `---` frontmatter on top of CommonMark defaults. `to_mdast` errors only on MDX, so `.expect()`/`.unwrap()` is safe for normal markdown.
- mdast block positions include their closing delimiters: `Code` includes the closing fence, `Yaml`/`Toml` includes the closing `---`, `Blockquote` includes the `>` marker. `List` positions include a trailing newline; loose `ListItem` positions end with `\n` (tight items do not). All slice back exactly via `input[start..end]`.

## Key Decisions
- Option A adapter approach: keep the existing `MarkdownBlock` / `BlockKind` interface intact and reimplement only `parse_markdown_blocks` as a `to_mdast` tree-walk. `tasks.rs` and all downstream code stayed byte-for-byte unchanged. Rationale: `heading_chain` (all preceding heading texts in document order) is the one field not derivable from a single mdast node; producing it forces a walk-and-attach step regardless, so keeping the other derived fields (`text`, `block_hash`, `excluded`) on the same struct avoids reworking every consumer for no functional gain.
- `block.text` always equals `input[start..end]` (never trimmed). The `input[block.start..block.end] == block.text` invariant is the load-bearing contract for staleness checks in tasks.rs.
- Top-level node mapping: Heading -> Heading (then push `collect_text(node)` to the chain AFTER emitting, so a heading's own chain excludes itself); Paragraph/Blockquote/Table -> one block each; Yaml|Toml -> excluded FrontMatter; Code -> excluded CodeFence; Html -> excluded Html; List -> flattened to one ListItem block per child using each ListItem's own position; all other top-level nodes (ThematicBreak, Definition, FootnoteDefinition, Math, MDX nodes) -> skipped. Headings nested inside blockquotes/lists never reach the top-level walk and never update the chain, matching old behavior.
- `collect_text` rebuilds plain text from inline `Text` descendants, dropping formatting markers (`## **Bold**` -> chain entry `Bold`). Accepted; the chain feeds only fact-check prompt context.

## Assumptions
- No persisted data depends on old `block_hash` values; `TaskStore` is in-memory, so the hash-identity changes from spec-correct block boundaries do not require migration.
- The app parses user `.md` notes (normal markdown), never MDX, so `to_mdast` never errors in practice.

## Trade-offs
- Chose the adapter (Option A) over reworking consumers to use `mdast::Node` directly (Option B). Cost: a thin projection layer (`emit_blocks`/`push_block`/`collect_text`) lives in pipeline.rs. Benefit: tasks.rs, routing, envelopes, and their tests are untouched; the blast radius is the parser internals plus two test files. Option B would have required a heading-chain side-channel (`HashMap` keyed by offset) anyway, so it would not have eliminated a derived struct.
- Accepted spec-correct behavior deltas (the intended payoff of the switch): mid-document `---` is now `ThematicBreak` (was `Paragraph`); `foo: bar\n---` is a setext `Heading`; 4-space indented code is now excluded `CodeFence` (was a scored `Paragraph`); nested/loose list items merge into one `ListItem` block per item; malformed/unclosed frontmatter no longer consumes to EOF as an excluded block. These change `block_hash` identities for those inputs and are acceptable.
- Added `markdown` + `unicode-id` as deps, ending the old parser's "no dependencies beyond std" purity.

## User Feedback
- The user challenged keeping `MarkdownBlock` and asked whether the crate exposes a block type. Clarified that mdast exposes a recursive tree (`Node`), not a flat block type; downstream iterates a flat list, so a tree-to-list projection is unavoidable. `heading_chain` is the field that forces the projection. User then approved Option A explicitly.

## Implementation Blockers / Open Follow-ups
- Pre-existing dead-code warnings for `decision_trigger` (pipeline.rs) and `PROMPT_SCHEMA_VERSION` (pipeline.rs) are unrelated to this change and were intentionally not fixed.
- GFM tables require a delimiter row; pipe-prefixed lines without one parse as `Paragraph` (correct, but no test pins it).

---

# Memory: pr subagent for delegated PR-raising (orchestration)

## Project Findings
- Agent orchestration is split: primary agents (`orchestrator`, `plan`, `build`, `supervisor`) are model-agnostic; subagents (`explore`, `general`, `scout`, `implementation`, `quick-implementation`, `review`, `merge`, `pr`) pin `opencode-go/deepseek-v4-flash`. Config lives in `opencode.json` (inline agent blocks) + `.opencode/agents/*.md` (file-defined subagents, auto-discovered) + `.opencode/prompts/*.txt` (primary prompts). File-defined subagents (merge, quick-implementation, review, implementation, pr) are NOT duplicated in `opencode.json`.
- opencode `permission.task` uses last-match-wins: the `"*": "deny"` rule must stay FIRST, specific allows LAST. The orchestrator's allow-list is: explore, general, implementation, merge, quick-implementation, review, pr.
- `gh` 2.52.0 is installed at `/opt/homebrew/bin/gh`, authenticated as `Joe-Goodsell` with `repo` scope. Remote `origin` is `https://github.com/Joe-Goodsell/orange_yeoman.git`.
- Config loads once at startup and is not hot-reloaded. Any change to `opencode.json`, an agent file, or a prompt file requires an opencode restart to take effect.

## Key Decisions
- PR-raising is a DELEGATED role, not an orchestrator capability. A new `pr` subagent (`.opencode/agents/pr.md`) is the SOLE agent permitted to run `git push`, and only to raise a PR via `gh pr create`. The orchestrator stays read-only (bash denied) and never runs `git push` itself; it delegates push + `gh pr create` to `pr`. This mirrors the existing commit->`quick-implementation` and merge->`merge` delegation pattern.
- The `pr` subagent is separate from `merge`. `merge` lands a reviewed branch into `main` locally and deletes the branch; `pr` publishes a branch to GitHub for human review and keeps the branch. Opposite terminal actions stay as two agents.
- Hard boundary: PR approval (`gh pr review --approve` / `--request-changes`) and PR merge (`gh pr merge`) are FORBIDDEN everywhere — in `pr.md`, `orchestrator.txt`, and by extension. Raising a PR is allowed; approving and merging are human/review concerns. Linear diff tools `linear_merge_diff` and `linear_submit_diff_review` are also forbidden to the orchestrator.

## Trade-offs
- Chose delegated `pr` subagent over letting the orchestrator run `git push`/`gh pr create` directly. Cost: one extra subagent hop and one extra agent file. Benefit: preserves the read-only orchestrator design (consistent with the global orchestrator skill and the project's commit/merge delegation pattern); keeps push isolated to a single auditable agent with a narrow permitted-command list.
- Chose a dedicated `pr` agent over extending `merge`. Cost: one extra agent file. Benefit: single responsibility; avoids an accidental local merge when a remote PR was wanted; `merge`'s mandatory post-merge verification gate does not fit PR creation.
- Enforcement is prompt-level (the `pr` agent has `bash: allow` with an instruction-only forbid list), matching the existing `merge.md` trust model. No command-level allowlist. Accepted as consistent with the rest of the setup.

## Implementation Blockers / Open Follow-ups
- None. The `pr` capability is committed (`4771899`) but requires an opencode restart to activate (config loads once at startup).
- Push by the orchestrator itself, and push for any purpose other than raising a PR, remain manual user steps.

# Memory: PER-11 save-to-disk and watcher completion (fix)

## Project Findings
- The Rust pipeline is disk-state-driven: submit_block hashes the file from disk (source_hash_of) and the inline stale check re-parses the disk file for the command line (is_inline_result_stale). Any editor feature that dispatches must persist the buffer first; the ordering contract lives in Editor.svelte dispatchSlashCommand (save before submit_block, early return on save failure).
- watcher.rs classify_and_emit previously had todo!() arms for Create/Remove/Rename; the first such event panicked and killed the watcher thread permanently. The spawn_* helpers (content processing, removal, relink, rename fallback) existed but were never called. All arms are wired now.
- The watcher debouncer window is 400 ms; the editor autosave debounce is 700 ms.
- No JS test runner exists in the repo; frontend logic is verified by pnpm check only. Rust tests (213 passing) cover the disk-dependent behaviors.

## Key Decisions
- write_text_file uses a single fs::write, NOT temp+rename, because a rename would surface as Rename events in the watcher instead of Data-modify content events. It returns stable_hash(contents).
- Self-write suppression protocol (three sites that must stay in sync): write_text_file returns stable_hash of the written content; the watcher Content arm emits content_hash(path) (same FNV-1a over the same bytes); the store records selfWrites[path] = hash and pushChange skips the stale-marking pass on hash match. Hash-absent content events fall back to conservative stale-marking.
- contentOrigin ("editor" | "external") on the project store is the contract between save/load paths and the editor doc-sync effect: external loads replace the whole buffer; the editor's own saves never replace (the local buffer is newer; the effect re-marks dirty and reschedules autosave instead). Every openFileContent assignment must pair with an origin assignment in the same block.
- File switches flush the old file's unsaved edits fire-and-forget via saveFileContent(path, content), the shared serialized write core (saveChain), before cancelling the pending autosave. saveFileContent never touches open-file state.

## Assumptions
- mockLlm defaults to true with no config files present (verified: no global config exists at the app data dir), so the mock provider is active. The broken chain was disk state, not the provider.

## Trade-offs
- Accepted LOW residual risks from review: a rapid A-B-A file re-switch can briefly serve a stale buffer (flush is queued, not awaited); a failed old-file flush attributes the error to the current file's header; the saving indicator can flicker between chained saves.

## User Feedback
- The user reported PER-11 as incomplete: no feedback from slash commands. The user diagnosed that the editor never wrote to disk so the watcher never triggered, and directed: fix disk writes first, then investigate the mock call path. Both turned out to be the same root cause plus the watcher todo!() panics.

## Implementation Blockers / Open Follow-ups
- Inline /ignore is recognized but not dispatched by Rust (roadmap); the frontend shows a local mock card for it.
- AgentPane renders feedback for all files, not just the open file (pre-existing).
- openFile could skip the disk read when a flush for that path is still queued (stale-buffer follow-up).
- The running app must be restarted (pnpm tauri dev) to pick up the Rust rebuild.

# Memory: Linear ticket status rules (process)

## Project Findings
- The Linear team is `Personal0000` (issue key `PER`). Status set: Backlog, Todo, In Progress, In Review, Done, Canceled, Duplicate. There is no Blocked, Triage, or Ready to Merge status, so the rules map onto these seven only.
- Branch names in this repo already embed the ticket ID (for example `josephplgoodsell/per-43-refactor-...`). This is the link Linear and GitHub use to attach PR and commit activity to tickets.
- `memories/` is gitignored (local working state); AGENTS.md is the tracked, normative home for the rules.

## Key Decisions
- Added a `## Linear ticket status rules` section to AGENTS.md (after `## Workflow`). Event mapping: work starts -> In Progress; review agent runs or PR open -> In Review; changes requested -> In Progress; merged into `main` -> Done; PR closed unmerged -> In Progress (continue) or Canceled (abandon); duplicate -> Duplicate; parked -> Backlog; planned not started -> Todo.
- `Done` is set only when the branch lands on `main` (local `merge` agent or GitHub PR merge). A passed review alone does not close a ticket.
- Rules went into AGENTS.md, not outline.md: outline.md is project intent only and holds no process content.

## Assumptions
- Workspace GitHub integration automations may be off. The rules deliberately match Linear's default automation mapping (branch -> In Progress, PR open -> In Review, PR merged -> Done), so manual updates and any native automation agree.

## Trade-offs
- Kept the default status set instead of adding custom statuses (Blocked, Ready to Merge). Cost: no explicit blocked state. Benefit: small status set that matches Linear's recommended workflow and native automations.

## User Feedback
- The user set the goal: logical status rules tied to PR state, with the example PR open -> In Review, PR merged -> Done. Research (Linear docs and workflow guides) confirmed this mapping as standard practice.

## Implementation Blockers / Open Follow-ups
- None. New sessions read AGENTS.md at start; already-running sessions must restart to load the new rules.

---

# Memory: Untrack agent/opencode configuration (2026-09-20)

## Key Decisions
- Untracked `opencode.json`, `.opencode/`, and `AGENTS.md` from git with `git rm -r --cached`; added all three to the root `.gitignore`. Files stay on disk so local opencode tooling keeps working. User goal: agent tooling must not appear on GitHub.
- User rejected deletion; untrack-only was chosen.

## Trade-offs
- The files remain in 14 historical commits and on remote feature branches on GitHub. User accepted this; no history rewrite. No secrets were found in these files.
- Fresh clones contain no agent config and no `AGENTS.md`. `outline.md` is the only tracked source of truth for project intent. `README.md` no longer lists `AGENTS.md`.

## User Feedback
- "we shouldn't delete anything, but we should untrack — I don't want them to show up in github."