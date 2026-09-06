# AGENTS.md

`outline.md` is the only source of truth for project intent; read it before non-trivial work.

## Output style
All agent responses to the user must use ASD-STE100 Simplified Technical English: short sentences, active voice, present tense procedures, approved-word vocabulary, one idea per sentence. Code and file contents are exempt; this applies to prose only.

## Stack
Tauri 2 (Rust core, `src-tauri/`) + SvelteKit / Svelte 5 + TypeScript / Vite (web frontend, root), macOS-first. Package manager: `pnpm` (v11+, configured via `pnpm-workspace.yaml` with `allowBuilds: esbuild: true`). Rust toolchain pinned to current stable via `rust-toolchain.toml`.

Canonical commands (run from repo root):
- `pnpm install` — install JS deps.
- `pnpm build` — production-build the frontend (Vite + adapter-static → `build/`).
- `pnpm check` — svelte-check + TypeScript diagnostics (run after non-trivial TS/Svelte edits).
- `cargo check --manifest-path src-tauri/Cargo.toml` — typecheck the Rust core.
- `pnpm tauri dev` — run the full app (opens a window; user-facing, not an automated gate).

Verification after non-trivial edits: run `pnpm check` and `cargo check --manifest-path src-tauri/Cargo.toml`. Do not run `pnpm tauri dev` as an automated gate (it opens a GUI window).

## Project
Orange Yeoman: an AI-powered macOS app that watches a repository of `.md` notes and gives real-time + async feedback (corrections, fact-checking, web-sourced research) as you write.

## Hard constraints (from outline.md)
- macOS-only for the initial build. iOS / Android / Windows / Web are roadmap, not current scope — don't optimize for them yet.
- Non-blocking UX is a core requirement: the user must write uninterrupted. Architecture must decouple writing from AI processing; feedback arrives async.
- Autonomous research should prefer discounted batch processing where possible.
- Slash commands are the canonical editor surface: `/research`, `/fact-check`, `/ignore` (more may follow). Build features against this command model.
- The app is an agnostic watcher/overlay over a plain `.md` folder, not the exclusive owner of the data. Files may be authored or changed by external tools (e.g. Obsidian); the app must watch the repo and react to disk state. The editor's folder tree must mirror on-disk structure exactly.
- UI layout: a bare-bones markdown editor in the main pane, with a right-hand side pane showing AI agent status/progress/results. Keep the main pane distraction-free. The editor is plaintext-only for the initial build (no markdown rendering); rendering and syntax highlighting are roadmap, not current scope.

## Workflow
GitHub flow: short-lived feature branches off `main`, merged via PR. The repo is initialized as a git repository on `main` (initial commit `c1a6ac9`).

## Linear ticket status rules

Keep Linear ticket status in sync with the real state of the work. Set the status from the workflow event, not from opinion. The team is `Personal0000` (issue key `PER`); its statuses are Backlog, Todo, In Progress, In Review, Done, Canceled, and Duplicate.

| Workflow event | Ticket status |
| --- | --- |
| Ticket is planned; work has not started | Todo |
| Work starts: feature branch created, or an implementation agent begins edits | In Progress |
| Review starts: review agent runs, or a PR is open | In Review |
| Review requests changes | In Progress (fixes are active work) |
| Branch merges into `main` (local merge agent or GitHub PR merge) | Done |
| PR closes without merge; the work continues | In Progress |
| PR closes without merge; the work is abandoned | Canceled |
| Ticket duplicates another ticket | Duplicate |
| Ticket is parked and not planned | Backlog |

- Set `Done` only after the branch merges into `main`. A passed review is not `Done`.
- Do not skip states. Do not move a ticket from `Todo` to `In Review` without edits in between.
- Put the ticket ID in the branch name and in the PR title (example: `per-42-fix-save`). This links git and GitHub activity to the ticket.
- These mappings match the Linear GitHub integration defaults. If workspace automations are on, let them move the ticket. Correct the status only when it is wrong.
- Do not leave a ticket in `In Progress` for many days without a PR. Move it to `Todo` with a comment, or set it to `Canceled`.

## Agent orchestration (opencode)
Multi-agent setup lives in `opencode.json` + `.opencode/agents/`. Primary agents (`plan`, `orchestrator`, `build`, `supervisor`) are model-agnostic and use the top-level `model`/`small_model` defaults or whatever model the user selects. Subagents (`explore`, `general`, `scout`, `implementation`, `quick-implementation`, `review`, `merge`, `pr`) pin the cheap model (`opencode-go/deepseek-v4-flash`) in their definitions. Roles and permissions (not models) differentiate the agents.

`orchestrator` is the default primary — plans, aligns with the user, then delegates execution/search/review to subagents (`implementation`, `quick-implementation`, `explore`, `general`, `review`, `merge`, `pr`). It is read-only: it never edits files or runs bash directly — including post-review fix-ups — so all behavior-changing edits and all verification go through subagents. The orchestrator also delegates a `git commit` to `quick-implementation` once a logical unit of work is reviewed. The `merge` subagent merges a reviewed worktree branch into `main` and removes the worktree and branch; it is the only agent permitted to run `git merge`. The `pr` subagent is the only agent permitted to run `git push`, and only to raise a PR; it must never approve or merge PRs. Push by the orchestrator itself, and push for any other purpose, stay manual user steps. `plan` is the read-only alternative — plans and researches via `explore`/`general`/`review` subagents only; cannot spawn `implementation` or `quick-implementation`; cannot edit or run bash directly. `build` is the edit-capable Tab-switch alternative. `supervisor` is a project-manager primary — it creates, updates, and tracks Linear tickets (issues, projects, milestones, status updates, comments) via the Linear MCP. It never writes code or runs bash; it may read local files for ticket context only. The orchestrator may read and update existing Linear objects (issue status, comments, status updates, milestones) but must never create new Linear tickets — ticket creation is the `supervisor` agent's role. `subagent_depth: 2` allows `implementation` to further delegate to `quick-implementation`. The global `orchestrator`/`implementation`/`review`/`quick-implementation`/`explore` skills (auto-loaded from `~/.agents/skills/`) define the workflows; the project `.opencode/agents/*.md` files define the registered agents that the orchestrator actually spawns. Config is loaded once at startup, so restart opencode after editing any of these files.
