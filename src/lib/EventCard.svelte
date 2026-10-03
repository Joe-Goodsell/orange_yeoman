<script lang="ts">
  // One backend event card. The collapsed row shows the essentials on one
  // line; clicking it expands a full field dump (definition list, block ref,
  // and free-form detail). Expansion state lives in the feed store so a list
  // re-render keeps which cards are open.
  import { feed } from "$lib/stores/feed.svelte";
  import type { BackendEvent } from "$lib/types";

  let { event }: { event: BackendEvent } = $props();

  const expanded = $derived(feed.expandedIds.has(event.id));

  function fmtTime(ts: number): string {
    const d = new Date(ts);
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }

  function fmtStamp(ts: number): string {
    const d = new Date(ts);
    const pad = (n: number) => n.toString().padStart(2, "0");
    const pad3 = (n: number) => n.toString().padStart(3, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad3(d.getMilliseconds())}`;
  }

  // Under 1000 ms -> "123ms", otherwise seconds with one decimal.
  function fmtDuration(ms: number): string {
    return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
  }

  function fmtCost(v: number): string {
    return `$${v.toFixed(4).replace(/0+$/, "").replace(/\.$/, "")}`;
  }

  // finalCostUsd wins when present, else estimatedCostUsd, else a dash.
  const cost = $derived(
    event.finalCostUsd !== null
      ? fmtCost(event.finalCostUsd)
      : event.estimatedCostUsd !== null
        ? fmtCost(event.estimatedCostUsd)
        : "\u2014",
  );
</script>

<div class="card">
  <div
    class="row {expanded ? "open" : ""}"
    role="button"
    tabindex="0"
    onclick={() => feed.toggleExpanded(event.id)}
    onkeydown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        feed.toggleExpanded(event.id);
      }
    }}
  >
    <span class="badge type {event.eventType}">{event.eventType}</span>
    <span class="badge status {event.status}">{event.status}</span>
    <span class="summary" title={event.summary}>{event.summary}</span>
    {#if event.model !== null}
      <span class="meta model">{event.model}</span>
    {/if}
    {#if event.durationMs !== null}
      <span class="meta duration">{fmtDuration(event.durationMs)}</span>
    {/if}
    <span class="meta time">{fmtTime(event.ts)}</span>
    <span class="meta cost">{cost}</span>
  </div>

  {#if expanded}
    <div class="expanded">
      <dl class="fields">
        <dt>id</dt>
        <dd>{event.id}</dd>
        <dt>eventType</dt>
        <dd>{event.eventType}</dd>
        <dt>status</dt>
        <dd>{event.status}</dd>
        <dt>createdAt</dt>
        <dd>{fmtStamp(event.createdAt)}</dd>
        <dt>ts</dt>
        <dd>{fmtStamp(event.ts)}</dd>
        <dt>durationMs</dt>
        <dd>{event.durationMs ?? "null"}</dd>
        <dt>model</dt>
        <dd>{event.model ?? "null"}</dd>
        <dt>estimatedCostUsd</dt>
        <dd>{event.estimatedCostUsd ?? "null"}</dd>
        <dt>finalCostUsd</dt>
        <dd>{event.finalCostUsd ?? "null"}</dd>
        <dt>summary</dt>
        <dd>{event.summary}</dd>
      </dl>

      {#if event.block}
        <section class="block">
          <h3 class="subhead">Block</h3>
          <dl class="fields">
            <dt>blockHash</dt>
            <dd><code>{event.block.blockHash}</code></dd>
            <dt>filePath</dt>
            <dd>{event.block.filePath ?? "null"}</dd>
            <dt>blockKind</dt>
            <dd>{event.block.blockKind ?? "null"}</dd>
            <dt>blockStart..blockEnd</dt>
            <dd>{event.block.blockStart ?? "null"}..{event.block.blockEnd ?? "null"}</dd>
            <dt>positionStart..positionEnd</dt>
            <dd>{event.block.positionStart ?? "null"}..{event.block.positionEnd ?? "null"}</dd>
          </dl>
          {#if event.block.excerpt}
            <pre class="excerpt"><code>{event.block.excerpt}</code></pre>
          {:else}
            <p class="no-excerpt">(no excerpt)</p>
          {/if}
        </section>
      {/if}

      {#if event.detail}
        <pre class="detail">{JSON.stringify(event.detail, null, 2)}</pre>
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    flex: 0 0 auto;
    border: 1px solid rgba(128, 128, 128, 0.22);
    border-radius: 5px;
    background: rgba(128, 128, 128, 0.05);
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 8px;
    cursor: pointer;
    white-space: nowrap;
  }

  .row.open {
    border-bottom: 1px solid rgba(128, 128, 128, 0.22);
  }

  .badge {
    flex: 0 0 auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    padding: 1px 6px;
    border-radius: 3px;
  }

  .summary {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    opacity: 0.92;
  }

  .meta {
    flex: 0 0 auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    opacity: 0.6;
  }

  .meta.time {
    font-variant-numeric: tabular-nums;
  }

  /* ---- Type badges ---- */

  .type.watcher_file {
    background: rgba(200, 160, 90, 0.2);
    color: #7a5c1e;
  }

  .type.block_diff {
    background: rgba(140, 110, 180, 0.2);
    color: #5a3f7a;
  }

  .type.store_update {
    background: rgba(80, 160, 180, 0.2);
    color: #1f6072;
  }

  .type.llm_call {
    background: rgba(90, 160, 130, 0.2);
    color: #23684e;
  }

  .type.config_reload {
    background: rgba(100, 120, 150, 0.2);
    color: #3d5066;
  }

  /* ---- Status badges ---- */

  .status.queued {
    background: rgba(128, 128, 128, 0.2);
    color: #5a5a5a;
  }

  .status.in_flight {
    background: rgba(60, 110, 190, 0.18);
    color: #1f4e8c;
  }

  .status.done {
    background: rgba(45, 140, 85, 0.18);
    color: #1d6b42;
  }

  .status.failed {
    background: rgba(190, 55, 55, 0.16);
    color: #a32d2d;
  }

  /* ---- Expanded view ---- */

  .expanded {
    padding: 6px 10px 8px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
  }

  .fields {
    margin: 0;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
  }

  .fields dt {
    opacity: 0.55;
  }

  .fields dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .block {
    border-top: 1px solid rgba(128, 128, 128, 0.22);
    padding-top: 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .subhead {
    margin: 0;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.55;
  }

  .excerpt {
    margin: 0;
    padding: 4px 6px;
    border-radius: 3px;
    background: rgba(128, 128, 128, 0.08);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .no-excerpt {
    margin: 0;
    opacity: 0.5;
  }

  .detail {
    margin: 0;
    padding: 6px;
    border-radius: 3px;
    background: rgba(128, 128, 128, 0.08);
    overflow-x: auto;
  }

  @media (prefers-color-scheme: dark) {
    /* Type badges, dark mode */
    .type.watcher_file {
      background: rgba(210, 170, 100, 0.22);
      color: #e8c98a;
    }

    .type.block_diff {
      background: rgba(160, 130, 200, 0.22);
      color: #c4ace8;
    }

    .type.store_update {
      background: rgba(90, 180, 200, 0.22);
      color: #9cd8e8;
    }

    .type.llm_call {
      background: rgba(100, 190, 150, 0.22);
      color: #9ce0c0;
    }

    .type.config_reload {
      background: rgba(130, 150, 180, 0.22);
      color: #b8c8d8;
    }

    /* Status badges, dark mode */
    .status.queued {
      background: rgba(128, 128, 128, 0.25);
      color: #b8b8b8;
    }

    .status.in_flight {
      background: rgba(80, 130, 210, 0.28);
      color: #9cc0f0;
    }

    .status.done {
      background: rgba(70, 170, 110, 0.25);
      color: #8fd8ab;
    }

    .status.failed {
      background: rgba(220, 80, 80, 0.25);
      color: #f0a0a0;
    }
  }
</style>