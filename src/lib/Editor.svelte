<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";

  // Plain text-only editor for the initial build.
  // Syntax highlighting, Vim keybindings, and markdown rendering are roadmap
  // features (see outline.md) and are intentionally NOT wired here.
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: "",
        extensions: [
          EditorView.lineWrapping,
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

  onDestroy(() => {
    view?.destroy();
    view = undefined;
  });
</script>

<div class="editor-host" bind:this={host}></div>

<style>
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