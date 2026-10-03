import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BackendEvent, ConfigStatus } from "./types";

export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}

export async function startWatcher(root: string): Promise<void> {
  return invoke<void>("start_watcher", { root });
}

export async function stopWatcher(): Promise<void> {
  return invoke<void>("stop_watcher");
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

// Structured backend event cards emitted by Rust on the "backend://event"
// channel. Events with the same id update the same card in place.
export function onBackendEvent(cb: (e: BackendEvent) => void): Promise<UnlistenFn> {
  return listen<BackendEvent>("backend://event", (event) => cb(event.payload));
}