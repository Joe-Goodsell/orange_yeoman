<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    EditorState,
    RangeSet,
    RangeSetBuilder,
    StateEffect,
    StateField,
    Transaction,
  } from "@codemirror/state";
  import { Decoration, EditorView } from "@codemirror/view";
  import { project } from "./stores/project.svelte";
  import type { AgentFeedback } from "./types";

  // Plain text-only editor for the initial build.
  // Syntax highlighting, Vim keybindings, and markdown rendering are roadmap
  // features (see outline.md) and are intentionally NOT wired here.
  let host: HTMLDivElement;
  // `view` is $state so the effects below re-run once the view exists.
  let view = $state<EditorView | undefined>(undefined);

  // Rebuild-on-demand decoration set. `setFeedbackEffect` carries a freshly
  // built RangeSet; the field keeps it mapped through document changes in
  // between, so decorations stay on the text they refer to.
  const setFeedbackEffect = StateEffect.define<RangeSet<Decoration>>();

  const feedbackField = StateField.define<RangeSet<Decoration>>({
    create: () => RangeSet.empty,
    update(rangeSet, tr) {
      rangeSet = rangeSet.map(tr.changes);
      for (const e of tr.effects) {
        if (e.is(setFeedbackEffect)) rangeSet = e.value;
      }
      return rangeSet;
    },
    provide: (field) => EditorView.decorations.from(field),
  });

  function buildFeedbackDecorations(
    items: AgentFeedback[],
    file: string | null,
    docLen: number
  ): RangeSet<Decoration> {
    // RangeSetBuilder requires ranges to be added in ascending (from, to)
    // order, and the feedback list is not guaranteed to be sorted.
    const relevant = items
      .filter((item) => item.range.file === file)
      .sort(
        (a, b) =>
          a.range.from - b.range.from || a.range.to - b.range.to
      );
    const builder = new RangeSetBuilder<Decoration>();
    for (const item of relevant) {
      const { from, to } = item.range;
      // Ranges that no longer fit the current document (external edits) get no
      // decoration; the card still communicates the stale state.
      if (from < 0 || to > docLen || from >= to) continue;
      builder.add(
        from,
        to,
        Decoration.mark({
          class: `cm-feedback cm-feedback-${item.status}`,
          attributes: {
            "data-feedback-id": item.id,
            title: item.title,
          },
        })
      );
    }
    return builder.finish();
  }

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
          feedbackField,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              const isRemote = u.transactions.some((tr) =>
                tr.annotation(Transaction.remote)
              );
              if (!isRemote) {
                project.dirty =
                  u.state.doc.toString() !== project.openFileContent;
              }
            }
            // Snapshot the selection so the agent pane can link cards whose
            // source range overlaps the current selection. Collapsed
            // selections clear the link.
            if (u.selectionSet || u.docChanged) {
              const sel = u.state.selection.main;
              const snapshot = sel.empty
                ? null
                : { from: sel.from, to: sel.to };
              const cur = project.editorSelection;
              const changed =
                snapshot === null
                  ? cur !== null
                  : cur === null ||
                    snapshot.from !== cur.from ||
                    snapshot.to !== cur.to;
              if (changed) project.setEditorSelection(snapshot);
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

    // Clicking a decorated feedback range links the agent pane to that item.
    view.dom.addEventListener("click", onDecorationClick);
  });

  // Clicking inside a decorated source range focuses the corresponding
  // feedback card (and the card highlights the range in turn).
  function onDecorationClick(e: MouseEvent) {
    const v = view;
    if (!v) return;
    const pos = v.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos == null) return;
    const ranges = v.state.field(feedbackField);
    let id: string | null = null;
    ranges.between(pos, pos + 1, (_from, _to, deco) => {
      const attrs = deco.spec.attributes as Record<string, string> | undefined;
      id = attrs?.["data-feedback-id"] ?? id;
    });
    if (id) {
      e.preventDefault();
      project.focusFeedback(id);
    }
  }

  // Link the editor to the focused feedback item: select and scroll to its
  // source range. `lastFocusedId` prevents re-selecting on unrelated feedback
  // updates (status transitions etc.). It is reset when the open file changes
  // so switching files never blocks a later re-reveal of the same item.
  let lastFocusedId: string | null = null;

  // Replace the document when the open file changes (preserved watcher path).
  let prevOpenFilePath: string | null = project.openFilePath;
  $effect(() => {
    const file = project.openFilePath;
    const content = project.openFileContent;
    if (file !== prevOpenFilePath) {
      prevOpenFilePath = file;
      // A freshly opened file has no meaningful selection to link against,
      // and the previous file's focus state must not block a re-reveal of
      // the same feedback id.
      project.setEditorSelection(null);
      lastFocusedId = null;
    }
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

  // Rebuild the range decorations whenever feedback or the open file (path or
  // content snapshot) changes. Reacting to the content snapshot means an
  // external edit or file reopen rebuilds decorations against the current
  // document instead of leaving the previous set mapped into a shrunken
  // document. The doc-sync effect above runs first and keeps the editor
  // document equal to the snapshot; feedback ranges were anchored to the
  // snapshot, so bounding by the smaller of the two lengths prunes ranges
  // that no longer fit.
  $effect(() => {
    const v = view;
    if (!v) return;
    const file = project.openFilePath;
    const items = project.feedback;
    const content = project.openFileContent;
    v.dispatch({
      effects: setFeedbackEffect.of(
        buildFeedbackDecorations(
          items,
          file,
          Math.min(v.state.doc.length, content.length)
        )
      ),
    });
  });

  // Link the editor to the focused feedback item: select and scroll to its
  // source range. `lastFocusedId` (declared above) prevents re-selecting on
  // unrelated feedback updates.
  $effect(() => {
    const v = view;
    const id = project.selectedFeedbackId;
    if (!v) return;
    if (!id) {
      lastFocusedId = null;
      return;
    }
    if (id === lastFocusedId) return;
    const item = project.feedback.find((f) => f.id === id);
    if (!item) return;
    if (item.range.file !== project.openFilePath) return;
    const { from, to } = item.range;
    if (from < 0 || to > v.state.doc.length || from >= to) return;
    lastFocusedId = id;
    v.dispatch({
      selection: { anchor: from, head: to },
      effects: EditorView.scrollIntoView(from, { y: "center" }),
    });
    v.focus();
  });

  onDestroy(() => {
    if (view) {
      view.dom.removeEventListener("click", onDecorationClick);
      view.destroy();
    }
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

  /* Source-range decorations produced by agent feedback. */
  .editor-host :global(.cm-feedback) {
    cursor: pointer;
  }

  .editor-host :global(.cm-feedback:hover) {
    outline: 1px solid rgba(255, 255, 255, 0.35);
  }

  .editor-host :global(.cm-feedback-arrived) {
    background-color: rgba(90, 160, 130, 0.28);
    border-radius: 2px;
  }

  .editor-host :global(.cm-feedback-running) {
    background-color: rgba(180, 150, 90, 0.22);
    border-radius: 2px;
  }

  .editor-host :global(.cm-feedback-stale),
  .editor-host :global(.cm-feedback-error) {
    background-color: rgba(220, 140, 140, 0.22);
    border-radius: 2px;
  }
</style>