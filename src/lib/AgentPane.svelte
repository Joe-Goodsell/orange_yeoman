<script lang="ts">
  // Right-hand side pane. Surfaces AI agent status/progress/results.
  // For the initial scaffold there are no active agents; this placeholder also
  // shows the live watcher change feed (structure/content events).
  import { project } from "./stores/project.svelte";

  function basename(p: string): string {
    const parts = p.split("/").filter(Boolean);
    return parts[parts.length - 1] || p;
  }

  function fmtTime(ts: number): string {
    const d = new Date(ts);
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }

  let rootName = $derived(
    project.projectRoot ? basename(project.projectRoot) : ""
  );
  let showEmpty = $derived(!project.watching && project.changeFeed.length === 0);
</script>

<aside class="pane">
  <header class="header">
    <h2>Agents</h2>
  </header>

  {#if project.configStatus}
    <section class="config">
      <h3 class="config-title">Config</h3>
      {#if project.configError}
        <p class="config-error">Config error: {project.configError}</p>
      {:else}
        <p class="config-ok">Config loaded</p>
      {/if}
      <p class="config-line">
        Small model: <span class="mono">{project.configStatus.small_model}</span>
      </p>
      <p class="config-line">
        Large model: <span class="mono">{project.configStatus.large_model}</span>
      </p>
      <p class="config-line">
        Providers:
        {#if project.configStatus.configured_providers.length > 0}
          {project.configStatus.configured_providers.join(", ")}
        {:else}
          none
        {/if}
      </p>
      <p class="config-meta">
        Global: {project.configStatus.global_loaded ? "loaded" : "absent"}
        &middot; Project:
        {project.configStatus.project_loaded ? "loaded" : "absent"}
      </p>
    </section>
  {/if}

  {#if showEmpty}
    <div class="empty">
      <p class="empty-title">No agents active</p>
      <p class="empty-sub">Agent activity will appear here.</p>
    </div>
  {:else}
    <div class="feed-scroll">
      {#if project.watching}
        <div class="status">Watching {rootName}</div>
      {/if}
      {#if project.changeFeed.length === 0}
        <p class="no-changes">No changes yet</p>
      {:else}
        <ul class="feed">
          {#each project.changeFeed as e (e.path + e.ts)}
            <li class="feed-item">
              <span class="feed-time">{fmtTime(e.ts)}</span>
              <span class="badge {e.kind}">
                {e.kind === "structure" ? "STRUCTURE" : "CONTENT"}
              </span>
              <span class="feed-path" title={e.path}>{basename(e.path)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <button class="clear-btn" onclick={() => (project.changeFeed = [])}>
      Clear
    </button>
  {/if}
</aside>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 12px 14px;
    background-color: rgba(128, 128, 128, 0.06);
  }

  .header {
    flex: 0 0 auto;
    padding-bottom: 8px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
  }

  .header h2 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.7;
  }

  .config {
    flex: 0 0 auto;
    margin-top: 10px;
    padding: 8px 10px;
    font-size: 12px;
    border: 1px solid rgba(128, 128, 128, 0.25);
    border-radius: 4px;
    background: rgba(128, 128, 128, 0.08);
  }

  .config-title {
    margin: 0 0 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.6;
  }

  .config-ok,
  .config-error {
    margin: 0 0 4px;
    font-weight: 500;
  }

  .config-ok {
    color: rgba(140, 210, 180, 0.95);
  }

  .config-error {
    color: rgba(220, 140, 140, 0.95);
    word-break: break-word;
  }

  .config-line {
    margin: 2px 0;
    opacity: 0.85;
  }

  .config-meta {
    margin: 4px 0 0;
    font-size: 11px;
    opacity: 0.5;
  }

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
  }

  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: 4px;
    opacity: 0.55;
  }

  .empty-title {
    margin: 0;
    font-size: 15px;
    font-weight: 500;
  }

  .empty-sub {
    margin: 0;
    font-size: 12px;
  }

  .feed-scroll {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    margin: 10px 0 8px;
  }

  .status {
    font-size: 12px;
    font-weight: 500;
    padding: 4px 6px;
    border: 1px solid rgba(128, 128, 128, 0.25);
    border-radius: 4px;
    background: rgba(128, 128, 128, 0.08);
    margin-bottom: 8px;
  }

  .no-changes {
    margin: 0;
    padding: 4px 6px;
    font-size: 12px;
    opacity: 0.45;
  }

  .feed {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .feed-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    padding: 4px 6px;
    border-radius: 4px;
    background: rgba(128, 128, 128, 0.07);
  }

  .feed-time {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
    opacity: 0.55;
  }

  .badge {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    padding: 1px 5px;
    border-radius: 3px;
  }

  .badge.structure {
    background: rgba(128, 128, 128, 0.22);
    color: rgba(200, 200, 200, 0.9);
  }

  .badge.content {
    background: rgba(90, 160, 130, 0.22);
    color: rgba(140, 210, 180, 0.95);
  }

  .feed-path {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.85;
  }

  .clear-btn {
    flex: 0 0 auto;
    align-self: flex-start;
    font-size: 11px;
    padding: 3px 10px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  .clear-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }
</style>
