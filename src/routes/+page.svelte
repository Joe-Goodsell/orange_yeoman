<script lang="ts">
  import Editor from "$lib/Editor.svelte";
  import AgentPane from "$lib/AgentPane.svelte";
</script>

<main class="app">
  <section class="editor-pane">
    <Editor />
  </section>
  <aside class="agent-pane">
    <AgentPane />
  </aside>
</main>

<style>
  .app {
    display: flex;
    height: 100%;
    width: 100%;
  }

  .editor-pane {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid rgba(128, 128, 128, 0.25);
  }

  .agent-pane {
    flex: 0 0 320px;
    width: 320px;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>