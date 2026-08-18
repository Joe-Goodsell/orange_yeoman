<script lang="ts">
  // Right-hand side pane. Surfaces AI agent status/progress/results.
  // Agent feedback cards link to their source ranges in the editor; the live
  // watcher change feed (structure/content events) is preserved below.
  import { project } from "./stores/project.svelte";
  import type { AgentFeedback, FeedbackKind, FeedbackStatus } from "./types";

  const KIND_LABEL: Record<FeedbackKind, string> = {
    research: "RESEARCH",
    "fact-check": "FACT-CHECK",
    correction: "CORRECTION",
    ignore: "IGNORE",
  };

  const STATUS_LABEL: Record<FeedbackStatus, string> = {
    queued: "Queued",
    running: "Running",
    arrived: "Arrived",
    stale: "Stale",
    error: "Error",
  };

  // Per-card collapse state, keyed by feedback id so it survives status
  // updates and list reordering.
  let expandedIds = $state<Record<string, boolean>>({});

  function toggleDetail(id: string) {
    expandedIds = { ...expandedIds, [id]: !(expandedIds[id] ?? false) };
  }

  let cardsRef = $state<HTMLDivElement | undefined>(undefined);

  // Scroll the focused card into view when the editor or another card links
  // to it.
  $effect(() => {
    const id = project.selectedFeedbackId;
    if (!id || !cardsRef) return;
    const el = cardsRef.querySelector(`[data-feedback-id="${CSS.escape(id)}"]`);
    el?.scrollIntoView({ block: "nearest" });
  });

  // Focusing a section (via the editor link or its own header) expands it so
  // the full response is visible. A manual collapse afterwards stays collapsed:
  // the latch only re-expands when a new id is focused, or the same id is
  // focused again after focus was cleared.
  let lastAutoExpandedId: string | null = null;
  $effect(() => {
    const id = project.selectedFeedbackId;
    if (id && id !== lastAutoExpandedId) {
      lastAutoExpandedId = id;
      expandedIds = { ...expandedIds, [id]: true };
    }
    if (!id) {
      lastAutoExpandedId = null;
    }
  });

  // Scroll the first linked card into view when the caret enters a feedback
  // range without an explicit focus. `lastLinkedKey` dedupes so keystrokes
  // inside the same range do not re-scroll. Explicit focus scrolling is
  // handled by the effect above.
  let lastLinkedKey = "";
  $effect(() => {
    const linked = project.linkedFeedbackIds;
    const key = linked.join(",");
    if (key === lastLinkedKey) return;
    if (!cardsRef || linked.length === 0) return;
    lastLinkedKey = key;
    const el = cardsRef.querySelector(
      `[data-feedback-id="${CSS.escape(linked[0])}"]`
    );
    el?.scrollIntoView({ block: "nearest" });
  });

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
      {:else if project.configStatus.global_loaded || project.configStatus.project_loaded}
        <p class="config-ok">Config loaded</p>
      {:else}
        <p class="config-ok">Defaults active</p>
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
    <section class="feedback-section">
      <header class="section-head">
        <h3 class="section-title">Feedback</h3>
        <button
          class="demo-btn"
          type="button"
          onclick={() => project.seedMockFeedback()}
          title="Seed demo feedback to preview the linked feedback UI"
        >
          Demo feedback
        </button>
      </header>

      {#if project.feedback.length === 0}
        <p class="no-feedback">
          No agent activity yet. Research and fact-check feedback will appear
          here.
        </p>
      {:else}
        <div class="cards" bind:this={cardsRef}>
          {#each project.feedback as item (item.id)}
            {@const linked = project.linkedFeedbackIds.includes(item.id)}
            {@const focused = project.selectedFeedbackId === item.id}
            {@const expanded = expandedIds[item.id] ?? false}
            {@const hasDetail = item.detail.length > 0}
            {@const titleId = `${item.id}-title`}
            {@const detailId = `${item.id}-detail`}
            <section
              class="card {item.status} {linked ? 'linked' : ''} {focused ? 'focused' : ''}"
              data-feedback-id={item.id}
              aria-labelledby={titleId}
            >
              <header class="card-head">
                <button
                  id={titleId}
                  class="card-title"
                  type="button"
                  onclick={() => project.focusFeedback(item.id)}
                  title="Reveal this range in the editor"
                >
                  {item.title}
                </button>
                <span class="badge {item.kind}">{KIND_LABEL[item.kind]}</span>
              </header>
              <p class="card-status {item.status}">{STATUS_LABEL[item.status]}</p>
              <p class="card-model">
                {item.provider} &middot; <span class="model-name">{item.model}</span>
              </p>
              <p class="card-summary">{item.summary}</p>
              {#if item.status === "stale"}
                <p class="stale-note">
                  File changed since this feedback was produced. Review before
                  relying on it.
                </p>
                <button
                  class="rerun-btn"
                  type="button"
                  onclick={() => project.requeueFeedback(item.id)}
                >
                  Re-run
                </button>
              {/if}
              {#if hasDetail}
                <button
                  class="toggle"
                  type="button"
                  aria-expanded={expanded}
                  aria-controls={detailId}
                  onclick={() => toggleDetail(item.id)}
                >
                  {expanded ? "Collapse" : "Expand"} full response
                </button>
                <div
                  id={detailId}
                  class="detail"
                  role="region"
                  aria-label="Full response"
                  hidden={!expanded}
                >
                  <p class="detail-text">{item.detail}</p>
                </div>
              {/if}
              <p class="card-time">Updated {fmtTime(item.updatedAt)}</p>
            </section>
          {/each}
        </div>
      {/if}
    </section>

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

  /* ---- Feedback cards ---- */

  .feedback-section {
    flex: 1 1 auto;
    min-height: 80px;
    display: flex;
    flex-direction: column;
    margin-top: 10px;
  }

  .section-head {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }

  .section-title {
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.6;
  }

  .demo-btn {
    font-size: 10px;
    padding: 2px 8px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.7;
  }

  .demo-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }

  .no-feedback {
    margin: 0;
    padding: 4px 6px;
    font-size: 12px;
    opacity: 0.45;
  }

  .cards {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 2px;
  }

  .card {
    flex: 0 0 auto;
    padding: 8px 10px;
    border: 1px solid rgba(128, 128, 128, 0.22);
    border-radius: 6px;
    background: rgba(128, 128, 128, 0.07);
    font-size: 12px;
  }

  .card.linked {
    border-color: rgba(140, 210, 180, 0.55);
  }

  .card.focused {
    outline: 1px solid rgba(255, 255, 255, 0.4);
  }

  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .card-title {
    flex: 1 1 auto;
    min-width: 0;
    text-align: left;
    font-size: 12px;
    font-weight: 600;
    color: inherit;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-title:hover {
    text-decoration: underline;
  }

  .card-status {
    margin: 6px 0 0;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .card-status.queued {
    color: rgba(190, 190, 190, 0.7);
  }

  .card-status.running {
    color: rgba(220, 190, 130, 0.95);
  }

  .card-status.arrived {
    color: rgba(140, 210, 180, 0.95);
  }

  .card-status.stale {
    color: rgba(220, 160, 120, 0.95);
  }

  .card-status.error {
    color: rgba(230, 140, 140, 0.95);
  }

  .card-summary {
    margin: 4px 0 0;
    opacity: 0.85;
  }

  /* Model provenance: secondary metadata, always visible in both states. */
  .card-model {
    margin: 4px 0 0;
    font-size: 10px;
    opacity: 0.5;
    font-variant-numeric: tabular-nums;
  }

  .card-model .model-name {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  .stale-note {
    margin: 6px 0 0;
    color: rgba(220, 160, 120, 0.9);
  }

  .rerun-btn {
    margin: 6px 4px 0 0;
    font-size: 11px;
    padding: 2px 8px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  .rerun-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }

  .toggle {
    margin-top: 6px;
    font-size: 11px;
    padding: 2px 0;
    background: transparent;
    border: none;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
    text-decoration: underline;
  }

  .toggle:hover {
    opacity: 1;
  }

  .detail {
    margin-top: 6px;
    padding: 6px 8px;
    border-left: 2px solid rgba(128, 128, 128, 0.3);
    background: rgba(128, 128, 128, 0.08);
    border-radius: 0 4px 4px 0;
  }

  .detail-text {
    margin: 0;
    font-size: 11px;
    opacity: 0.9;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  .card-time {
    margin: 6px 0 0;
    font-size: 10px;
    opacity: 0.45;
    font-variant-numeric: tabular-nums;
  }

  /* ---- Watcher feed (preserved) ---- */

  .feed-scroll {
    flex: 0 1 auto;
    max-height: 40%;
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

  .badge.research {
    background: rgba(120, 150, 200, 0.25);
    color: rgba(170, 190, 230, 0.95);
  }

  .badge.fact-check {
    background: rgba(90, 160, 130, 0.22);
    color: rgba(140, 210, 180, 0.95);
  }

  .badge.correction {
    background: rgba(200, 160, 90, 0.22);
    color: rgba(220, 190, 130, 0.95);
  }

  .badge.ignore {
    background: rgba(128, 128, 128, 0.22);
    color: rgba(200, 200, 200, 0.9);
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
