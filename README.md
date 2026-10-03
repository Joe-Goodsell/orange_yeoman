# Orange Yeoman

Orange Yeoman is an AI-powered macOS app that watches a repository of `.md` notes and runs corrections, fact-checking, and web-sourced research in the background. The current UI is a single-pane backend debug console that renders the backend's activity in real time as structured event cards. The app is an agnostic watcher/overlay over a plain `.md` folder, not the exclusive owner of the data — notes may be created or edited in other tools (e.g. Obsidian) and the app continues to watch the repo and process changes.

## Features

- Watches changes to a repository of `.md` files.
- Runs fact-checking, logic-checking, and web-sourced research in the background, preferring discounted batch processing where possible.
- Single-pane UI: a real-time backend debug console rendering the events Rust emits on the `backend://event` channel.
- Event cards update in place by id, so one card tracks a full lifecycle (queued, in flight, done, failed) and shows type, status, model, duration, cost, and expandable detail.
- The app is an overlay over a plain `.md` folder, not the exclusive owner of the data; there is no app-owned note database.

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
```

## Roadmap

- Reintroduce the markdown editor (the current build is console-only).
- Slash commands as the editor surface: `/research`, `/fact-check`, `/ignore`.
- Vim keybindings and syntax highlighting in the editor (high priority).
- Markdown rendering (render `.md` as formatted output, live preview).
- iOS app, and possibly Android/Windows/Web apps later on.