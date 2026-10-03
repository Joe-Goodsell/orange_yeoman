<script lang="ts">
  // The single-pane backend debug console: a header, a type filter row, and the
  // event card list. Stick-to-bottom scrolling follows the newest events unless
  // the user scrolls up to read history; an event burst never yanks the view.
  import { tick } from "svelte";
  import EventCard from "$lib/EventCard.svelte";
  import { feed } from "$lib/stores/feed.svelte";
  import { KNOWN_EVENT_TYPES } from "$lib/types";

  let container = $state<HTMLDivElement | undefined>(undefined);

  // Pinned stays true only while the list is at (or within 24px of) the
  // bottom. New events scroll the list down only when pinned.
  let pinned = $state(true);

  $effect(() => {
    const el = container;
    const events = feed.events;
    if (!el || events.length === 0 || !pinned) return;
    void tick().then(() => {
      el.scrollTop = el.scrollHeight;
    });
  });

  function onScroll() {
    const el = container;
    if (!el) return;
    pinned = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  const visibleEvents = $derived(feed.visible);
</script>

<div class="feed">
  <header class="head">
    <h2 class="title">Backend events</h2>
    {#if feed.dropped > 0}
      <span class="drop-note">
        {feed.dropped} dropped
      </span>
    {/if}
    <span class="count">{feed.events.length}</span>
    <button
      class="clear-btn"
      type="button"
      onclick={() => {
        feed.clear();
        pinned = true;
      }}
    >
      Clear
    </button>
  </header>

  <div class="filters">
    <button
      class="type-btn {feed.activeTypes.size === 0 ? "on" : ""}"
      type="button"
      onclick={() => feed.showAllTypes()}
    >
      All
    </button>
    {#each KNOWN_EVENT_TYPES as t (t)}
      <button
        class="type-btn {t} {feed.activeTypes.has(t) ? "on" : ""}"
        type="button"
        onclick={() => feed.toggleTypeFilter(t)}
      >
        {t} ({feed.typeCount(t)})
      </button>
    {/each}
  </div>

  <div class="list" bind:this={container} onscroll={onScroll}>
    {#if feed.events.length === 0}
      <p class="empty">Waiting for backend events...</p>
    {:else if visibleEvents.length === 0}
      <p class="empty">No events match the current filter.</p>
    {:else}
      {#each visibleEvents as e (e.id)}
        <EventCard event={e} />
      {/each}
    {/if}
  </div>
</div>

<style>
  .feed {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
  }

  .head {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
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

  .count {
    flex: 0 0 auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    opacity: 0.5;
  }

  .drop-note {
    flex: 0 0 auto;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 3px;
    border: 1px solid rgba(210, 140, 60, 0.35);
    background: rgba(210, 140, 60, 0.12);
    color: rgba(160, 110, 40, 0.95);
  }

  .clear-btn {
    flex: 0 0 auto;
    margin-left: auto;
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
    flex-wrap: wrap;
    gap: 4px;
    padding: 4px 8px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
  }

  .type-btn {
    font-size: 10px;
    padding: 1px 7px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 3px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.45;
  }

  .type-btn.on {
    opacity: 1;
    background: rgba(128, 128, 128, 0.18);
  }

  .type-btn.watcher_file.on {
    background: rgba(200, 160, 90, 0.3);
  }

  .type-btn.block_diff.on {
    background: rgba(140, 110, 180, 0.3);
  }

  .type-btn.store_update.on {
    background: rgba(80, 160, 180, 0.3);
  }

  .type-btn.llm_call.on {
    background: rgba(90, 160, 130, 0.3);
  }

  .type-btn.config_reload.on {
    background: rgba(100, 120, 150, 0.3);
  }

  .list {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 8px 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .empty {
    margin: 0;
    padding: 4px 2px;
    font-size: 11px;
    opacity: 0.45;
  }

  @media (prefers-color-scheme: dark) {
    .drop-note {
      color: rgba(240, 190, 120, 0.95);
    }
  }
</style>