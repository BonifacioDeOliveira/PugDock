<script lang="ts">
  import { app } from "$lib/state.svelte";
  import Chat from "./Chat.svelte";

  let open = $state(false);

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) open = false;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="ai-pop" class:dodge={!!app.panel}>
    <div class="ai-head">
      <span class="ai-title">🐾 PugDock AI</span>
      <button class="ghost" onclick={() => (open = false)} data-tip="Close (keeps the conversation)" data-tip-align="end">×</button>
    </div>
    <Chat />
  </div>
{/if}

<button
  class="ai-fab"
  class:open
  class:dodge={!!app.panel}
  onclick={() => (open = !open)}
  data-tip="PugDock AI"
  data-tip-pos="top"
  data-tip-align={app.panel ? undefined : "end"}
>
  {#if open}
    ×
  {:else}
    <img src="/PugDockAI.png" alt="PugDock AI" />
  {/if}
</button>

<style>
  .ai-fab {
    position: fixed;
    right: 24px;
    bottom: 24px;
    width: 52px;
    height: 52px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    font-size: 22px;
    line-height: 1;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    z-index: 400;
    transition: transform 0.15s ease, right 0.2s ease;
    padding: 0;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .ai-fab img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }
  .ai-fab.dodge {
    right: 364px; /* side panel width + margin */
  }
  .ai-fab:hover {
    transform: scale(1.08);
    background: var(--bg-hover);
  }
  .ai-fab.open {
    font-size: 20px;
    color: var(--text-dim);
  }
  .ai-pop {
    position: fixed;
    right: 24px;
    bottom: 84px;
    width: 400px;
    max-width: calc(100vw - 48px);
    height: 560px;
    max-height: calc(100vh - 130px);
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
    z-index: 399;
    overflow: hidden;
    transition: right 0.2s ease;
  }
  .ai-pop.dodge {
    right: 364px;
  }
  .ai-head {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 10px 6px 14px;
    border-bottom: 1px solid var(--border);
    flex: 0 0 auto;
  }
  .ai-title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
  }
</style>
