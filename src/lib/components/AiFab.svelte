<script lang="ts">
  import { api, errorMessage } from "$lib/api";
  import { app, openFile, refreshTree, settings, replaceTabContent, toast } from "$lib/state.svelte";
  import * as ai from "$lib/ai";

  type Msg = { role: "user" | "ai"; text: string; sources?: string[] };

  let open = $state(false);
  let input = $state("");
  let msgs = $state<Msg[]>([]);
  let busy = $state(false);
  let mode = $state<"chat" | "draft">("chat");
  let thread: HTMLDivElement | undefined = $state();

  const enabled = $derived(settings().aiEnabled);
  const activeTab = $derived(app.tabs.find((t) => t.path === app.activePath));
  const activeIsNote = $derived(!!activeTab && activeTab.kind === "text" && !ai.aiExcluded(activeTab.path));

  function scrollDown() {
    setTimeout(() => thread?.scrollTo({ top: thread.scrollHeight, behavior: "smooth" }), 30);
  }

  async function guarded(fn: () => Promise<void>) {
    busy = true;
    try {
      await fn();
    } catch (e) {
      msgs.push({ role: "ai", text: `⚠️ ${errorMessage(e)}` });
    } finally {
      busy = false;
      scrollDown();
    }
  }

  function submit() {
    const q = input.trim();
    if (!q || busy) return;
    input = "";
    msgs.push({ role: "user", text: q });
    scrollDown();
    if (mode === "draft") {
      mode = "chat";
      guarded(() => createNoteFrom(q));
    } else {
      guarded(async () => {
        const blocks = (await api.searchContext(q, 10)).filter(([p]) => !ai.aiExcluded(p));
        const answer = await ai.askPugdock(q, blocks.map(([path, text]) => ({ path, text })));
        msgs.push({ role: "ai", text: answer, sources: blocks.map(([p]) => p) });
      });
    }
  }

  async function createNoteFrom(request: string) {
    const content = await ai.draftNote(request);
    const title = content.match(/^#\s+(.+)$/m)?.[1] ?? request;
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "note";
    let dest = `notes/${slug}.md`;
    for (let i = 2; app.tabs.some((t) => t.path === dest); i++) dest = `notes/${slug}-${i}.md`;
    await api.writeFile(dest, content);
    await refreshTree();
    await openFile(dest);
    msgs.push({ role: "ai", text: `📝 Created and opened **${dest}**.` });
  }

  function enrichCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Enrich "${tab.name}"` });
    guarded(async () => {
      const improved = await ai.enrichNote(tab.content);
      await api.writeFile(tab.path, improved);
      replaceTabContent(tab.path, improved);
      msgs.push({ role: "ai", text: `✨ Enriched **${tab.name}** — previous version stays in History.` });
    });
  }

  function summarizeCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Summarize "${tab.name}"` });
    guarded(async () => {
      const summary = await ai.summarize(tab.name, tab.content);
      msgs.push({ role: "ai", text: summary, sources: [tab.path] });
    });
  }

  function continueWriting() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Continue writing "${tab.name}"` });
    guarded(async () => {
      const more = await ai.continueWriting(tab.content);
      const updated = tab.content.replace(/\s*$/, "\n\n") + more + "\n";
      await api.writeFile(tab.path, updated);
      replaceTabContent(tab.path, updated);
      msgs.push({ role: "ai", text: `✍️ Continued **${tab.name}**.` });
    });
  }

  async function insertIntoNote(text: string) {
    if (!activeTab || activeTab.kind !== "text") {
      toast("Open a text note first.");
      return;
    }
    const updated = activeTab.content.replace(/\s*$/, "\n\n") + text + "\n";
    await api.writeFile(activeTab.path, updated);
    replaceTabContent(activeTab.path, updated);
    toast(`Inserted into ${activeTab.name}`);
  }

  async function saveAsNote(text: string) {
    const title = text.match(/^#\s+(.+)$/m)?.[1] ?? text.split("\n")[0].slice(0, 50);
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "note";
    const dest = `notes/${slug}.md`;
    await api.writeFile(dest, text.startsWith("#") ? text : `# ${title}\n\n${text}`);
    await refreshTree();
    await openFile(dest);
    toast(`Saved as ${dest}`);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) open = false;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="ai-pop">
    <div class="ai-head">
      <span class="ai-title">🐾 PugDock AI</span>
      <button class="ghost" onclick={() => (msgs = [])} title="Clear conversation">↺</button>
      <button class="ghost" onclick={() => (open = false)}>×</button>
    </div>

    {#if !enabled}
      <div class="ai-connect">
        <p>Sign in with Anthropic to ask questions, generate notes and enrich your writing.</p>
        <button class="primary" onclick={() => { open = false; app.panel = "settings"; }}>
          Open AI settings
        </button>
      </div>
    {:else}
      <div class="thread" bind:this={thread}>
        {#if msgs.length === 0}
          <p class="hint">
            Ask anything about your workspace, or use a quick action below.
          </p>
        {/if}
        {#each msgs as m, i (i)}
          <div class="msg {m.role}">
            <div class="msg-text">{m.text}</div>
            {#if m.role === "ai" && !m.text.startsWith("⚠️") && !m.text.startsWith("📝") && !m.text.startsWith("✨") && !m.text.startsWith("✍️")}
              <div class="msg-actions">
                <button class="ghost" onclick={() => insertIntoNote(m.text)}>Insert into note</button>
                <button class="ghost" onclick={() => saveAsNote(m.text)}>Save as note</button>
              </div>
            {/if}
            {#if m.sources?.length}
              <div class="sources">
                {#each m.sources as s (s)}
                  <button class="ghost" onclick={() => openFile(s)}>{s}</button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
        {#if busy}<div class="msg ai"><div class="msg-text pulse">Thinking…</div></div>{/if}
      </div>

      <div class="chips">
        <button class="chip" class:on={mode === "draft"} onclick={() => (mode = mode === "draft" ? "chat" : "draft")}>
          ✍️ Draft a note
        </button>
        {#if activeIsNote}
          <button class="chip" onclick={enrichCurrent} disabled={busy}>✨ Enrich</button>
          <button class="chip" onclick={continueWriting} disabled={busy}>➡️ Continue writing</button>
          <button class="chip" onclick={summarizeCurrent} disabled={busy}>📄 Summarize</button>
        {/if}
      </div>

      <form
        class="ai-input"
        onsubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input
          bind:value={input}
          autofocus
          disabled={busy}
          placeholder={mode === "draft" ? "What should the note be about?" : "Ask, search, or generate…"}
        />
        <button type="submit" class="send" disabled={busy || !input.trim()}>↑</button>
      </form>
    {/if}
  </div>
{/if}

<button class="ai-fab" class:open onclick={() => (open = !open)} title="PugDock AI">
  {open ? "×" : "🐾"}
</button>

<style>
  .ai-fab {
    position: fixed;
    right: 24px;
    bottom: 24px;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    font-size: 22px;
    line-height: 1;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    z-index: 400;
    transition: transform 0.15s ease;
    padding: 0;
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
    height: 540px;
    max-height: calc(100vh - 130px);
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
    z-index: 399;
    overflow: hidden;
  }
  .ai-head {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 10px 8px 14px;
    border-bottom: 1px solid var(--border);
  }
  .ai-title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
  }
  .ai-connect {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 24px;
    text-align: center;
    color: var(--text-dim);
  }
  .thread {
    flex: 1;
    overflow-y: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .hint {
    color: var(--text-dim);
    font-size: 12.5px;
    text-align: center;
    margin: auto 12px;
  }
  .msg {
    max-width: 92%;
    border-radius: 10px;
    padding: 8px 11px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .msg.user {
    align-self: flex-end;
    background: var(--bg-active);
  }
  .msg.ai {
    align-self: flex-start;
    background: var(--bg);
    border: 1px solid var(--border);
  }
  .msg-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .pulse {
    color: var(--text-dim);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
  .msg-actions,
  .sources {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 6px;
  }
  .msg-actions button,
  .sources button {
    font-size: 11px;
    padding: 2px 8px;
    color: var(--accent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
  }
  .chip {
    font-size: 11.5px;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--bg);
  }
  .chip.on {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg));
    border-color: var(--accent);
  }
  .ai-input {
    display: flex;
    gap: 8px;
    padding: 10px 12px 12px;
  }
  .ai-input input {
    flex: 1;
    border-radius: 10px;
  }
  .send {
    width: 34px;
    border-radius: 10px;
    background: var(--accent);
    border-color: var(--accent);
    color: #10121a;
    font-weight: 700;
  }
</style>
