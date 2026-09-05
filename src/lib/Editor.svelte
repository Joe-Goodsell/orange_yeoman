<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    EditorState,
    Prec,
    RangeSet,
    RangeSetBuilder,
    StateEffect,
    StateField,
    Transaction,
  } from "@codemirror/state";
  import { Decoration, EditorView, keymap } from "@codemirror/view";
  import { completionStatus } from "@codemirror/autocomplete";
  import { project } from "./stores/project.svelte";
  import { submitBlock, getTaskStatus } from "./tauri";
  import {
    detectSlashCommandInLine,
    hashText,
    paragraphAround,
    type DetectedCommand,
    type ParagraphSlice,
  } from "./slashCommands";
  import { slashCommandAutocomplete } from "./slashCommandAutocomplete";
  import { markdownHighlight } from "./markdownHighlight";
  import type { AgentFeedback, SourceRange } from "./types";
  import { byteOffsetToCharOffset, commandNameToFeedbackKind } from "./taskFeedback";

  // Plain text-only editor for the initial build. Markdown syntax highlighting
  // (token coloring only, via markdownHighlight) is wired below; markdown
  // rendering and Vim keybindings remain roadmap features (see outline.md)
  // and are intentionally NOT wired here.
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

  // --- Slash-command detection and dispatch ---
  //
  // Detection and dispatch are deliberately separated. Dispatch happens only
  // when the user presses Enter with a complete bounded command in the cursor
  // paragraph, and only once per (command, paragraph-content) pair. The model
  // API call must fire ONLY on Enter, so typing alone never triggers it.
  // Dedup keys for already-dispatched commands. Cleared when the open file
  // changes so switching files never blocks a later dispatch.
  const dispatchedKeys = new Set<string>();

  // Scan a paragraph's lines in document order and return the first complete
  // bounded command, mirroring Rust parse_slash_command_in_block.
  function scanParagraphForCommand(text: string): DetectedCommand | null {
    for (const line of text.split("\n")) {
      const found = detectSlashCommandInLine(line);
      if (found) return found;
    }
    return null;
  }

  // Enter key handler: dispatch a complete bounded command in the cursor
  // paragraph. Returns false so CodeMirror still performs the default Enter
  // behavior (accept a completion when the popup is open, insert a newline
  // otherwise); the IPC call is a fire-and-forget side effect.
  function onEnterDispatch(view: EditorView): boolean {
    // With the autocompletion popup open, Enter must accept the completion,
    // not dispatch. The popup closes by itself once the command word is
    // complete and bounded, so by the time a real dispatch can happen this
    // check is false.
    if (completionStatus(view.state) !== null) return false;

    const paragraph = paragraphAround(
      view.state.doc.toString(),
      view.state.selection.main.from
    );
    const detected = scanParagraphForCommand(paragraph.text);
    if (!detected) return false;

    const key = `${detected.name}:${hashText(paragraph.text)}`;
    if (dispatchedKeys.has(key)) return false;

    // No dispatch without an open file: the editor mounts whenever a project
    // root is selected, so the document can hold a complete command with no
    // backing file. Returning false lets CodeMirror run the default Enter
    // behavior instead.
    if (!project.openFilePath) return false;

    dispatchedKeys.add(key);
    const range: SourceRange = {
      file: project.openFilePath,
      from: paragraph.startOffset,
      to: paragraph.startOffset + paragraph.text.length,
    };
    void dispatchSlashCommand(detected, range, paragraph);
    return false;
  }

  // Dispatch a slash command through the Rust task pipeline. When Rust
  // dispatches a task (/fact-check and /research), register it immediately so
  // the queued card appears, then refine the source range to the focus line
  // using the byte offsets from task metadata. When Rust does not dispatch
  // (only /ignore — recognized but not dispatched yet), fall back to the
  // frontend mock card.
  async function dispatchSlashCommand(
    detected: DetectedCommand,
    paragraphRange: SourceRange,
    paragraph: ParagraphSlice,
  ): Promise<void> {
    const kind = commandNameToFeedbackKind(detected.name);
    if (!kind) return;
    const filePath = project.openFilePath;
    if (!filePath) return;
    try {
      const taskId = await submitBlock(filePath, paragraph.text);
      if (taskId) {
        // Register the queued item before the running event arrives.
        project.registerTask(taskId, kind, paragraphRange);
        // Refine the range to the focus line. The metadata byte offsets are
        // paragraph-relative; convert them to character offsets within the
        // paragraph, then add the paragraph's start offset for file-relative
        // character offsets the editor expects.
        const meta = await getTaskStatus(taskId);
        if (meta && meta.focusStart != null && meta.focusEnd != null) {
          const from =
            paragraph.startOffset +
            byteOffsetToCharOffset(paragraph.text, meta.focusStart);
          const to =
            paragraph.startOffset +
            byteOffsetToCharOffset(paragraph.text, meta.focusEnd);
          project.updateFeedback(taskId, {
            range: { file: filePath, from, to },
          });
        }
      } else {
        // Rust did not dispatch this command. Only /ignore falls back to the
        // frontend mock card; it arrives immediately because it is a
        // directive.
        project.dispatchMockFeedback(detected, paragraphRange);
      }
    } catch (e) {
      project.error = String(e);
    }
  }

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: "",
        extensions: [
          EditorView.lineWrapping,
          // Markdown syntax highlighting, placed early so feedback
          // decorations (background overlays) layer on top of it.
          markdownHighlight(),
          feedbackField,
          slashCommandAutocomplete(),
          // Enter dispatches a complete bounded slash command as a side
          // effect; the highest precedence ensures it runs before both the
          // completion keymap and the default newline insertion.
          Prec.highest(keymap.of([{ key: "Enter", run: onEnterDispatch }])),
          EditorView.updateListener.of((u) => {
            const isRemote = u.transactions.some((tr) =>
              tr.annotation(Transaction.remote)
            );
            if (u.docChanged && !isRemote) {
              project.dirty =
                u.state.doc.toString() !== project.openFileContent;
              // Keep feedback ranges attached to the text they refer to as
              // the user types. Local edits only; remote edits replace the
              // whole document and stale-mark feedback instead.
              project.mapFeedbackRanges(
                u.changes.mapPos.bind(u.changes) as (
                  pos: number,
                  assoc: -1 | 1
                ) => number,
                project.openFilePath
              );
            }
            // Snapshot the selection so the agent pane can link cards whose
            // source range overlaps the current selection. A collapsed
            // selection is a caret position (from === to) and still links
            // ranges that contain it.
            if (u.selectionSet || u.docChanged) {
              const sel = u.state.selection.main;
              const snapshot = { from: sel.from, to: sel.to };
              const cur = project.editorSelection;
              const changed =
                cur === null ||
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
      // Slash-command dispatch state is per-file: a paragraph hash in the new
      // file must never be blocked by a dispatch that happened in the old one.
      dispatchedKeys.clear();
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

  // Rebuild the range decorations whenever feedback or the open file changes.
  // The effect still re-runs on project.openFilePath changes (file reopen) and
  // project.feedback changes (range mapping / new items / status transitions).
  // Local edits map feedback ranges through the change set in the update
  // listener, and external edits sync the whole document via the doc-sync
  // effect above, so the current document length is the correct bound: the
  // old Math.min with the content snapshot clipped live ranges after local
  // typing and dropped the decoration we just added.
  $effect(() => {
    const v = view;
    if (!v) return;
    const file = project.openFilePath;
    const items = project.feedback;
    v.dispatch({
      effects: setFeedbackEffect.of(
        buildFeedbackDecorations(items, file, v.state.doc.length)
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

  .editor-host :global(.cm-feedback-queued) {
    background-color: rgba(128, 128, 128, 0.18);
    border-radius: 2px;
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