# Detached Backend Feasibility

**Status:** Research deliverable
**Date:** 2026-08-16
**Scope:** Reusing the Orange Yeoman backend from the Tauri app, a VS Code extension, an Obsidian plugin, and future clients.

## Executive Summary

Detaching the backend is feasible. The current Rust code is small and has a clear extraction path. The main work is architectural, not algorithmic.

The recommended design has three layers:

1. A transport-free Rust core crate containing file watching, Markdown processing, AI orchestration, persistence, configuration, and result validation.
2. A small service process that exposes the core through a versioned protocol.
3. Thin host adapters for Tauri, VS Code, Obsidian, and later clients.

Use two transport modes:

- **Loopback HTTP as the common denominator.** Obsidian can use its `requestUrl` API. VS Code and Tauri can also use HTTP.
- **Stdio JSON-RPC as an optional local mode.** VS Code and Tauri can launch the backend directly. This avoids a listening port and simplifies local security.

Use MCP-style JSON-RPC semantics for backend operations if interoperability with AI clients is valuable. Do not make MCP the only editor protocol. VS Code-native diagnostics and editor features fit LSP better, while Obsidian needs its own plugin API adapter.

The first Tauri release does not need a separate backend process. Extract the core crate first and keep an in-process Tauri adapter. Add the service process when the first external client needs it.

## Feasibility Assessment

| Area | Assessment | Reason |
| --- | --- | --- |
| Extract Rust core from Tauri | High | Most current logic uses standard Rust types and crates |
| Use core from Tauri | High | Tauri can call a Rust crate directly |
| Use backend from VS Code | High | VS Code supports native child processes, LSP clients, and HTTP clients |
| Use backend from Obsidian | Medium to high | HTTP is practical; native process and binary distribution add constraints |
| Share one live daemon across hosts | Medium | Lifecycle, authentication, vault ownership, and multi-client state need design |
| Reuse the current frontend unchanged | Low | The current frontend calls Tauri `invoke` and event APIs directly |
| Preserve macOS-first scope | High | A macOS binary and macOS host adapters are sufficient for the initial build |

The most important decision is to detach the **domain core**, not to immediately detach the **process**. A library boundary gives most of the long-term value with less operational risk.

## Current Architecture

The current backend is in `src-tauri/src/lib.rs`. It contains:

- Configuration file parsing and merging.
- API key storage in Rust memory.
- Directory listing and text-file reads.
- A recursive `notify` watcher with debounce.
- Tauri event emission.
- Tauri command registration and application startup.

The current frontend uses `src/lib/tauri.ts` for all backend calls and event subscriptions. This is the main frontend coupling point.

The current Rust coupling points are narrow:

- `#[tauri::command]` attributes.
- `tauri::State` parameters.
- `tauri::AppHandle` in the watcher callback.
- `app.emit(...)` for watcher and configuration events.
- The Tauri builder and plugin setup.

The configuration merge functions, file operations, event classification, and watcher dependencies can move into a core crate with little change. The watcher currently mixes event classification with event delivery. Split those operations first.

## Target Architecture

```text
                         +----------------------+
                         | orange-yeoman-core   |
                         |                      |
                         | config               |
                         | markdown/extraction  |
                         | watcher              |
                         | AI pipeline          |
                         | batch/poller         |
                         | persistence          |
                         | result validation   |
                         +----------+-----------+
                                    |
                  +-----------------+------------------+
                  |                                    |
        +---------v---------+                +---------v---------+
        | Tauri adapter     |                | Backend service   |
        | in-process first  |                | stdio / HTTP      |
        +---------+---------+                +---------+---------+
                  |                                    |
          Svelte frontend                 +------------+------------+
                                           |                         |
                                    VS Code adapter          Obsidian adapter
```

### Core crate

The core crate must not import Tauri, VS Code, Obsidian, Electron, or frontend code. It should expose typed Rust services and events.

Suggested modules:

