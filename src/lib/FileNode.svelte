<script lang="ts">
  import { listDir } from "./tauri";
  import type { FileEntry } from "./types";
  import { project } from "./stores/project.svelte";
  import FileNode from "./FileNode.svelte";

  let {
    entry,
    level = 0,
  }: { entry: FileEntry; level?: number } = $props();

  let expanded = $state(false);
  let children = $state<FileEntry[]>([]);
  let loaded = $state(false);
  let loading = $state(false);

  let indent = $derived(level * 14);

  function isDotfile(name: string): boolean {
    return name.startsWith(".");
  }

  function shouldShow(e: FileEntry): boolean {
    if (isDotfile(e.name)) return false;
    if (e.is_dir) return true;
    return e.name.endsWith(".md");
  }

  async function handleClick() {
    if (entry.is_dir) {
      expanded = !expanded;
      if (expanded && !loaded) {
        loading = true;
        try {
          children = await listDir(entry.path);
          loaded = true;
        } catch (e) {
          project.error = String(e);
        } finally {
          loading = false;
        }
      }
    } else {
      project.openFile(entry.path);
    }
  }

  let isActive = $derived(
    !entry.is_dir && project.openFilePath === entry.path
  );
</script>

<div
  class="node"
  class:active={isActive}
  style="padding-left: {indent + 8}px"
  role="button"
  tabindex="0"
  onclick={handleClick}
  onkeydown={(e) => e.key === "Enter" && handleClick()}
>
  <span class="marker">
    {#if entry.is_dir}
      {#if loading}
        ...
      {:else if expanded}
        v
      {:else}
        >
      {/if}
    {:else}
      -
    {/if}
  </span>
  <span class="name">{entry.name}</span>
</div>

{#if entry.is_dir && expanded && loaded}
  {#each children.filter(shouldShow) as child (child.path)}
    <FileNode entry={child} level={level + 1} />
  {/each}
{/if}

<style>
  .node {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px 2px 0;
    font-size: 12.5px;
    line-height: 1.4;
    cursor: default;
    user-select: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border-radius: 3px;
  }

  .node:hover {
    background-color: rgba(128, 128, 128, 0.12);
  }

  .node.active {
    background-color: rgba(100, 150, 255, 0.18);
  }

  .node:focus-visible {
    outline: 1px solid rgba(100, 150, 255, 0.6);
    outline-offset: -1px;
  }

  .marker {
    flex: 0 0 16px;
    text-align: center;
    opacity: 0.5;
    font-size: 10px;
    font-family: ui-monospace, monospace;
  }

  .name {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>