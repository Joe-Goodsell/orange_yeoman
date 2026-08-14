import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ChangeEvent, FileEntry } from "./types";

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
