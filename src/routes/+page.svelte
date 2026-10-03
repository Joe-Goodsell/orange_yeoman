<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import EventFeed from "$lib/EventFeed.svelte";
  import { appStore } from "$lib/stores/app.svelte";
  import { feed } from "$lib/stores/feed.svelte";
  import {
    pickFolder,
    startWatcher,
    stopWatcher,
    onConfigChanged,
    onBackendEvent,
  } from "$lib/tauri";

  let unlistenConfig: (() => void) | null = null;
  let unlistenBackend: (() => void) | null = null;
  let alive = true;
  // Track the root already started so the $effect skips the initial value
  // (onMount starts the initial watcher after the listener is registered).
  let prevRoot: string | null = appStore.projectRoot;

  const rootName = $derived(
    appStore.projectRoot
      ? appStore.projectRoot.split("/").filter(Boolean).pop() ?? appStore.projectRoot
      : "",
  );

  onMount(async () => {
    const unCfg = await onConfigChanged((status) =>
      appStore.applyConfigStatus(status),
    );
    const unBk = await onBackendEvent((e) => feed.upsert(e));
    if (!alive) {
      unCfg();
      unBk();
      return;
    }
    unlistenConfig = unCfg;
    unlistenBackend = unBk;
    // Refresh the merged config status for the persisted root. The global
    // config was already loaded during Rust app setup.
    appStore.loadProjectConfig(appStore.projectRoot);
    if (appStore.projectRoot) {
      // Start the watcher for the initial/persisted root only after the
      // listener is ready, so no early events are dropped.
      startWatcher(appStore.projectRoot)
        .then(() => appStore.setWatching(true))
        .catch((e) => {
          appStore.setError(String(e));
          appStore.setWatching(false);
        });
    }
  });

  onDestroy(() => {
    alive = false;
    unlistenConfig?.();
    unlistenBackend?.();
    if (appStore.watching) {
      // Best-effort stop; ignore promise rejection
      stopWatcher().catch(() => {});
    }
  });

  // Start/stop the watcher when the project root CHANGES after mount: stop
  // the old watcher first (best-effort), then start the new one when the root
  // is non-null.
  $effect(() => {
    const root = appStore.projectRoot;
    if (root === prevRoot) return;
    prevRoot = root;
    // The root changed to a different project: clear the console before the
    // new watcher starts so events from two projects never mix.
    feed.clear();
    stopWatcher().catch(() => {});
    if (root) {
      startWatcher(root)
        .then(() => appStore.setWatching(true))
        .catch((e) => {
          appStore.setError(String(e));
          appStore.setWatching(false);
        });
    } else {
      appStore.setWatching(false);
    }
  });

  async function openProjectPicker() {
    const folder = await pickFolder();
    if (folder) {
      appStore.selectRoot(folder);
    }
  }
</script>

{#if !appStore.projectRoot}
  <div class="hero">
    <h1 class="hero-title">Orange Yeoman</h1>
    <button class="hero-btn" onclick={openProjectPicker}>Open a project</button>
    <p class="hero-text">
      Watch a folder of .md notes; the console shows backend activity.
    </p>
  </div>
{:else}
  <div class="app-shell">
    <header class="topbar">
      <span class="root" title={appStore.projectRoot ?? ""}>{rootName}</span>
      <span class="watch {appStore.watching ? "on" : ""}">
        {appStore.watching ? "watching" : "stopped"}
      </span>
      {#if appStore.configStatus}
        <span class="badge model" title="small model">
          small: {appStore.configStatus.small_model}
        </span>
        <span class="badge model" title="large model">
          large: {appStore.configStatus.large_model}
        </span>
        {#if appStore.configStatus.mock_llm}
          <span class="badge mock">mock</span>
        {/if}
        {#if appStore.configStatus.debug}
          <span class="badge debug">debug</span>
        {/if}
      {/if}
      <span class="actions">
        <button class="bar-btn" type="button" onclick={openProjectPicker}>
          Change project
        </button>
        <button class="bar-btn" type="button" onclick={() => appStore.clearProject()}>
          Close
        </button>
      </span>
    </header>
    {#if appStore.error}
      <p class="error-line">{appStore.error}</p>
    {/if}
    <EventFeed />
  </div>
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

  .hero-title {
    margin: 0;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.01em;
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

  .app-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    min-height: 0;
  }

  .topbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 10px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.25);
    font-size: 11px;
  }

  .root {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 30%;
    opacity: 0.85;
  }

  .watch {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 5px;
    opacity: 0.6;
  }

  .watch::before {
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #8a8a8a;
  }

  .watch.on {
    opacity: 1;
  }

  .watch.on::before {
    background: #2f9e5f;
  }

  .badge {
    flex: 0 0 auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    padding: 1px 6px;
    border-radius: 3px;
    background: rgba(128, 128, 128, 0.18);
    opacity: 0.85;
  }

  .badge.mock {
    background: rgba(90, 160, 130, 0.22);
    color: #23684e;
  }

  .badge.debug {
    background: rgba(80, 160, 180, 0.22);
    color: #1f6072;
  }

  .actions {
    flex: 0 0 auto;
    margin-left: auto;
    display: flex;
    gap: 6px;
  }

  .bar-btn {
    flex: 0 0 auto;
    font-size: 10px;
    padding: 2px 8px;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.7;
  }

  .bar-btn:hover {
    background-color: rgba(128, 128, 128, 0.15);
  }

  .error-line {
    flex: 0 0 auto;
    margin: 0;
    padding: 3px 10px;
    font-size: 10px;
    border-bottom: 1px solid rgba(190, 55, 55, 0.35);
    background: rgba(190, 55, 55, 0.12);
    color: #a32d2d;
    overflow-wrap: anywhere;
  }

  @media (prefers-color-scheme: dark) {
    .badge.mock {
      color: #9ce0c0;
    }

    .badge.debug {
      color: #9cd8e8;
    }

    .error-line {
      color: #f0a0a0;
    }
  }
</style>