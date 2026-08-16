import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChangeEvent,
  ConfigStatus,
  FileEntry,
  LlmRequest,
  LlmResponse,
} from "./types";

export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}

export async function listDir(dir: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("list_dir", { dir });
}

export async function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

export async function startWatcher(root: string): Promise<void> {
  return invoke<void>("start_watcher", { root });
}

export async function stopWatcher(): Promise<void> {
  return invoke<void>("stop_watcher");
}

// Payload emitted by Rust on the "watcher://change" channel.
export interface WatcherEvent {
  path: string;
  kind: "structure" | "content";
}

export function onWatcherChange(cb: (e: ChangeEvent) => void): Promise<UnlistenFn> {
  return listen<WatcherEvent>("watcher://change", (event) => {
    cb({ path: event.payload.path, kind: event.payload.kind, ts: Date.now() });
  });
}

// Config commands. The payloads are ConfigStatus, which never contain API key
// values, so these are safe to surface in the UI.

// root = null clears the project portion of the merged config in Rust.
export async function loadProjectConfig(root: string | null): Promise<ConfigStatus> {
  return invoke<ConfigStatus>("load_project_config", root ? { root } : {});
}

// Emitted by Rust when the watched repository's .orange-yeoman.json changes.
export function onConfigChanged(cb: (s: ConfigStatus) => void): Promise<UnlistenFn> {
  return listen<ConfigStatus>("config://changed", (event) => cb(event.payload));
}

// LLM completion through the provider boundary. Rust dispatches to the
// app-owned provider (currently the deterministic mock). No editor, store, or
// UI wiring yet; this is the thin invoke wrapper for the boundary.
export async function completeLlm(request: LlmRequest): Promise<LlmResponse> {
  return invoke<LlmResponse>("complete_llm", { request });
}