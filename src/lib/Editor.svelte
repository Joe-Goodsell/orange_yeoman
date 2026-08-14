<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, Transaction } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { project } from "./stores/project.svelte";

  // Plain text-only editor for the initial build.
  // Syntax highlighting, Vim keybindings, and markdown rendering are roadmap
  // features (see outline.md) and are intentionally NOT wired here.
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  let fileName = $derived(
    project.openFilePath
      ? project.openFilePath.split("/").filter(Boolean).pop() ?? project.openFilePath
      : ""
  );

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: "",
        extensions: [
          EditorView.lineWrapping,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              const isRemote = u.transactions.some(
                (tr) => tr.annotation(Transaction.remote)
              );
              if (!isRemote) {
                project.dirty = u.state.doc.toString() !== project.openFileContent;
              }
            }
          }),
          EditorView.theme({
            "&": {
              height: "100%",
              fontSize: "14px",
            },
            ".cm-scroller": {
              fontFamily:
                'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace',
            },
            ".cm-content": {
              padding: "12px 16px",
            },
          }),
        ],
      }),
      parent: host,
    });
  });

  $effect(() => {
    const content = project.openFileContent;
    if (view && view.state.doc.toString() !== content) {
      view.dispatch({
        annotations: Transaction.remote.of(true),
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: content,
        },
      });
    }
  });

  onDestroy(() => {
    view?.destroy();
    view = undefined;
  });
</script>

<div class="editor-container">
  <div class="editor-header">
    <span class="file-name">{fileName}</span>
    {#if project.dirty && project.openFilePath}
      <span class="unsaved">unsaved</span>
    {/if}
    {#if project.loading}
      <span class="loading">loading...</span>
    {/if}
  </div>
  <div class="editor-host" bind:this={host}></div>
</div>

<style>
  .editor-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .editor-header {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 14px;
    border-bottom: 1px solid rgba(128, 128, 128, 0.18);
    min-height: 28px;
  }

  .file-name {
    font-size: 12px;
    opacity: 0.6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unsaved {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #c88;
    opacity: 0.8;
  }

  .loading {
    font-size: 11px;
    opacity: 0.4;
  }

  .editor-host {
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }

  .editor-host :global(.cm-editor) {
    height: 100%;
  }

  .editor-host :global(.cm-scroller) {
    overflow: auto;
  }
</style>