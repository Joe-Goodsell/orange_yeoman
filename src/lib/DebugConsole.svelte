<script lang="ts">
  // Bottom console showing debug events emitted by the Rust core. Error and
  // warning events always arrive; info and below arrive when the `debug`
  // config flag is on. Newest events appear at the bottom. A level filter row
  // controls which levels render.
  import { tick } from "svelte";
  import { debug } from "$lib/stores/debug.svelte";

  type Level = "error" | "warn" | "info" | "debug" | "trace";

  const LEVELS: Level[] = ["error", "warn", "info", "debug", "trace"];
  let activeLevels = $state<Set<Level>>(new Set(LEVELS));

  const visibleEvents = $derived(
    debug.events.filter((e) => activeLevels.has(e.level)),
  );

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

  function toggleLevel(level: Level) {
    const next = new Set(activeLevels);
    if (next.has(level)) {
      next.delete(level);
    } else {
      next.add(level);
    }
    activeLevels = next;
  }

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
  <div class="filters">
    {#each LEVELS as level (level)}
      <button
        class="lvl-btn {level} {activeLevels.has(level) ? 'on' : 'off'}"
        type="button"
        onclick={() => toggleLevel(level)}
      >
        {level}
      </button>
    {/each}
  </div>
  <div class="list" bind:this={container}>
    {#if visibleEvents.length === 0}
      <p class="empty">
        No debug events. Error and warning events always appear; info and below
        appear when "debug": true is set in .orange-yeoman.json.
      </p>
    {:else}
      {#each [...visibleEvents].reverse() as e (e.id)}
        <div class="row">
          <span class="time">{fmtTime(e.ts)}</span>
          <span class="badge {e.category}">{e.category}</span>
          <span class="lvl {e.level}">{e.level}</span>
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

  .filters {
    flex: 0 0 auto;
    display: flex;
    gap: 4px;
    padding: 4px 8px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
  }

  .lvl-btn {
    font-size: 10px;
    padding: 1px 7px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 3px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.45;
  }

  .lvl-btn.on {
    opacity: 1;
  }

  .lvl-btn.error.on {
    background: rgba(200, 70, 70, 0.3);
    color: rgba(240, 140, 140, 0.95);
  }

  .lvl-btn.warn.on {
    background: rgba(210, 140, 60, 0.3);
    color: rgba(240, 190, 120, 0.95);
  }

  .lvl-btn.info.on {
    background: rgba(90, 130, 200, 0.3);
    color: rgba(150, 180, 240, 0.95);
  }

  .lvl-btn.debug.on,
  .lvl-btn.trace.on {
    background: rgba(140, 140, 140, 0.25);
    color: rgba(190, 190, 190, 0.9);
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

  .lvl {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding: 0 5px;
    border-radius: 3px;
  }

  .lvl.error {
    background: rgba(200, 70, 70, 0.3);
    color: rgba(240, 140, 140, 0.95);
  }

  .lvl.warn {
    background: rgba(210, 140, 60, 0.3);
    color: rgba(240, 190, 120, 0.95);
  }

  .lvl.info {
    background: rgba(90, 130, 200, 0.3);
    color: rgba(150, 180, 240, 0.95);
  }

  .lvl.debug,
  .lvl.trace {
    background: rgba(140, 140, 140, 0.25);
    color: rgba(190, 190, 190, 0.9);
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