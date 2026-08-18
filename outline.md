# Orange Yeoman

## Project outline
An AI-powered tool that watches a repository of .md files and provides corrections, suggestions, and autonomous research capabilities as you write.

The central goal of this app is to provide real-time and async feedback on your notes, while providing both the persistence of a note-taking app while also allowing you to write uninterrupted without having to wait for a response as you would when chatting to an AI normally.

## Features
* Watches changes to a repository of .md files
* Provides fact-checking and logic-checking for claims that you make in your notes.
* Searches the web for sources that support or refute these claims.
* Provides autonomous research capabilities in an async manner. Ideally this would use discounted batch processing where possible.
* Simple text editor GUI that provides slash commands such as `/research`, `/fact-check`, `/ignore` (to not research a block of text), etc. and has visual feedback on what the agents are doing.
  * The editor itself is a bare-bones markdown editor (distraction-free writing surface in the main pane). Initially plaintext only — no markdown rendering yet.
  * A right-hand side pane displays details of what the AI agent(s) are currently doing (status, progress, results), so writing stays uninterrupted in the main pane while agent activity stays visible but unobtrusive.
  * The editor's folder/file tree mirrors the on-disk repository structure exactly; there is no app-owned note database or divergent layout.

## Details
* Initially we only need a macOS application.
* The app watches and interacts with the .md repository agnostically: it must not assume it is the sole editor of the files. Notes may be created or edited in other tools (e.g. Obsidian) and the app continues to watch the repo and process changes. The app is an observer/overlay over a plain `.md` folder, not the exclusive owner of the data.
* Non-blocking UX is a core requirement: the main editor pane is for writing; AI processing is decoupled and async, with results surfacing in the side pane rather than blocking input.

## Roadmap
* Vim keybindings in the editor (high priority).
* Syntax highlighting in the editor (high priority).
* Markdown rendering (render .md as formatted output, live preview).
* Better contextual slash commands dropdown (rank and filter commands by surrounding context).
* iOS app, and possibly Android/Windows/Web apps later on.
