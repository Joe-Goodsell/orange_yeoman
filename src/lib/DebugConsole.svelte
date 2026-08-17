<script lang="ts">
  // Bottom console showing raw debug events emitted by the Rust core when the
  // `debug` config flag is on. Newest events appear at the bottom.
  import { tick } from "svelte";
  import { debug } from "$lib/stores/debug.svelte";

  let container = $state<HTMLDivElement | undefined>(undefined);

  // Pin the console to the newest event: when the event count changes, wait
  // for the DOM update and scroll the list to the bottom.
  $effect(() => {
    const el = container;
    if (!el || debug.events.length === 0) return;
    void tick().then(() => {
      el.scrollTop = el.scrollHeight;
    });
  });

  function fmtTime(ts: number): string {
    const d = new Date(ts);
    const pad = (n: number) => n.toString().padStart(2, "0");
    const ms = d.getMilliseconds().toString().padStart(3, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${ms}`;
  }
</script>

<div class="debug-pane">
  <header class="head">
    <h2 class="title">Debug console</h2>
    <button class="clear-btn" type="button" onclick={() => debug.clear()}>
      Clear
    </button>
  </header>
  <div class="list" bind:this={container}>
    {#if debug.events.length === 0}
      <p class="empty">
        No debug events. Set "debug": true in .orange-yeoman.json.
      </p>
    {:else}
      {#each [...debug.events].reverse() as e (e.id)}
        <div class="row">
          <span class="time">{fmtTime(e.ts)}</span>
          <span class="badge {e.category}">{e.category}</span>
          <span class="msg" title={e.message}>{e.message}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .debug-pane {
    flex: 0 0 220px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid rgba(128, 128, 128, 0.25);
    background-color: rgba(128, 128, 128, 0.04);
  }

  .head {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
  }

  .title {
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.6;
  }

  .clear-btn {
    font-size: 10px;
    padding: 2px 8px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.7;
  }

  .clear-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }

  .list {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .empty {
    margin: 0;
    padding: 4px 2px;
    font-size: 11px;
    opacity: 0.45;
  }

  .row {
    flex: 0 0 auto;
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    padding: 2px 4px;
    border-radius: 3px;
    background: rgba(128, 128, 128, 0.07);
  }

  .time {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
    opacity: 0.55;
  }

  .badge {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    padding: 0 5px;
    border-radius: 3px;
  }

  .badge.slash_command {
    background: rgba(120, 150, 200, 0.25);
    color: rgba(170, 190, 230, 0.95);
  }

  .badge.llm_call {
    background: rgba(90, 160, 130, 0.22);
    color: rgba(140, 210, 180, 0.95);
  }

  .badge.watcher {
    background: rgba(200, 160, 90, 0.22);
    color: rgba(220, 190, 130, 0.95);
  }

  .msg {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.9;
  }
</style>