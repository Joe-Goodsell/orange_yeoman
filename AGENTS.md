# AGENTS.md

`outline.md` is the only source of truth for project intent; read it before non-trivial work.

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

## Agent orchestration (opencode)
Multi-agent setup lives in `opencode.json` + `.opencode/agents/`. Model split:
- **Planning/orchestration** (primary `plan`, also `build`): `opencode-go/glm-5.2`.
- **Execution/search/review subagents**: `opencode-go/deepseek-v4-flash` (cheap tier).
- **`small_model`** (titles/summaries): `opencode-go/deepseek-v4-flash`.

`plan` is the default primary — a read-mostly orchestrator that delegates all execution to subagents (`implementation`, `quick-implementation`, `explore`, `general`, `review`) and asks before editing directly. `build` is the Tab-switch alternative on the same GLM-5.2 model that can edit directly. `subagent_depth: 2` allows `implementation` to further delegate to `quick-implementation`. The global `orchestrator`/`implementation`/`review`/`quick-implementation`/`explore` skills (auto-loaded from `~/.agents/skills/`) define the workflows; the project `.opencode/agents/*.md` files define the registered agents that the orchestrator actually spawns. Config is loaded once at startup, so restart opencode after editing any of these files.