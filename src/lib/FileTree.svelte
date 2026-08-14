<script lang="ts">
  import { listDir } from "./tauri";
  import type { FileEntry } from "./types";
  import { project } from "./stores/project.svelte";
  import FileNode from "./FileNode.svelte";

  let { onPickFolder }: { onPickFolder: () => void } = $props();

  let rootChildren = $state<FileEntry[]>([]);
  let rootLoading = $state(false);
  let loaded = $state(false);

  function isDotfile(name: string): boolean {
    return name.startsWith(".");
  }

  function shouldShow(e: FileEntry): boolean {
    if (isDotfile(e.name)) return false;
    if (e.is_dir) return true;
    return e.name.endsWith(".md");
  }

  function basename(path: string): string {
    const parts = path.split("/").filter(Boolean);
    return parts[parts.length - 1] || path;
  }

  async function loadRoot() {
    if (!project.projectRoot) return;
    rootLoading = true;
    loaded = false;
    try {
      rootChildren = await listDir(project.projectRoot);
      loaded = true;
    } catch (e) {
      project.error = String(e);
    } finally {
      rootLoading = false;
    }
  }

  $effect(() => {
    if (project.projectRoot) loadRoot();
  });

  // Reload the root listing when a structure change event arrives. The
  // lastStructureV tracker prevents re-entry: loadRoot does not modify
  // structureVersion, so this effect only fires on genuine increments.
  let lastStructureV = 0;
  $effect(() => {
    const v = project.structureVersion;
    if (v !== lastStructureV && v > 0 && project.projectRoot) {
      lastStructureV = v;
      loadRoot();
    }
  });

  let rootName = $derived(
    project.projectRoot ? basename(project.projectRoot) : ""
  );
</script>

<div class="tree-pane">
  <header class="tree-header">
    <span class="root-name" title={project.projectRoot ?? ""}>
      {rootName}
    </span>
    <button class="open-btn" onclick={onPickFolder} title="Open another folder">
      Open...
    </button>
  </header>

  <div class="tree-body">
    {#if project.error}
      <div class="error-banner">
        <span class="error-text">{project.error}</span>
        <button class="error-dismiss" onclick={() => (project.error = null)}>x</button>
      </div>
    {/if}
    {#if rootLoading}
      <div class="loading">Loading...</div>
    {:else if loaded}
      {#each rootChildren.filter(shouldShow) as child (child.path)}
        <FileNode entry={child} level={0} />
      {/each}
    {/if}
  </div>
</div>

<style>
  .tree-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .tree-header {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
  }

  .root-name {
    font-size: 12px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.8;
  }

  .open-btn {
    flex: 0 0 auto;
    font-size: 11px;
    padding: 2px 8px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    white-space: nowrap;
  }

  .open-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }

  .tree-body {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 4px 0;
  }

  .loading {
    padding: 8px 12px;
    font-size: 12px;
    opacity: 0.5;
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 4px 6px;
    padding: 6px 8px;
    font-size: 11px;
    border: 1px solid rgba(200, 100, 100, 0.4);
    border-radius: 4px;
    background: rgba(200, 100, 100, 0.1);
  }

  .error-text {
    flex: 1 1 auto;
    word-break: break-word;
  }

  .error-dismiss {
    flex: 0 0 auto;
    font-size: 11px;
    padding: 0 4px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.6;
  }

  .error-dismiss:hover {
    opacity: 1;
  }
</style>