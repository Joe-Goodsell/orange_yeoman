<script lang="ts">
  // Right-hand side pane. Surfaces AI agent status/progress/results.
  // For the initial scaffold there are no active agents; this is a placeholder
  // that keeps the main editor distraction-free per outline.md.
</script>

<aside class="pane">
  <header class="header">
    <h2>Agents</h2>
  </header>
  <div class="empty">
    <p class="empty-title">No agents active</p>
    <p class="empty-sub">Agent activity will appear here.</p>
  </div>
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
</style>