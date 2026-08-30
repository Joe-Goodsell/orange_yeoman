# Orange Yeoman

Orange Yeoman is an AI-powered macOS app that watches a repository of `.md` notes and gives real-time and async feedback — corrections, fact-checking, and web-sourced research — as you write. The central goal is feedback on your notes while you write uninterrupted: AI processing is decoupled and async, with results surfacing in a side pane rather than blocking input. The app is an agnostic watcher/overlay over a plain `.md` folder, not the exclusive owner of the data — notes may be created or edited in other tools (e.g. Obsidian) and the app continues to watch the repo and process changes.

## Features

- Watches changes to a repository of `.md` files.
- Provides fact-checking and logic-checking for claims in your notes.
- Searches the web for sources that support or refute those claims.
- Runs autonomous research in an async manner, preferring discounted batch processing where possible.
- Slash commands are the canonical editor surface: `/research`, `/fact-check`, `/ignore` (to skip a block of text), with more to follow.
- Bare-bones plaintext markdown editor in the main pane — a distraction-free writing surface, no markdown rendering yet.
- Right-hand side pane shows what the AI agents are doing (status, progress, results) while writing stays uninterrupted.
- Folder/file tree mirrors the on-disk repository structure exactly; there is no app-owned note database.

## Stack

- Tauri 2 (Rust core in `src-tauri/`) + SvelteKit / Svelte 5 + TypeScript / Vite (web frontend at the repo root), macOS-first.
- Package manager: `pnpm` (v11+).
- Rust toolchain pinned to current stable via `rust-toolchain.toml`.

## Requirements

- macOS.
- Node + pnpm v11+.
- Stable Rust (pinned via `rust-toolchain.toml`).

## Getting started

Canonical commands (run from repo root):

| Command | Description |
| --- | --- |
| `pnpm install` | install JS deps. |
| `pnpm build` | production-build the frontend (Vite + adapter-static → `build/`). |
| `pnpm check` | svelte-check + TypeScript diagnostics. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | typecheck the Rust core. |
| `pnpm tauri dev` | run the full app (opens a window). |

After non-trivial edits, run `pnpm check` and `cargo check --manifest-path src-tauri/Cargo.toml` as verification.

## Configuration

See [docs/configuration.md](docs/configuration.md) for the JSON configuration system (global and per-repository files, precedence, and the API-key warning). Use `.orange-yeoman.json.example` as the template. The real config file `.orange-yeoman.json` is gitignored and holds secrets (API keys) — never commit it.

## Project layout

```
src/          SvelteKit frontend
src-tauri/    Rust core + Tauri config
docs/         Documentation
research/     Research notes
outline.md    Project intent / source of truth
AGENTS.md     Agent + stack rules
```

## Roadmap

- Vim keybindings in the editor (high priority).
- Syntax highlighting in the editor (high priority).
- Markdown rendering (render `.md` as formatted output, live preview).
- Better contextual slash commands dropdown (rank and filter commands by surrounding context).
- iOS app, and possibly Android/Windows/Web apps later on.