- `config`: global and repository configuration, secret access, safe status views.
- `fs`: directory listing, Markdown reads, path validation.
- `markdown`: block parsing, offsets, heading context, slash commands.
- `extraction`: local signals, candidate scoring, structured model extraction.
- `watcher`: recursive watch, debounce, event classification, active repository state.
- `pipeline`: claim processing, research tasks, batch submission, polling, stale checks.
- `providers`: provider trait and provider-specific API clients.
- `persistence`: SQLite task, batch, claim, and result stores.
- `events`: typed core event definitions and subscriptions.
- `service`: request handlers independent of the wire transport.

The core should own task state. A client disconnect must not cancel a queued fact-check unless the client explicitly cancels it.

### Tauri adapter

Keep the current Tauri app as an adapter at first:

- Tauri commands call core service methods.
- A core event subscription maps typed events to Tauri event names.
- Tauri plugins remain in the adapter.
- API keys remain in the core and never enter frontend event payloads.

This preserves the existing app while enforcing the boundary at the Cargo dependency level.

### Service process

Add a separate binary when an external host needs the backend. It should:

- Resolve or receive a repository root.
- Load configuration.
- Own the watcher and async workers.
- Expose a versioned request and event protocol.
- Persist tasks and results outside the client process.
- Shut down cleanly when its owner disconnects, unless configured as a shared daemon.

Avoid starting with a permanently running system daemon. A per-client or per-repository process is easier to secure and avoids unclear ownership when Tauri, VS Code, and Obsidian watch the same folder at the same time.

## Protocol Options

### Rust library

**Strengths:** Fast, simple, no IPC, no listening port, easy secret handling.

**Limits:** Only Rust hosts can use it directly. TypeScript plugins cannot embed a Rust crate. Obsidian cannot use it as a normal plugin dependency.

**Use:** The Tauri app and tests. This is the required foundation, but it is not the universal integration mechanism.

### Stdio JSON-RPC

The client launches the backend and exchanges newline-delimited JSON-RPC messages over standard input and output.

**Strengths:** No network port, simple lifecycle, good isolation, and a natural fit for VS Code and Tauri sidecars. The client owns the child process.

**Limits:** The host must be able to launch a binary. This is not a common denominator for Obsidian plugins. It also requires strict stdout discipline and process packaging for each target architecture.

**Use:** VS Code and Tauri where direct child-process management is available.

### Loopback HTTP

The backend listens on `127.0.0.1` and clients use HTTP requests. Server-Sent Events or polling can deliver asynchronous events.

**Strengths:** Works from TypeScript clients, including Obsidian. Supports multiple clients. It is easy to inspect during development. It works across process boundaries.

**Limits:** It creates a local network attack surface. It needs authentication, origin checks, port discovery, lifecycle rules, and clear error handling.

**Use:** The shared protocol for all external clients.

### Unix domain socket

The backend listens on a Unix socket with filesystem permissions and macOS peer credentials.

**Strengths:** Stronger local access control than a TCP port. It avoids port collisions and network exposure.

**Limits:** Obsidian's normal HTTP API cannot address a Unix socket. It needs a separate client implementation in VS Code and Tauri. Socket path length is limited on macOS.

**Use:** An optional hardened transport for native clients, not the universal transport.

### LSP

LSP is a good protocol for editor-native features. VS Code has a first-class language client and can launch a native language server.

Useful LSP mappings include:

- Fact-check findings as diagnostics.
- Research explanations as hover content.
- Suggested corrections as code actions.
- Slash command completion as completion items.
- Document changes through `didOpen`, `didChange`, and `didClose`.

LSP does not cover the full product. Batch status, research history, cost information, and a rich agent pane need custom notifications or another API. Obsidian does not provide an LSP client.

**Use:** Add an LSP adapter for VS Code editor features later. Do not make the core itself an LSP server.

### MCP

MCP uses JSON-RPC and defines stdio and Streamable HTTP transports. Its tools and resources map reasonably well to Orange Yeoman operations:

- `fact_check` tool.
- `research` tool.
- `ignore_block` tool.
- Research result and task resources.

However, MCP is designed for model and tool interoperability, not as a complete editor UI protocol. It does not replace the VS Code extension API or Obsidian plugin API.

**Use:** Adopt MCP-compatible semantics if useful. Keep editor-specific operations and notifications in a versioned Orange Yeoman API namespace.

## Recommended Protocol Shape

