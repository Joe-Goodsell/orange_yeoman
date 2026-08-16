<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import Editor from "$lib/Editor.svelte";
  import AgentPane from "$lib/AgentPane.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import { project } from "$lib/stores/project.svelte";
  import {
    pickFolder,
    startWatcher,
    stopWatcher,
    onWatcherChange,
    onConfigChanged,
  } from "$lib/tauri";

  let unlistenWatcher: (() => void) | null = null;
  let unlistenConfig: (() => void) | null = null;
  let alive = true;
  // Track the root already started so the $effect skips the initial value
  // (onMount starts the initial watcher after the listener is registered).
  let prevRoot: string | null = project.projectRoot;

  onMount(async () => {
    const un = await onWatcherChange((e) => project.pushChange(e));
    const unCfg = await onConfigChanged((status) => project.applyConfigStatus(status));
    if (!alive) {
      un();
      unCfg();
      return;
    }
    unlistenWatcher = un;
    unlistenConfig = unCfg;
    if (project.projectRoot) {
      // Refresh the merged config status for the persisted root. The global
      // config was already loaded during Rust app setup.
      project.loadProjectConfig(project.projectRoot);
      // Start the watcher for the initial/persisted root only after the
      // listener is ready, so no early events are dropped.
      startWatcher(project.projectRoot).catch((e) => {
        project.error = String(e);
        project.watching = false;
      });
    }
  });

  onDestroy(() => {
    alive = false;
    unlistenWatcher?.();
    unlistenConfig?.();
    // Best-effort stop; ignore promise rejection
    stopWatcher().catch(() => {});
  });

  // Start/stop the watcher when the project root CHANGES after mount.
  $effect(() => {
    const root = project.projectRoot;
    if (root === prevRoot) return;
    prevRoot = root;
    if (root) {
      startWatcher(root).catch((e) => {
        project.error = String(e);
        project.watching = false;
      });
    } else {
      project.watching = false;
      stopWatcher().catch(() => {});
    }
  });

  async function openProjectPicker() {
    const folder = await pickFolder();
    if (folder) {
      project.selectRoot(folder);
    }
  }
</script>

{#if !project.projectRoot}
  <div class="hero">
    <button class="hero-btn" onclick={openProjectPicker}>Open a project</button>
    <p class="hero-text">Choose a folder of .md notes to get started.</p>
  </div>
{:else}
  <main class="app">
    <section class="tree-col">
      <FileTree onPickFolder={openProjectPicker} />
    </section>
    <section class="editor-pane">
      <Editor />
    </section>
    <aside class="agent-pane">
      <AgentPane />
    </aside>
  </main>
{/if}

<style>
  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    gap: 16px;
  }

  .hero-btn {
    font-size: 17px;
    font-weight: 500;
    padding: 10px 28px;
    border: 1px solid rgba(128, 128, 128, 0.35);
    border-radius: 8px;
    background: rgba(128, 128, 128, 0.08);
    color: inherit;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .hero-btn:hover {
    background: rgba(128, 128, 128, 0.18);
  }

  .hero-text {
    margin: 0;
    font-size: 13px;
    opacity: 0.5;
  }

  .app {
    display: flex;
    height: 100%;
    width: 100%;
  }

  .tree-col {
    flex: 0 0 240px;
    width: 240px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid rgba(128, 128, 128, 0.25);
  }

  .editor-pane {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid rgba(128, 128, 128, 0.25);
  }

  .agent-pane {
    flex: 0 0 320px;
    width: 320px;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>