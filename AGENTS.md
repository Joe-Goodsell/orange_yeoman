# AGENTS.md

Greenfield project — no code, build, test, lint, or typecheck tooling exists yet. `outline.md` is the only source of truth for project intent; read it before non-trivial work.

## Intended stack
Tauri (Rust core) + TypeScript / web frontend, macOS-first. No `Cargo.toml`, `package.json`, or `tauri.conf.json` exists yet, so the exact package manager and commands are not decided — do not fabricate or run build/test/lint commands. Update this section once tooling is initialized.

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
GitHub flow: short-lived feature branches off `main`, merged via PR. The repo is not a git repository as of this writing, so `git init` + an initial commit are prerequisites before this workflow applies.

## Agent orchestration (opencode)
Multi-agent setup lives in `opencode.json` + `.opencode/agents/`. Model split:
- **Planning/orchestration** (primary `plan`, also `build`): `opencode-go/glm-5.2`.
- **Execution/search/review subagents**: `opencode-go/deepseek-v4-flash` (cheap tier).
- **`small_model`** (titles/summaries): `opencode-go/deepseek-v4-flash`.

`plan` is the default primary — a read-mostly orchestrator that delegates all execution to subagents (`implementation`, `quick-implementation`, `explore`, `general`, `review`) and asks before editing directly. `build` is the Tab-switch alternative on the same GLM-5.2 model that can edit directly. `subagent_depth: 2` allows `implementation` to further delegate to `quick-implementation`. The global `orchestrator`/`implementation`/`review`/`quick-implementation`/`explore` skills (auto-loaded from `~/.agents/skills/`) define the workflows; the project `.opencode/agents/*.md` files define the registered agents that the orchestrator actually spawns. Do not run build/test/lint commands — none exist yet; config is loaded once at startup, so restart opencode after editing any of these files.