Use a small, versioned service API. Do not expose raw filesystem access to clients.

Example requests:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"client":"obsidian","version":"0.1.0"}}
{"jsonrpc":"2.0","id":2,"method":"workspace/open","params":{"root":"/Users/example/Notes"}}
{"jsonrpc":"2.0","id":3,"method":"task/fact-check","params":{"path":"history.md","start":120,"end":168}}
{"jsonrpc":"2.0","id":4,"method":"task/research","params":{"path":"cavalry.md","selection":"...","goal":"find additional battles"}}
{"jsonrpc":"2.0","id":5,"method":"task/cancel","params":{"task_id":"task_123"}}
```

Example notifications:

```json
{"jsonrpc":"2.0","method":"event/file-changed","params":{"path":"history.md","kind":"content","revision":"sha256:..."}}
{"jsonrpc":"2.0","method":"event/task-updated","params":{"task_id":"task_123","status":"researching"}}
{"jsonrpc":"2.0","method":"event/result-ready","params":{"task_id":"task_123","result_id":"result_456"}}
```

The protocol should include:

- `protocol_version`.
- Client identity and capabilities.
- Repository root identity.
- Stable task and result IDs.
- Source revision or content hash.
- Cancellation.
- Pagination for task history and results.
- Reconnection and event replay from a cursor.
- Explicit stale and orphaned states.

The backend should return result data by ID. Events should announce state changes. This prevents large result payloads from being lost when a UI is closed or restarted.

## Client Integration

### Tauri

#### Initial approach

Embed the core in the Tauri Rust process. Keep thin commands and event forwarding. This has the lowest risk and keeps the macOS application simple.

#### Later sidecar approach

Tauri 2 supports bundling an external binary as a sidecar. The app can launch it and communicate over stdio or localhost HTTP. A sidecar needs architecture-specific binaries, capability permissions, and a signed distribution path.

Use a sidecar only when the same backend must remain alive beyond the Tauri window or when the service must be shared with other clients.

### VS Code

VS Code extensions run in an extension host process. A TypeScript extension can launch a native backend and manage its lifecycle.

Recommended integration:

- Start with a thin TypeScript extension client.
- Launch the backend in stdio mode for single-client use.
- Use `vscode-languageclient` if exposing LSP diagnostics, hover, or code actions.
- Use custom JSON-RPC notifications for task progress and research results.
- Use a `WebviewView` in the secondary sidebar for the agent pane.
- Use commands for `/research`, `/fact-check`, and `/ignore`.
- Use a completion provider triggered by `/` if inline slash suggestions are needed.
- Activate on Markdown files or workspace presence, not on every VS Code startup.

VS Code can also use the HTTP service when a shared daemon is needed. A native extension cannot run in a web extension host, so this integration is macOS and desktop oriented at first.

Avoid a custom editor for the first extension. The built-in Markdown editor plus diagnostics and a sidebar gives the required feedback without replacing the editor.

### Obsidian

Obsidian plugins can access vault and workspace events, register commands, provide editor suggestions, and create an `ItemView` in the right sidebar.

Recommended integration:

- Use the Obsidian vault as the visible source of truth.
- Send file and editor events to the backend, or let the backend watch the vault directly.
- Use the HTTP service over `requestUrl`.
- Use a per-install or per-vault bearer token.
- Use polling first for task updates. Add SSE only after testing it in the target Obsidian desktop versions.
- Register `/research`, `/fact-check`, and `/ignore` as Obsidian commands.
- Use `registerEditorSuggest` for inline slash suggestions if needed.
- Render status and results in an `ItemView` in the right sidebar.
- Use `Vault.process()` for plugin-owned writes and coordinate all writes with the backend.

Set `isDesktopOnly` if the plugin uses Node or Electron APIs to launch a process. Prefer HTTP so the plugin can avoid native Node ABI and binary installation problems. The plugin must disclose network use and any files or services accessed outside the vault.

### Future clients

A command-line client can use stdio or HTTP. A Neovim plugin can use stdio, HTTP, or a Unix socket. A web client can use authenticated HTTPS to a remote service, but it must not access an unauthenticated local endpoint.

The core API should not assume that every client has a cursor, selection, sidebar, or live editor buffer. Those are adapter-level features. The core should accept optional source ranges and should process on-disk revisions correctly when a client has no live buffer.

## Service Ownership and Multi-Client Behavior

The repository may be open in Tauri, VS Code, and Obsidian at the same time. The backend must define ownership rules.

Recommended rules:

- One logical workspace identity per repository root.
- One watcher per backend process. Avoid multiple processes writing the same database without coordination.
- Multiple read clients are allowed.
- Task submission is idempotent by task key and source revision.
- The backend never overwrites user text automatically.
- A client can disconnect without stopping research.
- A result is visible to every authorized client after it passes validation.
- The backend uses content hashes rather than line numbers as durable identities.
- External edits always win. A result for changed text becomes stale.

For the first version, prefer one backend process per client. A shared daemon can follow after reconnection and authorization behavior is tested.

## Security Model

The backend reads private notes and holds AI credentials. Treat every client and every note as untrusted at the protocol boundary.

### HTTP requirements

- Bind only to `127.0.0.1`.
- Use a random bearer token with sufficient entropy.
- Require the token on every request except a tightly scoped discovery mechanism.
- Validate `Origin` and `Host` headers. CORS alone is not enough.
- Use a random port or an authenticated port-discovery file with restrictive permissions.
- Do not expose API keys, raw provider errors, or full private note contents in status events.
- Scope every request to an authorized repository root.
- Rate-limit requests and bound input sizes.
- Validate all result payloads before display or file operations.
- Never allow a client request to execute an arbitrary shell command or read an arbitrary path.

The main local threat is a malicious webpage or process attempting to call a localhost service. Loopback binding alone does not solve this.

### Stdio requirements

- Write protocol messages only to stdout.
- Write logs only to stderr.
- Enforce message size limits.
- Validate method names and parameters.
- Shut down on parent process exit where appropriate.

### Secrets

Keep provider keys in the core process. Tauri already follows this rule. VS Code and Obsidian should store only a reference or client token in their own settings. Never put provider keys in a vault, VSIX, plugin bundle, or webview.

## Distribution Feasibility

### Tauri

Tauri can bundle a sidecar binary. macOS distribution requires architecture handling, signing, and likely notarization. This is manageable for the macOS-first scope.

### VS Code

The extension package must contain or obtain a compatible native binary. A VSIX can include the macOS binary. Remote and web extension hosts require separate plans and are out of current scope.

### Obsidian

An Obsidian plugin can be a thin TypeScript client, which is easier to distribute. Bundling and launching a Rust binary creates larger releases, desktop-only constraints, update coordination, and review risk. An already running local service or the Tauri app avoids some of those issues, but requires discovery and authentication UX.

The simplest distribution model is:

1. Install the Orange Yeoman macOS app, which owns or starts the local service.
2. Install a thin VS Code or Obsidian client.
3. The client discovers the local service and asks the user to authorize the repository.

This model creates a product dependency on the desktop app. A self-contained client model is more convenient but duplicates binary packaging.

## Incremental Migration Plan

### Phase 1: Extract without behavior change

- Create a Rust core crate with no Tauri dependency.
- Move configuration, file operations, Markdown event classification, and watcher logic.
- Replace Tauri state wrappers with core-owned service state.
- Define typed core events.
- Keep Tauri commands and event names unchanged.
- Add core unit tests.

### Phase 2: Add a frontend client boundary

- Replace direct `tauri.ts` imports with a `BackendClient` interface.
- Keep the existing Tauri implementation.
- Preserve the current payload types where possible.
- Add task, result, and event types before adding another host.

### Phase 3: Add the service binary

- Add a `orange-yeoman-service` binary that uses the core crate.
- Implement stdio JSON-RPC first because it is easy to test and secure.
- Add HTTP only when an external TypeScript client needs it.
- Add request authentication, version negotiation, and event replay before exposing it.

### Phase 4: Add VS Code support

- Build a thin TypeScript extension.
- Start with commands, diagnostics, and a sidebar.
- Use stdio for the backend process.
- Add LSP only for editor-native diagnostics, hover, and code actions.

### Phase 5: Add Obsidian support

- Build a thin plugin using `requestUrl`.
- Add authenticated HTTP discovery and repository authorization.
- Use vault and editor events as hints while the backend remains the durable watcher.
- Add the right-sidebar `ItemView` and command integration.

### Phase 6: Shared daemon

- Support multiple authorized clients.
- Add workspace leases or a single-instance lock per repository.
- Add event cursors and reconnect replay.
- Add clear start, stop, and ownership UX.

## Main Risks

### Duplicate watchers

Tauri, Obsidian, and the backend can all observe the same files. Duplicate observation can create duplicate tasks. Use stable event IDs, source hashes, and idempotent task keys. Prefer the backend watcher as the only task-producing watcher.

### Unsaved editor content

VS Code and Obsidian can contain edits that are not yet on disk. The backend watcher sees only saved content. Host adapters can send an optional in-memory snapshot with a client revision. The backend must mark this snapshot as ephemeral and must not persist it as the repository state until the file is saved.

### Cursor-aware processing

The active-line grace period belongs in each editor adapter because only the host knows the cursor. The core should accept a committed range or explicit trigger. On-disk watcher processing still uses debounce and has no active cursor.

### Competing writes

The backend should return suggestions and proposed edits. It should not silently edit notes. If a client applies an edit, use the host's edit API where available and include the source revision used to create the edit.

### Long-running jobs

Jobs must survive client reloads and reconnects. Store task state and results in the core-owned database. Use event notifications only as a cache invalidation and progress channel.

### Protocol drift

Version all request and event schemas. Keep provider-specific fields behind the core service. Add compatibility tests for each client adapter.

## Conclusion

The project can support VS Code, Obsidian, and other clients without abandoning Tauri. The correct first step is to extract a transport-free Rust core and preserve the current Tauri app as one adapter.

Use stdio for native clients where process ownership is simple. Use authenticated loopback HTTP for the universal TypeScript integration, especially Obsidian. Add LSP as a VS Code-specific adapter for editor-native features. Use MCP-compatible JSON-RPC semantics where tool interoperability helps, but do not force all editor behavior into MCP.

This approach preserves the macOS-first scope, keeps the writing experience asynchronous, protects the filesystem and API keys, and leaves room for other clients without coupling the research pipeline to any one editor.

## Sources

- Current project outline: `../outline.md`
- Current backend: `../src-tauri/src/lib.rs`
- Current frontend bridge: `../src/lib/tauri.ts`
- VS Code Language Server Extension Guide: https://code.visualstudio.com/api/language-extensions/language-server-extension-guide
- VS Code Extension Host: https://code.visualstudio.com/api/advanced-topics/extension-host
- VS Code Webview API: https://code.visualstudio.com/api/extension-guides/webview
- VS Code Webview Views: https://code.visualstudio.com/api/references/vscode-api#WebviewView
- VS Code MCP guide: https://code.visualstudio.com/api/extension-guides/ai/mcp
- Language Server Protocol overview: https://microsoft.github.io/language-server-protocol/overviews/lsp/overview/
- Obsidian plugin anatomy: https://docs.obsidian.md/Plugins/Getting+started/Anatomy+of+a+plugin
- Obsidian plugin lifecycle: https://docs.obsidian.md/Plugins/Guides/Manage+plugin+lifecycle
- Obsidian events: https://docs.obsidian.md/Plugins/Events
- Obsidian Vault API: https://docs.obsidian.md/Reference/TypeScript+API/Vault
- Obsidian `requestUrl`: https://docs.obsidian.md/Reference/TypeScript+API/requestUrl
- Obsidian views: https://docs.obsidian.md/Plugins/User+interface/Views
- Obsidian plugin guidelines: https://docs.obsidian.md/Plugins/Releasing/Plugin+guidelines
- Obsidian developer policies: https://docs.obsidian.md/Community+directory/Developer+policies
- JSON-RPC 2.0 specification: https://www.jsonrpc.org/specification
- MCP transports: https://modelcontextprotocol.io/specification/2025-06-18/basic/transports
- Tauri sidecars: https://v2.tauri.app/develop/sidecar/
- macOS Unix sockets: https://keith.github.io/xcode-man-pages/unix.4.html
